//! Real vision perception — the third genuine perception organ (MasterPlan #15).
//!
//! The constitution's missing piece: *"No camera/video pipeline … not sensing."*
//! This sensor runs the real pipeline:
//!
//! ```text
//! webcam (Media Foundation device source)
//!      ↓
//! RGB32 frame (w × h × 4)
//!      ↓
//! pure frame differencing (motion = pixels that changed)
//!      ↓
//! pure connected-component blob detection → DetectedObject { x, y, w, h, confidence }
//!      ↓
//! optional ML seam (ObjectDetector trait)
//!      ↓
//! Topic::ObjectDetected event → SensorLoop → GlobalWorkspace
//! ```
//!
//! Honesty rules, mirroring the screen/audio sensors:
//!
//! - **One frame sees nothing.** Motion is detected *between* frames: the first
//!   frame is a baseline, only subsequent frames can produce events. A camera
//!   that never changes produces nothing.
//! - **No camera → no events.** Media Foundation open failure (headless,
//!   no capture device, permission) degrades the sensor to silence — it never
//!   invents an object.
//! - **The detector is real and deterministic.** [`changed_regions`] is a pure
//!   connected-component blob detector over the frame diff — genuine computer
//!   vision that needs no model, fully unit-tested with synthetic frames. An ML
//!   backend (YOLO/ONNX) plugs into the [`ObjectDetector`] trait; until then
//!   the event reports `detector: "diff"` — never a fabricated label.
//!
//! Only the completed event (a few hundred bytes) enters the workspace — never
//! raw frames (small-specialist → event → core).

use std::sync::Arc;

use async_trait::async_trait;
use friday_cognitive::workspace::Topic;
use friday_cognitive::WorkspaceEvent;

use crate::Sensor;

/// A raw RGB32 (BGRA) frame. `pixels.len() == width * height * 4`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

/// A detected object: a changed region of the frame with its bounding box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedObject {
    pub label: String,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// Fraction of the box that actually changed (0..1 scaled to 0..100).
    pub confidence: u32,
}

/// Per-pixel absolute difference of two same-sized frames, as a luma
/// (perceptual brightness) map. `None` when dimensions differ. Pure.
pub fn frame_diff(a: &Frame, b: &Frame) -> Option<Vec<u8>> {
    if a.width != b.width || a.height != b.height || a.pixels.len() != b.pixels.len() {
        return None;
    }
    Some(
        a.pixels
            .chunks_exact(4)
            .zip(b.pixels.chunks_exact(4))
            .map(|(pa, pb)| {
                // BT.601 luma from the BGRA byte order.
                let la = (pa[2] as u32 * 299 + pa[1] as u32 * 587 + pa[0] as u32 * 114) / 1000;
                let lb = (pb[2] as u32 * 299 + pb[1] as u32 * 587 + pb[0] as u32 * 114) / 1000;
                la.abs_diff(lb) as u8
            })
            .collect(),
    )
}

/// Fraction of pixels (0.0..1.0) whose diff exceeds `threshold` — the motion
/// score. Pure.
pub fn motion_score(diff: &[u8], threshold: u8) -> f32 {
    if diff.is_empty() {
        return 0.0;
    }
    let changed = diff.iter().filter(|&&d| d > threshold).count();
    changed as f32 / diff.len() as f32
}

/// Connected-component blob detection over a thresholded diff map. Returns
/// one [`DetectedObject`] per connected changed region (4-connected, scanline
/// flood fill), with confidence = changed-pixels / box-area. Pure and
/// deterministic — the built-in vision model.
pub fn changed_regions(diff: &[u8], width: u32, height: u32, threshold: u8) -> Vec<DetectedObject> {
    let (w, h) = (width as usize, height as usize);
    if diff.len() != w * h || w == 0 || h == 0 {
        return Vec::new();
    }

    let mut visited = vec![false; w * h];
    let mut objects = Vec::new();

    for start in 0..w * h {
        if visited[start] || diff[start] <= threshold {
            continue;
        }
        // BFS flood fill from `start`.
        let mut stack = vec![start];
        visited[start] = true;
        let (mut min_x, mut min_y, mut max_x, mut max_y) = (usize::MAX, usize::MAX, 0usize, 0usize);
        let mut changed = 0usize;
        while let Some(p) = stack.pop() {
            let (x, y) = (p % w, p / w);
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
            changed += 1;
            for (nx, ny) in [
                (x.wrapping_sub(1), y),
                (x + 1, y),
                (x, y.wrapping_sub(1)),
                (x, y + 1),
            ] {
                if nx < w && ny < h {
                    let np = ny * w + nx;
                    if !visited[np] && diff[np] > threshold {
                        visited[np] = true;
                        stack.push(np);
                    }
                }
            }
        }
        let (bw, bh) = (max_x - min_x + 1, max_y - min_y + 1);
        let confidence = ((changed as u32 * 100) / (bw * bh) as u32).min(100);
        objects.push(DetectedObject {
            label: "motion".into(),
            x: min_x as u32,
            y: min_y as u32,
            width: bw as u32,
            height: bh as u32,
            confidence,
        });
    }

    objects
}

/// ML detector seam — the frozen plug-in point for real object models
/// (YOLO/ONNX). Until attached, the built-in diff/blob detector is used and
/// the event says `detector: "diff"`.
#[async_trait]
pub trait ObjectDetector: Send + Sync {
    fn name(&self) -> &str;

    /// Detect objects in a single frame. `Ok(empty)` = nothing found.
    async fn detect(&self, frame: &Frame) -> Result<Vec<DetectedObject>, String>;
}

/// Deterministic detector for tests/CI: returns a fixed object.
pub struct MockDetector {
    pub object: DetectedObject,
}

#[async_trait]
impl ObjectDetector for MockDetector {
    fn name(&self) -> &str {
        "mock"
    }

    async fn detect(&self, _frame: &Frame) -> Result<Vec<DetectedObject>, String> {
        Ok(vec![self.object.clone()])
    }
}

/// Build the `sensor.vision.objects` workspace event. Pure. `detector` names
/// the model that produced the objects ("diff" for the built-in).
pub fn build_object_event(
    frame: &Frame,
    objects: &[DetectedObject],
    detector: &str,
    motion: f32,
) -> WorkspaceEvent {
    WorkspaceEvent::new(
        Topic::ObjectDetected,
        serde_json::json!({
            "count": objects.len(),
            "motion": (motion * 1000.0).round() as u64,
            "detector": detector,
            "frame": { "width": frame.width, "height": frame.height },
            "objects": objects.iter().map(|o| serde_json::json!({
                "label": o.label,
                "x": o.x,
                "y": o.y,
                "width": o.width,
                "height": o.height,
                "confidence": o.confidence,
            })).collect::<Vec<_>>(),
        }),
    )
}

/// The real vision sensor: webcam capture → frame diff → blob detection →
/// optional ML seam → [`Topic::ObjectDetected`] events. On Windows it captures
/// the first Media Foundation video device; elsewhere (or with no camera) it
/// stays silent. `process_frame` is exposed so the pipeline is fully testable
/// over synthetic frames everywhere.
pub struct CameraSensor {
    detector: Option<Arc<dyn ObjectDetector>>,
    capture: Option<MfCapture>,
    /// The previous frame (baseline for the diff). `None` = first frame seen.
    last: Option<Frame>,
    /// Diff threshold (0..255) above which a pixel counts as changed.
    pub diff_threshold: u8,
    /// Min motion score to bother emitting (avoids sensor-noise spam).
    pub min_motion: f32,
}

impl Default for CameraSensor {
    fn default() -> Self {
        Self::new()
    }
}

impl CameraSensor {
    pub fn new() -> Self {
        Self {
            detector: None,
            capture: open_default_camera(),
            last: None,
            diff_threshold: 24,
            min_motion: 0.002,
        }
    }

    /// Build a sensor with no capture device (headless/CI, or only the pure
    /// pipeline is wanted).
    pub fn without_capture() -> Self {
        Self {
            detector: None,
            capture: None,
            last: None,
            diff_threshold: 24,
            min_motion: 0.002,
        }
    }

    /// Attach an ML detector. Until then, the built-in diff/blob detector
    /// runs and events say `detector: "diff"`.
    pub fn with_detector(mut self, detector: Arc<dyn ObjectDetector>) -> Self {
        self.detector = Some(detector);
        self
    }

    /// Whether a capture device is actually open (headless/CI = false).
    pub fn capture_available(&self) -> bool {
        self.capture.is_some()
    }

    /// Pure pipeline entry: feed the next frame and get any diff-detected
    /// objects plus the motion score. The first frame is a baseline — it
    /// detects nothing. ML-seam results are obtained separately via
    /// [`CameraSensor::process_detections`] (async).
    pub fn process_frame(&mut self, frame: &Frame) -> (Vec<DetectedObject>, f32) {
        let mut score = 0.0f32;
        let objects = match &self.last {
            Some(prev) => match frame_diff(prev, frame) {
                Some(d) => {
                    score = motion_score(&d, self.diff_threshold);
                    if score >= self.min_motion {
                        changed_regions(&d, frame.width, frame.height, self.diff_threshold)
                    } else {
                        Vec::new()
                    }
                }
                None => Vec::new(),
            },
            None => Vec::new(),
        };
        self.last = Some(frame.clone());
        (objects, score)
    }

    /// Async half: run the ML seam over a frame when attached. `None` = no
    /// seam (the diff path is used instead) or the seam failed — honest.
    pub async fn process_detections(&self, frame: &Frame) -> Option<Vec<DetectedObject>> {
        match &self.detector {
            Some(det) => det.detect(frame).await.ok(),
            None => None,
        }
    }
}

#[async_trait]
impl Sensor for CameraSensor {
    fn name(&self) -> &str {
        "camera"
    }

    fn topic_prefix(&self) -> &str {
        "sensor.vision"
    }

    async fn poll(&mut self) -> Vec<WorkspaceEvent> {
        let Some(capture) = &self.capture else {
            return Vec::new();
        };
        let Some(frame) = capture.read_frame() else {
            return Vec::new();
        };

        // An ML seam replaces the diff path entirely — no wasted diff compute
        // and no mixed provenance in the event (detector and objects come from
        // the same source).
        if let Some(det) = &self.detector {
            let objects = match det.detect(&frame).await {
                Ok(objs) if !objs.is_empty() => objs,
                _ => return Vec::new(),
            };
            let detector = det.name().to_string();
            return vec![build_object_event(&frame, &objects, &detector, 0.0)];
        }

        // Built-in diff path (keeps the first-frame baseline semantics).
        let (objects, score) = self.process_frame(&frame);
        if objects.is_empty() {
            return Vec::new();
        }
        vec![build_object_event(&frame, &objects, "diff", score)]
    }
}

// ── Real capture (Windows only) ───────────────────────────────────

/// A live Media Foundation webcam source reader. Wrapped in `Option` so a
/// sensor without a device (headless/CI) is quiet rather than broken.
/// `Drop` releases the reader and pairs the `MFStartup` from open with
/// `MFShutdown` (MF refcounts, so per-instance pairing is correct).
#[cfg(target_os = "windows")]
struct MfCapture {
    reader: Option<windows::Win32::Media::MediaFoundation::IMFSourceReader>,
}

#[cfg(target_os = "windows")]
impl Drop for MfCapture {
    fn drop(&mut self) {
        // Release the reader (and the source it holds) before shutting MF down.
        self.reader = None;
        unsafe {
            let _ = windows::Win32::Media::MediaFoundation::MFShutdown();
        }
    }
}

#[cfg(not(target_os = "windows"))]
struct MfCapture {}

#[cfg(not(target_os = "windows"))]
impl MfCapture {
    // Never constructed on non-Windows; mirrors the Windows surface so the
    // platform-neutral `Sensor::poll` compiles on every target.
    fn read_frame(&self) -> Option<Frame> {
        None
    }
}

#[cfg(target_os = "windows")]
unsafe impl Send for MfCapture {}
#[cfg(target_os = "windows")]
unsafe impl Sync for MfCapture {}

/// Open the first Media Foundation video capture device and negotiate RGB32.
/// `None` = no device / permission / MF failure — an honest silence.
#[cfg(target_os = "windows")]
fn open_default_camera() -> Option<MfCapture> {
    use windows::Win32::Media::MediaFoundation::{
        MFCreateAttributes, MFCreateMediaType, MFCreateSourceReaderFromMediaSource,
        MFEnumDeviceSources, MFMediaType_Video, MFStartup, MFVideoFormat_RGB32, MFSTARTUP_FULL,
        MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_SOURCE_READER_FIRST_VIDEO_STREAM, MF_VERSION,
    };
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    unsafe {
        // MTA COM + MF: callable from any tokio worker thread.
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            return None;
        }
        if MFStartup(MF_VERSION, MFSTARTUP_FULL).is_err() {
            return None;
        }

        // Enumerate video capture devices.
        let mut attrs: Option<windows::Win32::Media::MediaFoundation::IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 1).ok()?;
        let attrs = attrs?;
        attrs
            .SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
            .ok()?;

        let mut activates: *mut Option<windows::Win32::Media::MediaFoundation::IMFActivate> =
            std::ptr::null_mut();
        let mut count: u32 = 0;
        if MFEnumDeviceSources(&attrs, &mut activates, &mut count).is_err() || count == 0 {
            return None;
        }
        // Take the first device; release the rest; free the array MF allocated.
        let activate = (*activates).take()?;
        for i in 1..count as usize {
            let _ = (*activates.add(i)).take();
        }
        windows::Win32::System::Com::CoTaskMemFree(Some(activates as *mut core::ffi::c_void));

        let source: windows::Win32::Media::MediaFoundation::IMFMediaSource =
            activate.ActivateObject().ok()?;
        let reader = MFCreateSourceReaderFromMediaSource(&source, None).ok()?;
        let _ = source;

        // Negotiate RGB32 so we get raw BGRA pixels, no color conversion.
        let media_type = MFCreateMediaType().ok()?;
        media_type
            .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
            .ok()?;
        media_type
            .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
            .ok()?;
        let first_video = MF_SOURCE_READER_FIRST_VIDEO_STREAM;
        reader
            .SetCurrentMediaType(first_video.0 as u32, None, &media_type)
            .ok()?;

        Some(MfCapture {
            reader: Some(reader),
        })
    }
}

#[cfg(not(target_os = "windows"))]
fn open_default_camera() -> Option<MfCapture> {
    None
}

#[cfg(target_os = "windows")]
impl MfCapture {
    /// Grab one RGB32 frame. `None` on error / end-of-stream / no frame yet —
    /// the sensor goes quiet rather than panicking.
    fn read_frame(&self) -> Option<Frame> {
        use windows::Win32::Media::MediaFoundation::{
            MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
        };

        let first_video = MF_SOURCE_READER_FIRST_VIDEO_STREAM;
        let reader = self.reader.as_ref()?;
        let mut sample: Option<windows::Win32::Media::MediaFoundation::IMFSample> = None;
        let mut stream_flags: u32 = 0;
        unsafe {
            reader
                .ReadSample(
                    first_video.0 as u32,
                    0,
                    None,
                    Some(&mut stream_flags),
                    None,
                    Some(&mut sample),
                )
                .ok()?;
            // Media-type change: keep reading until an actual frame arrives
            // (re-checking the flag each pass, not just once).
            while stream_flags & (MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32) != 0 {
                sample = None;
                stream_flags = 0;
                reader
                    .ReadSample(
                        first_video.0 as u32,
                        0,
                        None,
                        Some(&mut stream_flags),
                        None,
                        Some(&mut sample),
                    )
                    .ok()?;
            }
            let sample = sample?;
            let buffer = sample.GetBufferByIndex(0).ok()?;
            let mut data: *mut u8 = std::ptr::null_mut();
            let mut len: u32 = 0;
            buffer.Lock(&mut data, None, Some(&mut len)).ok()?;
            let pixels = std::slice::from_raw_parts(data, len as usize).to_vec();
            buffer.Unlock().ok()?;
            pixels_to_frame(pixels, self.frame_size())
        }
    }

    /// Query MF_MT_FRAME_SIZE (packed width<<32 | height) from the reader.
    unsafe fn frame_size(&self) -> Option<(u32, u32)> {
        use windows::Win32::Media::MediaFoundation::{
            MF_MT_FRAME_SIZE, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
        };
        let media_type = self
            .reader
            .as_ref()?
            .GetCurrentMediaType(MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32)
            .ok()?;
        let packed = media_type.GetUINT64(&MF_MT_FRAME_SIZE).ok()?;
        Some(((packed >> 32) as u32, (packed & 0xFFFF_FFFF) as u32))
    }
}

/// RGB32 (BGRA) buffer → [`Frame`]. `size` comes from the negotiated media
/// type; if the reader cannot report it, the frame is refused (honest — we
/// never guess an aspect ratio that would mislabel every coordinate).
#[cfg(target_os = "windows")]
fn pixels_to_frame(pixels: Vec<u8>, size: Option<(u32, u32)>) -> Option<Frame> {
    if pixels.len() % 4 != 0 || pixels.is_empty() {
        return None;
    }
    let (width, height) = size?;
    if width == 0 || height == 0 || (width * height * 4) as usize != pixels.len() {
        return None;
    }
    Some(Frame {
        width,
        height,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(w: u32, h: u32, fill: u8) -> Frame {
        Frame {
            width: w,
            height: h,
            pixels: vec![fill; (w * h * 4) as usize],
        }
    }

    fn frame_with_rect(w: u32, h: u32, base: u8, rect: (u32, u32, u32, u32), fill: u8) -> Frame {
        let mut pixels = vec![base; (w * h * 4) as usize];
        let (rx, ry, rw, rh) = rect;
        for y in ry..ry + rh {
            for x in rx..rx + rw {
                let i = ((y * w + x) * 4) as usize;
                pixels[i..i + 4].fill(fill);
            }
        }
        Frame {
            width: w,
            height: h,
            pixels,
        }
    }

    #[test]
    fn diff_is_none_for_mismatched_frames() {
        let a = frame(4, 4, 10);
        let b = frame(8, 8, 10);
        assert!(frame_diff(&a, &b).is_none());
    }

    #[test]
    fn diff_highlights_only_changed_pixels() {
        let a = frame(4, 4, 10);
        let b = frame_with_rect(4, 4, 10, (1, 1, 2, 2), 200);
        let d = frame_diff(&a, &b).unwrap();
        // 2x2 changed region inside a 4x4 frame.
        assert!(motion_score(&d, 24) > 0.2);
        assert!(motion_score(&d, 24) < 0.3);
    }

    #[test]
    fn identical_frames_have_zero_motion() {
        let a = frame(4, 4, 10);
        let d = frame_diff(&a, &a).unwrap();
        assert_eq!(motion_score(&d, 24), 0.0);
    }

    #[test]
    fn changed_regions_finds_one_blob_with_confidence() {
        let a = frame(8, 8, 10);
        let b = frame_with_rect(8, 8, 10, (2, 2, 3, 3), 200);
        let d = frame_diff(&a, &b).unwrap();
        let blobs = changed_regions(&d, 8, 8, 24);
        assert_eq!(blobs.len(), 1);
        assert_eq!(blobs[0].x, 2);
        assert_eq!(blobs[0].y, 2);
        assert_eq!(blobs[0].width, 3);
        assert_eq!(blobs[0].height, 3);
        assert_eq!(blobs[0].confidence, 100); // fully changed box
    }

    #[test]
    fn changed_regions_splits_disconnected_blobs() {
        let a = frame(10, 10, 10);
        // Two disconnected changed regions in one frame.
        let mut b = frame_with_rect(10, 10, 10, (1, 1, 2, 2), 200);
        for y in 7..9 {
            for x in 7..9 {
                let i = ((y * 10 + x) * 4) as usize;
                b.pixels[i..i + 4].fill(200);
            }
        }
        let d = frame_diff(&a, &b).unwrap();
        let blobs = changed_regions(&d, 10, 10, 24);
        assert_eq!(blobs.len(), 2);
    }

    #[test]
    fn event_payload_is_honest_and_well_formed() {
        let f = frame(8, 8, 10);
        let objs = vec![DetectedObject {
            label: "motion".into(),
            x: 2,
            y: 2,
            width: 3,
            height: 3,
            confidence: 100,
        }];
        let event = build_object_event(&f, &objs, "diff", 0.25);
        assert_eq!(event.topic, Topic::ObjectDetected);
        assert_eq!(event.payload["count"], 1);
        assert_eq!(event.payload["detector"], "diff");
        assert_eq!(event.payload["objects"][0]["label"], "motion");
        assert_eq!(event.payload["objects"][0]["confidence"], 100);
        assert_eq!(event.payload["frame"]["width"], 8);
    }

    #[test]
    fn first_frame_is_baseline_and_detects_nothing() {
        let mut sensor = CameraSensor::without_capture();
        let f1 = frame(8, 8, 10);
        let (objects, score) = sensor.process_frame(&f1);
        assert!(objects.is_empty());
        assert_eq!(score, 0.0);
        // Second frame with a change → object detected.
        let f2 = frame_with_rect(8, 8, 10, (2, 2, 3, 3), 200);
        let (objects, _) = sensor.process_frame(&f2);
        assert_eq!(objects.len(), 1);
        assert_eq!(objects[0].width, 3);
    }

    #[test]
    fn static_scene_never_detects_after_baseline() {
        let mut sensor = CameraSensor::without_capture();
        let f = frame(8, 8, 10);
        let _ = sensor.process_frame(&f);
        for _ in 0..5 {
            let (objects, _) = sensor.process_frame(&f);
            assert!(objects.is_empty());
        }
    }

    #[tokio::test]
    async fn camera_events_flow_through_the_sensor_loop() {
        use crate::SensorLoop;

        let mut sensor = CameraSensor::without_capture();
        let f1 = frame(8, 8, 10);
        let f2 = frame_with_rect(8, 8, 10, (2, 2, 3, 3), 200);
        let _ = sensor.process_frame(&f1);
        let (objects, _) = sensor.process_frame(&f2);
        let event = build_object_event(&f2, &objects, "diff", 0.25);
        assert_eq!(event.topic, Topic::ObjectDetected);

        let mut loop_ = SensorLoop::new();
        loop_.add(Box::new(crate::MockSensor::new(
            "vision_events",
            vec![event],
        )));
        let workspace = friday_cognitive::workspace::GlobalWorkspace::new();
        let topics = loop_.poll_all_with_topics(&workspace).await;
        assert_eq!(topics, vec!["sensor.vision.objects"]);
        assert_eq!(workspace.history(None).len(), 1);
    }

    #[tokio::test]
    async fn ml_seam_reports_its_own_objects() {
        let detector: Arc<dyn ObjectDetector> = Arc::new(MockDetector {
            object: DetectedObject {
                label: "person".into(),
                x: 1,
                y: 1,
                width: 2,
                height: 2,
                confidence: 80,
            },
        });
        let sensor = CameraSensor::without_capture().with_detector(detector);
        let f = frame(8, 8, 10);
        let detected = sensor.process_detections(&f).await;
        assert_eq!(detected.unwrap()[0].label, "person");
    }
}
