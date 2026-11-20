//! `friday-ctl watch`: say when something finishes or changes, so you can look
//! away. Everything is watched on this computer; nothing is sent anywhere.
//!
//! - `watch kitty <socket> <window id> <command>`: the command running in a
//!   kitty window; told when the window is back at its prompt, with the exit
//!   status (kitty's remote control, `kitten @ ls`).
//! - `watch region <geometry>`: an area of the screen (`grim -g`, compared
//!   pixel by pixel). A still area: told when it changes. One that was already
//!   changing (a progress bar): told when it stops.
//! - `watch stop`: stop the watch. One watch at a time; a new one replaces it.

use serde_json::Value;

/// How long a watch lasts at most.
pub const MAX_SECS: u64 = 2 * 60 * 60;
/// Seconds between looks.
pub const EVERY_SECS: u64 = 2;

/// A kitty window's state, from `kitten @ ls`.
#[derive(Debug, PartialEq)]
pub enum Kitty {
    Running,
    Finished(i64),
    Gone,
}

pub fn kitty_state(ls: &str, id: u64) -> Kitty {
    let Ok(v) = serde_json::from_str::<Value>(ls) else {
        return Kitty::Gone;
    };
    let win = v
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|os| os["tabs"].as_array())
        .flatten()
        .filter_map(|t| t["windows"].as_array())
        .flatten()
        .find(|w| w["id"].as_u64() == Some(id));
    match win {
        None => Kitty::Gone,
        Some(w) if w["at_prompt"].as_bool() == Some(true) => {
            Kitty::Finished(w["last_cmd_exit_status"].as_i64().unwrap_or(0))
        }
        Some(_) => Kitty::Running,
    }
}

/// What to say when a command finished: (title, body).
pub fn finished_message(cmd: &str, status: i64) -> (String, String) {
    let cmd: String = cmd.chars().take(60).collect();
    let body = match status {
        0 => "It worked (exit 0).".to_string(),
        130 => "It was stopped (Ctrl+C).".to_string(),
        n => format!("It failed (exit {n}). Press Super+Y there to have it explained."),
    };
    (format!("Finished: {cmd}"), body)
}

/// The pixels of a binary PPM (`P6`, what `grim -t ppm` writes): width,
/// height and the RGB bytes.
pub fn parse_ppm(data: &[u8]) -> Option<(usize, usize, &[u8])> {
    let mut fields = Vec::new();
    let mut i = 0;
    while fields.len() < 4 {
        while data.get(i)?.is_ascii_whitespace() {
            i += 1;
        }
        if data[i] == b'#' {
            while *data.get(i)? != b'\n' {
                i += 1;
            }
            continue;
        }
        let start = i;
        while !data.get(i)?.is_ascii_whitespace() {
            i += 1;
        }
        fields.push(std::str::from_utf8(&data[start..i]).ok()?);
    }
    // One whitespace byte, then the pixels.
    i += 1;
    if fields[0] != "P6" || fields[3] != "255" {
        return None;
    }
    let (w, h): (usize, usize) = (fields[1].parse().ok()?, fields[2].parse().ok()?);
    let end = i.checked_add(w.checked_mul(h)?.checked_mul(3)?)?;
    let pixels = data.get(i..end)?;
    Some((w, h, pixels))
}

/// The share of pixels (0 to 1) that differ visibly between two frames; 1
/// when their sizes differ.
pub fn changed_share(a: &[u8], b: &[u8]) -> f64 {
    let (Some((aw, ah, ap)), Some((bw, bh, bp))) = (parse_ppm(a), parse_ppm(b)) else {
        return 1.0;
    };
    if (aw, ah) != (bw, bh) || ap.is_empty() {
        return 1.0;
    }
    let differ = ap
        .chunks_exact(3)
        .zip(bp.chunks_exact(3))
        .filter(|(p, q)| p.iter().zip(q.iter()).any(|(x, y)| x.abs_diff(*y) > 24))
        .count();
    differ as f64 / (ap.len() / 3) as f64
}

/// More than this share of pixels changed counts as a change.
const CHANGED: f64 = 0.005;
/// Looks at the start that decide whether the area was already changing.
const SETTLE_LOOKS: usize = 3;
/// Still looks in a row that count as "stopped changing".
const STILL_LOOKS: usize = 5;

/// What a region watch tells the person.
#[derive(Debug, PartialEq)]
pub enum RegionEvent {
    Changed,
    Stopped,
}

/// A region watch: fed one frame per look.
#[derive(Default)]
pub struct Region {
    first: Option<Vec<u8>>,
    last: Option<Vec<u8>>,
    looks: usize,
    busy_at_start: bool,
    changed_in_a_row: usize,
    still_in_a_row: usize,
}

impl Region {
    pub fn look(&mut self, frame: Vec<u8>) -> Option<RegionEvent> {
        self.looks += 1;
        let Some(first) = &self.first else {
            self.first = Some(frame.clone());
            self.last = Some(frame);
            return None;
        };
        let since_last = changed_share(self.last.as_deref().unwrap_or_default(), &frame);
        let since_first = changed_share(first, &frame);
        self.last = Some(frame);
        if self.looks <= SETTLE_LOOKS {
            self.busy_at_start |= since_last > CHANGED;
            return None;
        }
        if self.busy_at_start {
            if since_last > CHANGED {
                self.still_in_a_row = 0;
            } else {
                self.still_in_a_row += 1;
            }
            (self.still_in_a_row >= STILL_LOOKS).then_some(RegionEvent::Stopped)
        } else {
            if since_first > CHANGED {
                self.changed_in_a_row += 1;
            } else {
                self.changed_in_a_row = 0;
            }
            (self.changed_in_a_row >= 2).then_some(RegionEvent::Changed)
        }
    }
}

/// A geometry as `slurp` prints it: `X,Y WxH`.
pub fn valid_geometry(g: &str) -> bool {
    let Some((xy, wh)) = g.split_once(' ') else {
        return false;
    };
    let nums = |s: &str, sep: char| {
        s.split_once(sep).is_some_and(|(a, b)| {
            a.trim_start_matches('-').parse::<u32>().is_ok()
                && b.trim_start_matches('-').parse::<u32>().is_ok()
        })
    };
    nums(xy, ',') && nums(wh, 'x')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ls(at_prompt: bool, status: i64) -> String {
        format!(
            r#"[{{"tabs":[{{"windows":[{{"id":3,"at_prompt":false}},{{"id":7,"at_prompt":{at_prompt},"last_cmd_exit_status":{status}}}]}}]}}]"#
        )
    }

    #[test]
    fn a_kitty_window_is_running_then_finished_or_gone() {
        assert_eq!(kitty_state(&ls(false, 0), 7), Kitty::Running);
        assert_eq!(kitty_state(&ls(true, 2), 7), Kitty::Finished(2));
        assert_eq!(kitty_state(&ls(true, 0), 9), Kitty::Gone);
        assert_eq!(kitty_state("not json", 7), Kitty::Gone);
        let (title, body) = finished_message("npm run build", 2);
        assert_eq!(title, "Finished: npm run build");
        assert!(body.contains("exit 2"), "{body}");
        assert!(finished_message("x", 0).1.contains("worked"));
    }

    fn frame(w: usize, h: usize, lit: usize) -> Vec<u8> {
        let mut out = format!("P6\n# grim\n{w} {h}\n255\n").into_bytes();
        for i in 0..w * h {
            out.extend(if i < lit { [255, 255, 255] } else { [0, 0, 0] });
        }
        out
    }

    #[test]
    fn frames_are_compared_pixel_by_pixel() {
        assert_eq!(parse_ppm(&frame(4, 2, 0)).unwrap().0, 4);
        assert!(parse_ppm(b"P6\n4 2\n255\nshort").is_none());
        let huge = format!("P6\n{} 1\n255\n", usize::MAX / 3);
        assert!(parse_ppm(huge.as_bytes()).is_none());
        assert!(parse_ppm(b"").is_none());
        assert_eq!(changed_share(&frame(10, 10, 0), &frame(10, 10, 0)), 0.0);
        assert_eq!(changed_share(&frame(10, 10, 0), &frame(10, 10, 25)), 0.25);
        assert_eq!(changed_share(&frame(10, 10, 0), &frame(5, 10, 0)), 1.0);
        assert_eq!(changed_share(b"junk", &frame(1, 1, 0)), 1.0);
    }

    #[test]
    fn a_still_area_tells_when_it_changes_twice_in_a_row() {
        let mut r = Region::default();
        for _ in 0..4 {
            assert_eq!(r.look(frame(20, 20, 0)), None);
        }
        // One flicker isn't a change.
        assert_eq!(r.look(frame(20, 20, 50)), None);
        assert_eq!(r.look(frame(20, 20, 0)), None);
        assert_eq!(r.look(frame(20, 20, 50)), None);
        assert_eq!(r.look(frame(20, 20, 50)), Some(RegionEvent::Changed));
    }

    #[test]
    fn a_busy_area_tells_when_it_stops_changing() {
        let mut r = Region::default();
        let mut lit = 0;
        for _ in 0..6 {
            lit += 20;
            assert_eq!(r.look(frame(20, 20, lit)), None);
        }
        for i in 0..STILL_LOOKS {
            let got = r.look(frame(20, 20, lit));
            assert_eq!(got.is_some(), i == STILL_LOOKS - 1, "look {i}");
        }
    }

    #[test]
    fn only_slurp_geometry_is_accepted() {
        assert!(valid_geometry("10,20 300x200"));
        assert!(valid_geometry("-1920,0 300x200"));
        assert!(!valid_geometry("10,20"));
        assert!(!valid_geometry("10,20 300x200; rm -rf ~"));
    }
}
