//! Local AI sizing, shared by Linux and Windows: what the machine has, how
//! much of it local AI may use, and the model each budget gets. This is the
//! Rust twin of `friday local-ai detect|presets` in the bash script (kept
//! while the installer still calls that); `tests/local_ai_parity.rs` checks
//! the two give the same answers.
//!
//! `friday-ctl local-ai detect|presets|model <budget_mb>|local-first <model>
//! <auto|local|cloud> [tokens/s]` print the same lines as the bash ones, so
//! the Windows tray reads them too.

use std::path::{Path, PathBuf};

/// One rung of the ladder.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Model {
    pub id: &'static str,
    /// Memory it needs while answering (MB).
    pub need_mb: u64,
    /// Its largest download, Ollama or GGUF (MB).
    pub download_mb: u64,
    /// Whether it sees screenshots.
    pub vision: bool,
}

pub const LADDER: &[Model] = &[
    Model {
        id: "qwen3:1.7b",
        need_mb: 2048,
        download_mb: 1834,
        vision: false,
    },
    Model {
        id: "qwen3:4b",
        need_mb: 3584,
        download_mb: 2497,
        vision: false,
    },
    Model {
        id: "gemma3:4b",
        need_mb: 4608,
        download_mb: 3350,
        vision: true,
    },
    Model {
        id: "gemma3:12b",
        need_mb: 9216,
        download_mb: 8164,
        vision: true,
    },
    Model {
        id: "gemma3:27b",
        need_mb: 19456,
        download_mb: 17413,
        vision: true,
    },
];

/// The largest ladder model that fits the budget.
pub fn model_for_budget(budget_mb: u64) -> Option<&'static Model> {
    LADDER.iter().rev().find(|m| budget_mb >= m.need_mb)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Order {
    Auto,
    Local,
    Cloud,
}

impl Order {
    pub fn parse(s: &str) -> Order {
        match s {
            "local" => Order::Local,
            "cloud" => Order::Cloud,
            _ => Order::Auto,
        }
    }
}

/// Slower than this (tokens a second) and a big model backs up cloud too.
pub const MIN_TPS: f64 = 8.0;

/// Whether the local model answers first. Auto: above 4B it answers about as
/// well as the free cloud models, so it goes first (unless a measured speed
/// is under `MIN_TPS`); at 4B and below cloud goes first while online.
pub fn local_first(model: &str, order: Order, tps: Option<f64>) -> bool {
    match order {
        Order::Local => true,
        Order::Cloud => false,
        Order::Auto => {
            // The number the size starts with ("12b", "8B-it": awk reads it
            // the same way).
            let size = model.rsplit(':').next().unwrap_or("");
            let digits: String = size
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            let size = digits.parse::<f64>().unwrap_or(0.0);
            size > 4.0 && tps.is_none_or(|t| t >= MIN_TPS)
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hardware {
    pub ram_mb: u64,
    /// `none`, `nvidia`, `amd` or `intel`.
    pub gpu: &'static str,
    pub gpu_discrete: bool,
    /// `None` when it can't be told (an NVIDIA card with no driver loaded).
    pub vram_mb: Option<u64>,
    pub cpu_threads: u64,
    pub avx2: bool,
    pub disk_free_mb: u64,
}

impl Hardware {
    /// The `key=value` lines `friday local-ai detect` prints.
    pub fn lines(&self) -> String {
        format!(
            "ram_mb={}\ngpu={}\ngpu_discrete={}\nvram_mb={}\ncpu_threads={}\navx2={}\ndisk_free_mb={}\n",
            self.ram_mb,
            self.gpu,
            if self.gpu_discrete { "yes" } else { "no" },
            self.vram_mb.map_or("unknown".to_string(), |v| v.to_string()),
            self.cpu_threads,
            if self.avx2 { "yes" } else { "no" },
            self.disk_free_mb,
        )
    }
}

/// What the machine has. `FRIDAY_HW_ROOT` points the Linux scan at a fake
/// /proc and /sys tree and `FRIDAY_DISK_FREE_MB` fixes the free disk, as
/// for the bash script (its tests use both).
pub fn detect() -> Hardware {
    let mut hw = platform();
    hw.disk_free_mb = match std::env::var("FRIDAY_DISK_FREE_MB")
        .ok()
        .and_then(|v| v.trim().parse().ok())
    {
        Some(mb) => mb,
        None => disk_free_mb(),
    };
    hw
}

fn disk_free_mb() -> u64 {
    let home = std::env::var_os("FRIDAY_DISK_PATH")
        .or_else(|| std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }))
        .map(PathBuf::from)
        .unwrap_or_default();
    let disks = sysinfo::Disks::new_with_refreshed_list();
    // Windows paths don't care about case ("c:\\users" is "C:\\Users").
    let fold = |p: &Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        if cfg!(windows) {
            s.to_lowercase()
        } else {
            s
        }
    };
    let home_s = fold(&home);
    // The disk whose mount point is the longest prefix of the home folder;
    // for a home on no local disk (a network share), the roomiest one.
    let own = disks
        .list()
        .iter()
        .filter(|d| {
            let m = fold(d.mount_point());
            home_s == m.trim_end_matches('/')
                || home_s.starts_with(&format!("{}/", m.trim_end_matches('/')))
        })
        .max_by_key(|d| d.mount_point().as_os_str().len());
    own.or_else(|| disks.list().iter().max_by_key(|d| d.available_space()))
        .map_or(0, |d| d.available_space() / 1_048_576)
}

fn nvidia_smi_mb() -> Option<u64> {
    let out = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=memory.total", "--format=csv,noheader,nounits"])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let digits: String = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()?
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    digits.parse().ok().filter(|_| out.status.success())
}

#[cfg(not(windows))]
fn platform() -> Hardware {
    let root = std::env::var("FRIDAY_HW_ROOT").unwrap_or_default();
    scan_linux(Path::new(&root))
}

/// Reads /proc and /sys under `root` ("" for the real ones).
pub fn scan_linux(root: &Path) -> Hardware {
    let at = |rel: &str| PathBuf::from(format!("{}{rel}", root.display()));
    let meminfo = std::fs::read_to_string(at("/proc/meminfo")).unwrap_or_default();
    let ram_mb = meminfo
        .lines()
        .find_map(|l| l.strip_prefix("MemTotal:"))
        .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
        .map_or(0, |kb| kb / 1024);
    let cpuinfo = std::fs::read_to_string(at("/proc/cpuinfo")).unwrap_or_default();
    let cpu_threads = cpuinfo
        .lines()
        .filter(|l| l.starts_with("processor"))
        .count() as u64;
    let avx2 = cpuinfo
        .lines()
        .any(|l| l.split_whitespace().any(|w| w == "avx2"));
    let (mut gpu, mut discrete, mut vram_mb) = ("none", false, None);
    // The PCI display devices (class 0x03xx), so a GPU with no driver loaded
    // counts: NVIDIA first, then AMD with its own memory, then integrated.
    let mut devices: Vec<PathBuf> = std::fs::read_dir(at("/sys/bus/pci/devices"))
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    devices.sort();
    for dev in devices {
        let read = |f: &str| {
            std::fs::read_to_string(dev.join(f))
                .unwrap_or_default()
                .trim()
                .to_string()
        };
        if !read("class").starts_with("0x03") {
            continue;
        }
        match read("vendor").as_str() {
            "0x10de" => {
                (gpu, discrete, vram_mb) = ("nvidia", true, nvidia_smi_mb());
                break;
            }
            "0x1002" => {
                let mem = read("mem_info_vram_total").parse::<u64>().unwrap_or(0) / 1_048_576;
                if mem >= 2048 {
                    (gpu, discrete, vram_mb) = ("amd", true, Some(mem));
                } else if gpu == "none" {
                    gpu = "amd";
                }
            }
            "0x8086" if gpu == "none" => gpu = "intel",
            _ => {}
        }
    }
    Hardware {
        ram_mb,
        gpu,
        gpu_discrete: discrete,
        vram_mb,
        cpu_threads,
        avx2,
        disk_free_mb: 0,
    }
}

#[cfg(windows)]
fn platform() -> Hardware {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.refresh_cpu_all();
    let (mut gpu, mut discrete, mut vram_mb) = ("none", false, None);
    // Windows lists its adapters (Win32_VideoController); the vendor is in
    // the PnP id. NVIDIA memory comes from nvidia-smi (AdapterRAM stops at
    // 4 GB); AMD's AdapterRAM is a lower bound, good enough to size by.
    let ps = std::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_VideoController | ForEach-Object { \"$($_.PNPDeviceID)|$($_.AdapterRAM)\" }",
        ])
        .stderr(std::process::Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default();
    for line in ps.lines() {
        let (id, ram) = line.split_once('|').unwrap_or((line, ""));
        let id = id.to_uppercase();
        let mem = ram.trim().parse::<u64>().unwrap_or(0) / 1_048_576;
        if id.contains("VEN_10DE") {
            (gpu, discrete, vram_mb) = ("nvidia", true, nvidia_smi_mb());
            break;
        } else if id.contains("VEN_1002") {
            if mem >= 2048 {
                (gpu, discrete, vram_mb) = ("amd", true, Some(mem));
            } else if gpu == "none" {
                gpu = "amd";
            }
        } else if id.contains("VEN_8086") && gpu == "none" {
            gpu = "intel";
        }
    }
    Hardware {
        ram_mb: sys.total_memory() / 1_048_576,
        gpu,
        gpu_discrete: discrete,
        vram_mb,
        cpu_threads: sys.cpus().len() as u64,
        avx2: std::is_x86_feature_detected!("avx2"),
        disk_free_mb: 0,
    }
}

/// "4 GB" or "4.5 GB".
pub fn gb(mb: u64) -> String {
    let g = mb as f64 / 1024.0;
    if g == g.trunc() {
        format!("{} GB", g as u64)
    } else {
        format!("{g:.1} GB")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Preset {
    pub id: &'static str,
    pub label: String,
    pub budget_mb: u64,
    pub model: Option<&'static str>,
    pub download_mb: u64,
    pub vision: bool,
    pub fast: bool,
    pub available: bool,
    pub note: String,
}

impl Preset {
    /// `id|label|budget_mb|model|download_mb|vision|fast|available|note`.
    pub fn line(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.id,
            self.label,
            self.budget_mb,
            self.model.unwrap_or(""),
            self.download_mb,
            self.vision as u8,
            self.fast as u8,
            self.available as u8,
            self.note
        )
    }
}

/// How much local AI may use, and the model each budget gets.
pub fn presets(hw: &Hardware) -> Vec<Preset> {
    let mut budgets: Vec<(&'static str, String, u64, bool)> = Vec::new();
    if let Some(vram) = hw.vram_mb.filter(|v| hw.gpu_discrete && *v > 512) {
        let b = vram - 512;
        budgets.push((
            "gpu",
            format!("GPU (up to {} of video memory)", gb(b)),
            b,
            true,
        ));
    }
    for (id, pct, label) in [
        ("light", 25, "Light"),
        ("balanced", 50, "Balanced"),
        ("most", 75, "Most"),
    ] {
        let b = hw.ram_mb * pct / 100;
        budgets.push((id, format!("{label} (up to {} of memory)", gb(b)), b, false));
    }
    budgets
        .into_iter()
        .map(|(id, label, budget_mb, fast)| {
            let Some(m) = model_for_budget(budget_mb) else {
                return Preset {
                    id,
                    label,
                    budget_mb,
                    model: None,
                    download_mb: 0,
                    vision: false,
                    fast,
                    available: false,
                    note: "too little memory for local AI; use a cloud provider".into(),
                };
            };
            let sees = if m.vision {
                "sees screenshots"
            } else {
                "text only: can't see screenshots"
            };
            let mut note = format!("needs about {} while answering, {sees}", gb(m.need_mb));
            note += if local_first(m.id, Order::Auto, None) {
                ", answers first"
            } else {
                ", backs up cloud (cloud answers first while online)"
            };
            let mut available = true;
            if m.download_mb + 1024 > hw.disk_free_mb {
                available = false;
                note = format!(
                    "needs {} of disk, {} free",
                    gb(m.download_mb + 1024),
                    gb(hw.disk_free_mb)
                );
            }
            Preset {
                id,
                label,
                budget_mb,
                model: Some(m.id),
                download_mb: m.download_mb,
                vision: m.vision,
                fast,
                available,
                note,
            }
        })
        .collect()
}

// ------------------------------------------------------------ installed engines

/// A model an engine on this computer already has.
#[derive(Debug, Clone, PartialEq)]
pub struct FoundModel {
    pub name: String,
    pub size_mb: u64,
    pub vision: bool,
}

/// Ollama or llama.cpp as found on this computer: installed, running (its
/// server answers), where it answers, and the models it already has.
#[derive(Debug, Clone, PartialEq)]
pub struct Engine {
    pub name: &'static str,
    pub installed: bool,
    pub running: bool,
    pub base_url: &'static str,
    pub models: Vec<FoundModel>,
}

pub const OLLAMA_URL: &str = "http://127.0.0.1:11434";
pub const LLAMACPP_URL: &str = "http://127.0.0.1:8080";

/// Models that see images, by family, when the engine doesn't say.
fn vision_by_name(name: &str) -> bool {
    let n = name.to_lowercase();
    [
        "gemma3",
        "llava",
        "-vl",
        "vl:",
        "vision",
        "minicpm-v",
        "moondream",
        "pixtral",
        "mmproj",
    ]
    .iter()
    .any(|f| n.contains(f))
}

/// Ollama's `/api/tags`: its models and their sizes.
pub fn parse_ollama_tags(json: &str) -> Vec<FoundModel> {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    v["models"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| {
                    let name = m["name"].as_str().or_else(|| m["model"].as_str())?;
                    Some(FoundModel {
                        name: name.to_string(),
                        size_mb: m["size"].as_u64().unwrap_or(0) / 1_048_576,
                        vision: vision_by_name(name),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// An OpenAI-style `/v1/models` (llama.cpp's server): the model names.
pub fn parse_openai_models(json: &str) -> Vec<String> {
    let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
    v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["id"].as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// The GGUF models in a folder (FRIDAY's own llama.cpp models); one with an
/// `-mmproj.gguf` beside it sees images. The projector files aren't models.
pub fn gguf_models(dir: &Path) -> Vec<FoundModel> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<FoundModel> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if !name.ends_with(".gguf") || name.contains("mmproj") {
                return None;
            }
            let projector = dir.join(format!("{}-mmproj.gguf", name.trim_end_matches(".gguf")));
            Some(FoundModel {
                size_mb: e.metadata().map(|m| m.len()).unwrap_or(0) / 1_048_576,
                vision: projector.is_file(),
                name,
            })
        })
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name));
    found
}

fn on_path(names: &[&str]) -> bool {
    std::env::var_os("PATH").is_some_and(|p| {
        std::env::split_paths(&p).any(|d| names.iter().any(|n| d.join(n).is_file()))
    })
}

/// Where FRIDAY keeps its own llama.cpp (the release build and its models).
pub fn llama_dir() -> std::path::PathBuf {
    if let Some(d) = std::env::var_os("FRIDAY_LLAMA_DIR").filter(|d| !d.is_empty()) {
        return d.into();
    }
    #[cfg(windows)]
    {
        std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
            .join("friday")
            .join("llama.cpp")
    }
    #[cfg(not(windows))]
    {
        std::path::PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
            .join(".local/share/friday/llama.cpp")
    }
}

fn ollama_installed() -> bool {
    // Windows' per-user install, which a tray started at sign-in may not
    // have on PATH yet.
    let per_user = cfg!(windows)
        && std::path::PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default())
            .join("Programs/Ollama/ollama.exe")
            .is_file();
    per_user || on_path(&["ollama", "ollama.exe"])
}

/// Ollama and llama.cpp on this computer, each with the models it already
/// has. A server that doesn't answer within a second counts as not running.
pub async fn engines() -> Vec<Engine> {
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(2))
        .connect_timeout(std::time::Duration::from_millis(800))
        .build()
        .unwrap_or_default();
    let get = |url: String| {
        let http = http.clone();
        async move {
            let r = http.get(url).send().await.ok()?;
            r.status().is_success().then_some(())?;
            r.text().await.ok()
        }
    };

    // Ollama: its models, and whether each one sees images (`/api/show`).
    let tags = get(format!("{OLLAMA_URL}/api/tags")).await;
    let mut ollama_models = tags.as_deref().map(parse_ollama_tags).unwrap_or_default();
    // All at once: many models mustn't add up to a long wait.
    let shows = futures::future::join_all(ollama_models.iter().map(|m| {
        let http = http.clone();
        let name = m.name.clone();
        async move {
            let r = http
                .post(format!("{OLLAMA_URL}/api/show"))
                .json(&serde_json::json!({ "model": name }))
                .send()
                .await
                .ok()?;
            let v = r.json::<serde_json::Value>().await.ok()?;
            v["capabilities"]
                .as_array()
                .map(|caps| caps.iter().any(|c| c == "vision"))
        }
    }))
    .await;
    for (m, vision) in ollama_models.iter_mut().zip(shows) {
        if let Some(v) = vision {
            m.vision = v;
        }
    }
    let ollama = Engine {
        name: "ollama",
        installed: ollama_installed() || tags.is_some(),
        running: tags.is_some(),
        base_url: OLLAMA_URL,
        models: ollama_models,
    };

    // llama.cpp: the running server's model, then FRIDAY's own downloads.
    let dir = llama_dir();
    let served = get(format!("{LLAMACPP_URL}/v1/models"))
        .await
        .map(|j| parse_openai_models(&j));
    let mut models = gguf_models(&dir.join("models"));
    for name in served.iter().flatten() {
        if !models
            .iter()
            .any(|m| m.name == *name || name.ends_with(&m.name))
        {
            models.push(FoundModel {
                name: name.clone(),
                size_mb: 0,
                vision: vision_by_name(name),
            });
        }
    }
    let llamacpp = Engine {
        name: "llamacpp",
        installed: on_path(&["llama-server", "llama-server.exe"])
            || dir.join("llama-server").is_file()
            || dir.join("llama-server.exe").is_file()
            || served.is_some(),
        running: served.is_some(),
        base_url: LLAMACPP_URL,
        models,
    };
    vec![ollama, llamacpp]
}

/// How far a download is, from Ollama's `/api/pull` lines: each layer
/// reports its own total and how much of it is in, so the whole is their sum.
#[derive(Debug, Default)]
pub struct PullProgress {
    layers: std::collections::BTreeMap<String, (u64, u64)>,
}

/// What one `/api/pull` line means.
#[derive(Debug, PartialEq)]
pub enum PullEvent {
    /// (percent, megabytes in, megabytes in all)
    Progress(u8, u64, u64),
    Done,
    Failed(String),
    Other,
}

impl PullProgress {
    pub fn feed(&mut self, line: &str) -> PullEvent {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            return PullEvent::Other;
        };
        if let Some(e) = v["error"].as_str() {
            return PullEvent::Failed(e.to_string());
        }
        if v["status"] == "success" {
            return PullEvent::Done;
        }
        let (Some(digest), Some(total)) = (v["digest"].as_str(), v["total"].as_u64()) else {
            return PullEvent::Other;
        };
        let done = v["completed"].as_u64().unwrap_or(0).min(total);
        self.layers.insert(digest.to_string(), (done, total));
        let (done, total) = self
            .layers
            .values()
            .fold((0, 0), |(d, t), (ld, lt)| (d + ld, t + lt));
        if total == 0 {
            return PullEvent::Other;
        }
        PullEvent::Progress(
            (done * 100 / total) as u8,
            done / 1_048_576,
            total / 1_048_576,
        )
    }
}

/// Downloads `model` with Ollama's API, calling `on` with each change in
/// progress (a percent at most once).
pub async fn ollama_pull(model: &str, mut on: impl FnMut(&PullEvent)) -> Result<(), String> {
    use futures::StreamExt;
    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(5))
        .build()
        .map_err(|e| e.to_string())?;
    let r = http
        .post(format!("{OLLAMA_URL}/api/pull"))
        .json(&serde_json::json!({ "model": model, "stream": true }))
        .send()
        .await
        .map_err(|_| "Ollama isn't running".to_string())?;
    if !r.status().is_success() {
        return Err(format!("Ollama said {}", r.status()));
    }
    let mut stream = r.bytes_stream();
    let (mut buf, mut progress, mut last) = (Vec::new(), PullProgress::default(), None);
    while let Some(chunk) = stream.next().await {
        buf.extend_from_slice(&chunk.map_err(|e| e.to_string())?);
        while let Some(at) = buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = buf.drain(..=at).collect();
            match progress.feed(&String::from_utf8_lossy(&line)) {
                PullEvent::Failed(e) => return Err(e),
                PullEvent::Done => {
                    on(&PullEvent::Done);
                    return Ok(());
                }
                // Ollama names its layers one at a time, so the whole grows as
                // it goes: what is shown only moves forward.
                PullEvent::Progress(p, d, t) => {
                    if last.is_none_or(|l| p > l) {
                        last = Some(p);
                        on(&PullEvent::Progress(p, d, t));
                    }
                }
                PullEvent::Other => {}
            }
        }
    }
    Err("the download stopped before it finished".into())
}

// ------------------------------------------------------------ llama.cpp on Windows

/// llama.cpp's own release build (the same one Linux pins), Windows zips:
/// (kind, file, sha256 GitHub publishes).
pub const LLAMA_BUILD: &str = "b12556";
pub const LLAMA_WIN: [(&str, &str, &str); 2] = [
    (
        "cpu",
        "llama-b12556-bin-win-cpu-x64.zip",
        "98882a3ca45688ffa585c3adba49213ca90c5b2dbbf1bc2af41ac437a0987d17",
    ),
    (
        "vulkan",
        "llama-b12556-bin-win-vulkan-x64.zip",
        "73f3346fbbad14db8f5a5b35c4e9dbae7debf306309bfc77aca4a17096cb2f93",
    ),
];

/// The GGUF file for each ladder model, pinned like `FRIDAY_LOCAL_GGUF` in
/// the Linux script (a test keeps the two the same): id, repo, revision,
/// file, sha256, and the image projector's sha256 for the ones that see.
pub struct Gguf {
    pub id: &'static str,
    pub repo: &'static str,
    pub rev: &'static str,
    pub file: &'static str,
    pub sha: &'static str,
    pub mmproj_sha: Option<&'static str>,
}

pub const GGUF: [Gguf; 5] = [
    Gguf {
        id: "qwen3:1.7b",
        repo: "Qwen/Qwen3-1.7B-GGUF",
        rev: "90862c4b9d2787eaed51d12237eafdfe7c5f6077",
        file: "Qwen3-1.7B-Q8_0.gguf",
        sha: "061b54daade076b5d3362dac252678d17da8c68f07560be70818cace6590cb1a",
        mmproj_sha: None,
    },
    Gguf {
        id: "qwen3:4b",
        repo: "Qwen/Qwen3-4B-GGUF",
        rev: "bc640142c66e1fdd12af0bd68f40445458f3869b",
        file: "Qwen3-4B-Q4_K_M.gguf",
        sha: "7485fe6f11af29433bc51cab58009521f205840f5b4ae3a32fa7f92e8534fdf5",
        mmproj_sha: None,
    },
    Gguf {
        id: "gemma3:4b",
        repo: "ggml-org/gemma-3-4b-it-GGUF",
        rev: "d0976223747697cb51e056d85c532013931fe52e",
        file: "gemma-3-4b-it-Q4_K_M.gguf",
        sha: "882e8d2db44dc554fb0ea5077cb7e4bc49e7342a1f0da57901c0802ea21a0863",
        mmproj_sha: Some("8c0fb064b019a6972856aaae2c7e4792858af3ca4561be2dbf649123ba6c40cb"),
    },
    Gguf {
        id: "gemma3:12b",
        repo: "ggml-org/gemma-3-12b-it-GGUF",
        rev: "ec0cbabd8dbff316f659876a50202295c3c4a314",
        file: "gemma-3-12b-it-Q4_K_M.gguf",
        sha: "7bb69bff3f48a7b642355d64a90e481182a7794707b3133890646b1efa778ff5",
        mmproj_sha: Some("30c02d056410848227001830866e0a269fcc28aaf8ca971bded494003de9f5a5"),
    },
    Gguf {
        id: "gemma3:27b",
        repo: "ggml-org/gemma-3-27b-it-GGUF",
        rev: "f94c25afed0072339c5fa3b705a7b4222afe5f62",
        file: "gemma-3-27b-it-Q4_K_M.gguf",
        sha: "edc9aff4d811a285b9157618130b08688b0768d94ee5355b02dc0cb713012e15",
        mmproj_sha: Some("54cb61c842fe49ac3c89bc1a614a2778163eb49f3dec2b90ff688b4c0392cb48"),
    },
];

/// The arguments llama-server runs with for one model file (as Linux's
/// friday-llama.service): no thinking out loud, asleep after five idle
/// minutes, on the GPU when there is one.
pub fn llama_args(model: &Path, projector: Option<&Path>, gpu: bool) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "-m".into(),
        model.to_string_lossy().into_owned(),
        "-c".into(),
        "8192".into(),
        "--reasoning".into(),
        "off".into(),
        "--sleep-idle-seconds".into(),
        "300".into(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        "8080".into(),
    ];
    if let Some(p) = projector {
        a.push("--mmproj".into());
        a.push(p.to_string_lossy().into_owned());
    }
    if gpu {
        a.push("--n-gpu-layers".into());
        a.push("99".into());
    }
    a
}

/// Downloads `url` to `dest` (through a `.part` file), refused unless its
/// sha256 matches; `on` hears how far it is.
async fn fetch_checked(
    http: &reqwest::Client,
    url: &str,
    dest: &Path,
    sha: &str,
    on: &mut impl FnMut(&PullEvent),
) -> Result<(), String> {
    use futures::StreamExt;
    use sha2::{Digest, Sha256};
    use std::io::Write;
    if let Some(dir) = dest.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let name = dest
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let r = http
        .get(url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("couldn't download {name}: {e}"))?;
    let total = r.content_length().unwrap_or(0);
    let part = dest.with_extension("part");
    let mut file = std::fs::File::create(&part).map_err(|e| e.to_string())?;
    let (mut hash, mut done, mut last) = (Sha256::new(), 0u64, None);
    let mut stream = r.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("the download of {name} stopped: {e}"))?;
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        hash.update(&chunk);
        done += chunk.len() as u64;
        if let Some(p) = (done * 100).checked_div(total).map(|p| p as u8) {
            if last != Some(p) {
                last = Some(p);
                on(&PullEvent::Progress(p, done / 1_048_576, total / 1_048_576));
            }
        }
    }
    drop(file);
    let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if got != sha {
        let _ = std::fs::remove_file(&part);
        return Err(format!("{name} doesn't match its checksum; not using it"));
    }
    std::fs::rename(&part, dest).map_err(|e| e.to_string())
}

/// llama.cpp on Windows: its release build, then the model (`model`: a ladder
/// id, or a `.gguf` already in FRIDAY's models folder), and the arguments
/// the tray runs llama-server with (`llama.args`, one per line). What is
/// already there isn't fetched again. Returns whether the model sees images.
pub async fn install_llamacpp(
    model: &str,
    gpu: bool,
    mut on: impl FnMut(&PullEvent),
) -> Result<bool, String> {
    let dir = llama_dir();
    let models = dir.join("models");
    let http = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    if !dir.join("llama-server.exe").is_file() {
        let (_, file, sha) = LLAMA_WIN[usize::from(gpu)];
        let zip = dir.join("downloads").join(file);
        let url =
            format!("https://github.com/ggml-org/llama.cpp/releases/download/{LLAMA_BUILD}/{file}");
        fetch_checked(&http, &url, &zip, sha, &mut on).await?;
        // Windows' own tar unpacks zip files too.
        let ok = std::process::Command::new("tar")
            .arg("-xf")
            .arg(&zip)
            .arg("-C")
            .arg(&dir)
            .status()
            .map_err(|e| format!("couldn't run tar: {e}"))?
            .success();
        let _ = std::fs::remove_file(&zip);
        if !ok {
            return Err("couldn't unpack llama.cpp".into());
        }
    }
    let (file, projector) = if model.ends_with(".gguf") {
        let file = models.join(model);
        if !file.is_file() {
            return Err(format!("{model} isn't in {}", models.display()));
        }
        let p = models.join(format!("{}-mmproj.gguf", model.trim_end_matches(".gguf")));
        let has = p.is_file();
        (file, has.then_some(p))
    } else {
        let row = GGUF
            .iter()
            .find(|g| g.id == model)
            .ok_or_else(|| format!("no llama.cpp download for {model}"))?;
        let hf = format!("https://huggingface.co/{}/resolve/{}", row.repo, row.rev);
        let file = models.join(row.file);
        if !file.is_file() {
            fetch_checked(
                &http,
                &format!("{hf}/{}", row.file),
                &file,
                row.sha,
                &mut on,
            )
            .await?;
        }
        let projector = match row.mmproj_sha {
            Some(sha) => {
                let p = models.join(format!(
                    "{}-mmproj.gguf",
                    row.file.trim_end_matches(".gguf")
                ));
                if !p.is_file() {
                    fetch_checked(
                        &http,
                        &format!("{hf}/mmproj-model-f16.gguf"),
                        &p,
                        sha,
                        &mut on,
                    )
                    .await?;
                }
                Some(p)
            }
            None => None,
        };
        (file, projector)
    };
    let args = llama_args(&file, projector.as_deref(), gpu);
    std::fs::write(dir.join("llama.args"), args.join("\n") + "\n").map_err(|e| e.to_string())?;
    on(&PullEvent::Done);
    Ok(projector.is_some())
}

/// One line per engine, for `friday local-ai status` and the settings pages.
pub fn engine_line(e: &Engine) -> String {
    let label = if e.name == "ollama" {
        "Ollama"
    } else {
        "llama.cpp"
    };
    if !e.installed {
        return format!("{label}: not installed");
    }
    let state = if e.running {
        "running"
    } else {
        "installed, not running"
    };
    if e.models.is_empty() {
        return format!("{label}: {state}, no models yet");
    }
    let list: Vec<String> = e
        .models
        .iter()
        .map(|m| {
            let mut s = m.name.clone();
            let mut notes = Vec::new();
            if m.size_mb > 0 {
                notes.push(gb(m.size_mb));
            }
            if m.vision {
                notes.push("sees screenshots".to_string());
            }
            if !notes.is_empty() {
                s += &format!(" ({})", notes.join(", "));
            }
            s
        })
        .collect();
    format!("{label}: {state}, {}", list.join("; "))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fake /sys trees have `0000:01:00.0` folders: Linux only.
    #[cfg(not(windows))]
    fn fake_machine(ram_gb: u64, avx2: bool, gpu: Option<(&str, Option<u64>)>) -> PathBuf {
        let r = std::env::temp_dir().join(format!("friday-hw-{}", uuid::Uuid::new_v4()));
        let w = |rel: &str, text: String| {
            let p = r.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        w(
            "proc/meminfo",
            format!("MemTotal:       {} kB\n", ram_gb * 1024 * 1024),
        );
        let flags = if avx2 { "fpu sse avx avx2" } else { "fpu sse" };
        w(
            "proc/cpuinfo",
            (0..8)
                .map(|i| format!("processor\t: {i}\nflags\t\t: {flags}\n\n"))
                .collect(),
        );
        w("sys/bus/pci/devices/0000:00:1f.0/class", "0x060100".into());
        w("sys/bus/pci/devices/0000:00:1f.0/vendor", "0x8086".into());
        if let Some((vendor, vram)) = gpu {
            w("sys/bus/pci/devices/0000:01:00.0/class", "0x030000".into());
            w("sys/bus/pci/devices/0000:01:00.0/vendor", vendor.into());
            if let Some(v) = vram {
                w(
                    "sys/bus/pci/devices/0000:01:00.0/mem_info_vram_total",
                    (v * 1024 * 1024).to_string(),
                );
            }
        }
        r
    }

    #[cfg(not(windows))]
    fn hw(root: &Path, disk: u64) -> Hardware {
        Hardware {
            disk_free_mb: disk,
            ..scan_linux(root)
        }
    }

    #[cfg(not(windows))]
    #[test]
    fn the_machine_is_read_from_proc_and_sys() {
        let h = hw(
            &fake_machine(16, true, Some(("0x1002", Some(8192)))),
            50_000,
        );
        assert_eq!(
            (h.ram_mb, h.gpu, h.gpu_discrete, h.vram_mb),
            (16384, "amd", true, Some(8192))
        );
        assert_eq!((h.cpu_threads, h.avx2, h.disk_free_mb), (8, true, 50_000));
        assert!(h.lines().contains("vram_mb=8192\n"));
        // A non-display PCI device doesn't count.
        let none = hw(&fake_machine(8, true, None), 0);
        assert_eq!((none.gpu, none.gpu_discrete), ("none", false));
        // NVIDIA with no driver to ask: video memory unknown.
        let nv = scan_linux(&fake_machine(16, true, Some(("0x10de", None))));
        assert_eq!((nv.gpu, nv.gpu_discrete), ("nvidia", true));
        assert_eq!(
            hw(&fake_machine(8, true, Some(("0x8086", None))), 0).gpu,
            "intel"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn each_budget_gets_the_largest_model_that_fits() {
        let rows = presets(&hw(
            &fake_machine(16, true, Some(("0x1002", Some(8192)))),
            100_000,
        ));
        let by = |id: &str| rows.iter().find(|p| p.id == id).unwrap();
        assert_eq!(by("gpu").model, Some("gemma3:4b"));
        assert!(by("gpu").fast);
        assert_eq!(by("most").model, Some("gemma3:12b"));
        assert!(by("most").vision);
        assert_eq!(
            (by("light").model, by("light").vision),
            (Some("qwen3:4b"), false)
        );
        assert!(
            by("most").note.contains("answers first"),
            "{}",
            by("most").note
        );
        assert!(
            by("light").note.contains("backs up cloud"),
            "{}",
            by("light").note
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn too_little_memory_or_disk_is_said_in_words() {
        let small = presets(&hw(&fake_machine(2, true, None), 100_000));
        let balanced = small.iter().find(|p| p.id == "balanced").unwrap();
        assert!(balanced.model.is_none() && !balanced.available);
        assert!(balanced.note.contains("too little memory"));
        let tight = presets(&hw(
            &fake_machine(16, true, Some(("0x1002", Some(8192)))),
            1500,
        ));
        let most = tight.iter().find(|p| p.id == "most").unwrap();
        assert!(
            !most.available && most.note.contains("disk"),
            "{}",
            most.note
        );
    }

    #[test]
    fn auto_puts_big_fast_models_first_and_the_order_can_be_forced() {
        assert!(local_first("gemma3:12b", Order::Auto, None));
        assert!(!local_first("gemma3:4b", Order::Auto, None));
        assert!(!local_first("gemma3:12b", Order::Auto, Some(3.0)));
        assert!(local_first("gemma3:12b", Order::Auto, Some(8.0)));
        assert!(local_first("qwen3:1.7b", Order::Local, None));
        assert!(!local_first("gemma3:27b", Order::Cloud, None));
        // The size is the number the id's tag starts with.
        assert!(local_first("some:12b-it", Order::Auto, None));
        assert!(!local_first("some:4B", Order::Auto, None));
        assert!(!local_first("whatever", Order::Auto, None));
        assert_eq!(model_for_budget(1000), None);
        assert_eq!(model_for_budget(20_000).unwrap().id, "gemma3:27b");
    }

    #[test]
    fn sizes_are_written_like_the_bash_script_does() {
        assert_eq!(gb(2048), "2 GB");
        assert_eq!(gb(1536), "1.5 GB");
        assert_eq!(gb(4608), "4.5 GB");
    }

    #[test]
    fn ollamas_models_come_with_their_sizes() {
        let tags = r#"{"models":[{"name":"qwen3:4b","size":2620788019},{"name":"gemma3:4b","size":3338801804}]}"#;
        let found = parse_ollama_tags(tags);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "qwen3:4b");
        assert_eq!(found[0].size_mb, 2499);
        assert!(!found[0].vision);
        assert!(found[1].vision, "gemma3 sees images");
        assert!(parse_ollama_tags("not json").is_empty());
    }

    #[test]
    fn a_llama_server_names_its_model() {
        let v = r#"{"object":"list","data":[{"id":"Qwen3-4B-Q4_K_M.gguf","object":"model"}]}"#;
        assert_eq!(parse_openai_models(v), vec!["Qwen3-4B-Q4_K_M.gguf"]);
    }

    #[test]
    fn fridays_own_gguf_files_are_found_and_projectors_skipped() {
        let dir = std::env::temp_dir().join(format!("friday-gguf-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("gemma-3-4b-it-Q4_K_M.gguf"), vec![0u8; 2_097_152]).unwrap();
        std::fs::write(dir.join("gemma-3-4b-it-Q4_K_M-mmproj.gguf"), b"x").unwrap();
        std::fs::write(dir.join("Qwen3-4B-Q4_K_M.gguf"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        let found = gguf_models(&dir);
        assert_eq!(found.len(), 2, "{found:?}");
        assert_eq!(found[1].name, "gemma-3-4b-it-Q4_K_M.gguf");
        assert!(found[1].vision);
        assert_eq!(found[1].size_mb, 2);
        assert!(!found[0].vision);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_engine_line_says_what_is_there() {
        let mut e = Engine {
            name: "ollama",
            installed: false,
            running: false,
            base_url: OLLAMA_URL,
            models: vec![],
        };
        assert_eq!(engine_line(&e), "Ollama: not installed");
        e.installed = true;
        assert_eq!(
            engine_line(&e),
            "Ollama: installed, not running, no models yet"
        );
        e.running = true;
        e.models = vec![FoundModel {
            name: "gemma3:4b".into(),
            size_mb: 3184,
            vision: true,
        }];
        let line = engine_line(&e);
        assert!(line.starts_with("Ollama: running, gemma3:4b ("), "{line}");
        assert!(line.contains("sees screenshots"), "{line}");
    }

    #[test]
    fn pull_progress_adds_up_the_layers() {
        let mut p = PullProgress::default();
        assert_eq!(p.feed(r#"{"status":"pulling manifest"}"#), PullEvent::Other);
        assert_eq!(
            p.feed(r#"{"status":"pulling a","digest":"sha256:a","total":3145728000,"completed":1048576000}"#),
            PullEvent::Progress(33, 1000, 3000)
        );
        assert_eq!(
            p.feed(r#"{"status":"pulling b","digest":"sha256:b","total":1048576000,"completed":1048576000}"#),
            PullEvent::Progress(50, 2000, 4000)
        );
        assert_eq!(p.feed(r#"{"status":"success"}"#), PullEvent::Done);
        assert_eq!(
            p.feed(r#"{"error":"pull model manifest: file does not exist"}"#),
            PullEvent::Failed("pull model manifest: file does not exist".into())
        );
    }

    #[test]
    fn the_gguf_pins_match_the_linux_script() {
        let script = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/linux/friday"),
        )
        .unwrap();
        for g in &GGUF {
            let row = format!(
                "{}|{}|{}|{}|{}|{}",
                g.id,
                g.repo,
                g.rev,
                g.file,
                g.sha,
                g.mmproj_sha.unwrap_or("")
            );
            assert!(script.contains(&row), "{row} not in FRIDAY_LOCAL_GGUF");
        }
        assert!(script.contains(&format!("FRIDAY_LLAMA_BUILD={LLAMA_BUILD}")));
    }

    #[test]
    fn llama_server_runs_like_on_linux() {
        let a = llama_args(Path::new("m.gguf"), Some(Path::new("m-mmproj.gguf")), true);
        let s = a.join(" ");
        assert!(
            s.starts_with("-m m.gguf -c 8192 --reasoning off --sleep-idle-seconds 300"),
            "{s}"
        );
        assert!(
            s.contains("--port 8080") && s.contains("--host 127.0.0.1"),
            "{s}"
        );
        assert!(
            s.contains("--mmproj m-mmproj.gguf") && s.ends_with("--n-gpu-layers 99"),
            "{s}"
        );
        assert!(!llama_args(Path::new("m.gguf"), None, false)
            .join(" ")
            .contains("mmproj"));
    }
}
