//! Proof that nothing left (`friday doctor --offline`, Settings): what the
//! egress log says went to a provider off this computer lately, and what
//! FRIDAY's daemon is connected to right now. Read here; nothing is sent.

use std::collections::{BTreeMap, HashSet};

/// Asks that went off this computer since `since` (provider → count), and
/// how many went to a local model.
pub fn tally(
    egress: &str,
    since: chrono::DateTime<chrono::Utc>,
) -> (BTreeMap<String, usize>, usize) {
    let mut off = BTreeMap::new();
    let mut local = 0;
    for line in egress.lines() {
        let Ok(e) = serde_json::from_str::<friday_intelligence::egress::EgressEntry>(line) else {
            continue;
        };
        let Ok(ts) = chrono::DateTime::parse_from_rfc3339(&e.ts) else {
            continue;
        };
        if ts < since {
            continue;
        }
        if e.local == Some(true) {
            local += 1;
        } else {
            *off.entry(e.provider).or_insert(0) += 1;
        }
    }
    (off, local)
}

/// What the report knows besides the log.
pub struct Seen<'a> {
    /// The egress log, or `None` when there is none.
    pub egress: Option<&'a str>,
    pub private: bool,
    /// FRIDAY's connections off this computer now; `None` when it isn't running.
    pub connections: Option<&'a [String]>,
    /// Coding-agent runs in the window: they talk to their own model.
    pub agent_runs: usize,
    /// What the connection check covers, in words.
    pub covers: &'a str,
}

/// The plain report.
pub fn report(since: chrono::DateTime<chrono::Utc>, hours: i64, seen: &Seen) -> String {
    let mut out = vec![if seen.private {
        "Private mode is on.".to_string()
    } else {
        "Private mode is off: your provider answers asks (friday private on keeps them here)."
            .to_string()
    }];
    match seen.egress {
        None => out.push(
            "There's no egress log here, so what FRIDAY sent can't be told from it.".to_string(),
        ),
        Some(egress) => {
            let (off, local) = tally(egress, since);
            let sent: usize = off.values().sum();
            out.push(if sent == 0 {
                format!(
                    "In the last {hours} hours FRIDAY sent no ask to a provider off this computer ({local} to the local model)."
                )
            } else {
                let who: Vec<String> = off.iter().map(|(p, n)| format!("{p} {n}")).collect();
                format!(
                    "In the last {hours} hours {sent} asks went off this computer ({}), {local} to the local model.",
                    who.join(", ")
                )
            });
        }
    }
    if seen.agent_runs > 0 {
        out.push(format!(
            "{} coding-agent runs in that time talked to their own model; those aren't in FRIDAY's log.",
            seen.agent_runs
        ));
    }
    out.push(match seen.connections {
        None => "FRIDAY isn't running, so it has no connections now.".to_string(),
        Some([]) => format!(
            "Right now {} has no connection to anything off this computer.",
            seen.covers
        ),
        Some(c) => format!(
            "Right now {} is connected off this computer to: {}.",
            seen.covers,
            c.join(", ")
        ),
    });
    out.join("\n")
}

/// An address as /proc/net writes it: 32-bit words in this machine's byte order.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn proc_ip(hex: &str) -> Option<std::net::IpAddr> {
    let word = |i: usize| u32::from_str_radix(hex.get(i * 8..i * 8 + 8)?, 16).ok();
    match hex.len() {
        8 => Some(std::net::Ipv4Addr::from(word(0)?.to_ne_bytes()).into()),
        32 => {
            let mut b = [0u8; 16];
            for i in 0..4 {
                b[i * 4..i * 4 + 4].copy_from_slice(&word(i)?.to_ne_bytes());
            }
            Some(std::net::Ipv6Addr::from(b).into())
        }
        _ => None,
    }
}

/// This computer itself (loopback, also as an IPv4-mapped address).
fn this_computer(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_loopback() || v4.is_unspecified(),
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
    }
}

/// Connected sockets (TCP established, connected UDP such as QUIC) in a
/// /proc/net table, for these sockets, to anything off this computer.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn remote_in(table: &str, inodes: &HashSet<u64>) -> Vec<String> {
    table
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let (rem, st, inode) = (f.get(2)?, f.get(3)?, f.get(9)?.parse::<u64>().ok()?);
            let (addr, port) = rem.split_once(':')?;
            let ip = proc_ip(addr)?;
            let port = u16::from_str_radix(port, 16).ok()?;
            (*st == "01" && inodes.contains(&inode) && !this_computer(ip))
                .then(|| std::net::SocketAddr::new(ip, port).to_string())
        })
        .collect()
}

/// The daemon and everything it started (coding agents too): their
/// connections off this computer now; `None` when it isn't running.
#[cfg(target_os = "linux")]
pub fn daemon_connections() -> Option<Vec<String>> {
    // (pid, parent pid, name) of every process.
    let procs: Vec<(u32, u32, String)> = std::fs::read_dir("/proc")
        .ok()?
        .flatten()
        .filter_map(|e| {
            let pid: u32 = e.file_name().to_str()?.parse().ok()?;
            let stat = std::fs::read_to_string(e.path().join("stat")).ok()?;
            let (name, rest) = stat.split_once(" (")?.1.rsplit_once(") ")?;
            let ppid = rest.split_whitespace().nth(1)?.parse().ok()?;
            Some((pid, ppid, name.to_string()))
        })
        .collect();
    let mut ours: HashSet<u32> = procs
        .iter()
        .filter(|p| p.2.starts_with("friday-linux-da"))
        .map(|p| p.0)
        .collect();
    if ours.is_empty() {
        return None;
    }
    loop {
        let more: Vec<u32> = procs
            .iter()
            .filter(|p| ours.contains(&p.1) && !ours.contains(&p.0))
            .map(|p| p.0)
            .collect();
        if more.is_empty() {
            break;
        }
        ours.extend(more);
    }
    let mut inodes = HashSet::new();
    for pid in &ours {
        for fd in std::fs::read_dir(format!("/proc/{pid}/fd"))
            .into_iter()
            .flatten()
            .flatten()
        {
            if let Ok(t) = std::fs::read_link(fd.path()) {
                let t = t.to_string_lossy();
                if let Some(n) = t.strip_prefix("socket:[").and_then(|r| r.strip_suffix(']')) {
                    if let Ok(n) = n.parse() {
                        inodes.insert(n);
                    }
                }
            }
        }
    }
    let read = |p: &str| std::fs::read_to_string(std::path::Path::new(p)).unwrap_or_default();
    let mut out = Vec::new();
    for t in ["tcp", "tcp6", "udp", "udp6"] {
        out.extend(remote_in(&read(&format!("/proc/net/{t}")), &inodes));
    }
    out.sort();
    out.dedup();
    Some(out)
}

/// What [`daemon_connections`] covers, in words.
pub const COVERS: &str = if cfg!(windows) {
    "FRIDAY (its daemon and tray; coding agents it started aren't checked on Windows)"
} else {
    "FRIDAY (its daemon and everything it started, coding agents too)"
};

/// The daemon's and the tray's TCP connections off this computer now
/// (`netstat -ano`); `None` when FRIDAY isn't running.
#[cfg(windows)]
pub fn daemon_connections() -> Option<Vec<String>> {
    use std::os::windows::process::CommandExt;
    let run = |prog: &str, args: &[&str]| {
        std::process::Command::new(prog)
            .args(args)
            .creation_flags(0x0800_0000)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    };
    let mut pids: HashSet<String> = HashSet::new();
    for image in ["friday-daemon.exe", "friday-tray.exe"] {
        let filter = format!("IMAGENAME eq {image}");
        let list = run("tasklist", &["/FI", &filter, "/FO", "CSV", "/NH"])?;
        pids.extend(
            list.lines()
                .filter_map(|l| l.split("\",\"").nth(1).map(str::to_string)),
        );
    }
    if pids.is_empty() {
        return None;
    }
    let net = run("netstat", &["-ano", "-p", "TCP"])?;
    Some(windows_remote(&net, &pids))
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn daemon_connections() -> Option<Vec<String>> {
    None
}

/// Established connections of `pids` in `netstat -ano` output, off this computer.
#[cfg_attr(not(windows), allow(dead_code))]
fn windows_remote(netstat: &str, pids: &HashSet<String>) -> Vec<String> {
    let mut out: Vec<String> = netstat
        .lines()
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let (remote, state, pid) = (f.get(2)?, f.get(3)?, f.get(4)?);
            let here = remote
                .parse::<std::net::SocketAddr>()
                .is_ok_and(|a| this_computer(a.ip()));
            (*state == "ESTABLISHED" && pids.contains(*pid) && !here).then(|| remote.to_string())
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_report_counts_what_left_and_what_stayed() {
        let now = chrono::Utc::now();
        let line = |provider: &str, local: bool, ago: i64| {
            serde_json::json!({
                "ts": (now - chrono::Duration::hours(ago)).to_rfc3339(),
                "provider": provider, "model": "m", "kinds": ["text"],
                "text_chars": 10, "outcome": "ok",
                "local": if local { Some(true) } else { None },
            })
            .to_string()
        };
        let log = [
            line("llamacpp", true, 1),
            line("llamacpp", true, 2),
            line("opencode", false, 30),
        ]
        .join("\n");
        let since = now - chrono::Duration::hours(24);
        let seen = |egress, conns, runs| Seen {
            egress,
            private: true,
            connections: conns,
            agent_runs: runs,
            covers: "FRIDAY",
        };
        let r = report(since, 24, &seen(Some(log.as_str()), Some(&[]), 0));
        assert!(
            r.contains("sent no ask to a provider off this computer (2 to the local model)"),
            "{r}"
        );
        assert!(r.contains("FRIDAY has no connection to anything off this computer"));
        let log2 = format!("{log}\n{}", line("duckduckgo", false, 3));
        let c = ["192.0.2.7:443".to_string()];
        let r = report(since, 24, &seen(Some(log2.as_str()), Some(&c), 2));
        assert!(
            r.contains("1 asks went off this computer (duckduckgo 1)")
                && r.contains("192.0.2.7:443"),
            "{r}"
        );
        assert!(r.contains("2 coding-agent runs"));
        // No log: no claim that nothing left.
        let r = report(since, 24, &seen(None, None, 0));
        assert!(
            r.contains("no egress log")
                && r.contains("isn't running")
                && !r.contains("sent no ask"),
            "{r}"
        );
    }

    #[test]
    fn only_connections_off_this_computer_count() {
        let v4 = |a: [u8; 4]| format!("{:08X}", u32::from_ne_bytes(a));
        let v6 = |b: [u8; 16]| {
            (0..4)
                .map(|i| {
                    format!(
                        "{:08X}",
                        u32::from_ne_bytes(b[i * 4..i * 4 + 4].try_into().unwrap())
                    )
                })
                .collect::<String>()
        };
        let mapped = |a: [u8; 4]| {
            let mut b = [0u8; 16];
            b[10] = 0xff;
            b[11] = 0xff;
            b[12..].copy_from_slice(&a);
            v6(b)
        };
        let row = |rem: String, st: &str, inode: u32| {
            format!("   0: 00000000:0000 {rem}:01BB {st} 00000000:00000000 00:00000000 00000000  1000        0 {inode} 1")
        };
        let table = [
            "header".to_string(),
            row(v4([127, 0, 0, 1]), "01", 11),
            row(v4([192, 0, 2, 7]), "01", 12),
            row(v4([192, 0, 2, 8]), "06", 13),
            row(v4([192, 0, 2, 9]), "01", 99),
            row(mapped([127, 0, 0, 1]), "01", 14),
            row(mapped([192, 0, 2, 127]), "01", 15),
            row(
                v6(std::net::Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1).octets()),
                "01",
                16,
            ),
        ]
        .join("\n");
        let mine: HashSet<u64> = [11, 12, 13, 14, 15, 16].into();
        assert_eq!(
            remote_in(&table, &mine),
            vec![
                "192.0.2.7:443".to_string(),
                "[::ffff:192.0.2.127]:443".to_string(),
                "[2001:db8::1]:443".to_string()
            ]
        );
        let netstat = "  TCP    10.0.2.15:49700   192.0.2.7:443    ESTABLISHED     4242\n  TCP    127.0.0.1:49701   127.0.0.1:18432  ESTABLISHED     4242\n  TCP    10.0.2.15:49702   192.0.2.8:443    ESTABLISHED     9999\n";
        assert_eq!(
            windows_remote(netstat, &["4242".to_string()].into()),
            vec!["192.0.2.7:443".to_string()]
        );
    }
}
