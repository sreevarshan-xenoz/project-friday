//! The Rust local-AI sizing (`friday_mobile::local_ai`) and the bash script's
//! (`friday local-ai detect|presets`) must give the same answers on the same
//! machines, until the bash copy goes. Linux only: the script is bash.
#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};
use std::process::Command;

use friday_mobile::local_ai;

/// A GPU's PCI vendor and, if the kernel tells, its video memory in MB.
type Gpu<'a> = Vec<(&'a str, Option<u64>)>;
type Case<'a> = (u64, bool, Gpu<'a>, u64);

fn machine(ram_gb: u64, avx2: bool, gpu: Gpu) -> PathBuf {
    let r = std::env::temp_dir().join(format!("friday-parity-{}", uuid::Uuid::new_v4()));
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
    for (i, (vendor, vram)) in gpu.iter().enumerate() {
        let dev = format!("sys/bus/pci/devices/0000:0{}:00.0", i + 1);
        w(&format!("{dev}/class"), "0x030000".into());
        w(&format!("{dev}/vendor"), (*vendor).into());
        if let Some(v) = vram {
            w(
                &format!("{dev}/mem_info_vram_total"),
                (v * 1024 * 1024).to_string(),
            );
        }
    }
    r
}

fn bash(root: &Path, disk: u64, sub: &str) -> String {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/linux/friday");
    let out = Command::new("bash")
        .arg(script)
        .args(["local-ai", sub])
        .env("FRIDAY_HW_ROOT", root)
        .env("FRIDAY_DISK_FREE_MB", disk.to_string())
        // No nvidia-smi: its answer would depend on this computer.
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", std::env::temp_dir())
        .output()
        .expect("bash");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn rust_and_bash_agree_on_the_same_machines() {
    // (ram GB, avx2, GPU vendor and video memory in GB, free disk MB)
    let amd = |mb| ("0x1002", Some(mb));
    let cases: Vec<Case> = vec![
        (16, true, vec![amd(8192)], 100_000),
        (16, true, vec![amd(8192)], 1_500),
        (16, true, vec![amd(1024)], 100_000),
        // An AMD card whose memory the kernel doesn't report.
        (16, true, vec![("0x1002", None)], 100_000),
        (16, true, vec![("0x10de", None)], 100_000),
        (8, false, vec![], 100_000),
        (2, true, vec![], 100_000),
        (4, true, vec![("0x8086", None)], 6_000),
        (32, true, vec![amd(24576)], 100_000),
        (64, true, vec![amd(49152)], 30_000),
        // Several GPUs: who wins does not depend on the order.
        (16, true, vec![("0x8086", None), amd(8192)], 100_000),
        (16, true, vec![amd(8192), ("0x8086", None)], 100_000),
        (16, true, vec![amd(1024), amd(8192)], 100_000),
        (16, true, vec![amd(8192), ("0x10de", None)], 100_000),
        (16, true, vec![("0x10de", None), amd(8192)], 100_000),
        // The disk margin (the download plus 1 GB) to the megabyte.
        (8, true, vec![], 2_858),
        (8, true, vec![], 2_857),
        (8, true, vec![], 2_000),
    ];
    for (ram, avx2, gpu, disk) in &cases {
        let root = machine(*ram, *avx2, gpu.clone());
        let ctx = format!("{ram} GB, avx2={avx2}, gpu={gpu:?}, disk={disk}");
        let hw = local_ai::Hardware {
            disk_free_mb: *disk,
            ..local_ai::scan_linux(&root)
        };
        assert_eq!(hw.lines(), bash(&root, *disk, "detect"), "detect: {ctx}");
        let rust: String = local_ai::presets(&hw)
            .iter()
            .map(|p| format!("{}\n", p.line()))
            .collect();
        assert_eq!(rust, bash(&root, *disk, "presets"), "presets: {ctx}");
    }
}
