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

/// The plain report.
pub fn report(
    egress: &str,
    since: chrono::DateTime<chrono::Utc>,
    hours: i64,
    private: bool,
    connections: Option<&[String]>,
) -> String {
    let (off, local) = tally(egress, since);
    let mut out = vec![if private {
        "Private mode is on.".to_string()
    } else {
        "Private mode is off: your provider answers asks (friday private on keeps them here)."
            .to_string()
    }];
    let sent: usize = off.values().sum();
    out.push(if sent == 0 {
        format!(
            "In the last {hours} hours nothing went to a provider off this computer ({local} to the local model)."
        )
    } else {
        let who: Vec<String> = off.iter().map(|(p, n)| format!("{p} {n}")).collect();
        format!(
            "In the last {hours} hours {sent} went off this computer ({}), {local} to the local model.",
            who.join(", ")
        )
    });
    out.push(match connections {
        None => "FRIDAY isn't running, so it has no connections now.".to_string(),
        Some([]) => "Right now FRIDAY has no connection to anything off this computer.".to_string(),
        Some(c) => format!(
            "Right now FRIDAY is connected off this computer to: {}.",
            c.join(", ")
        ),
    });
    out.join("\n")
}

/// Whether an address (as /proc/net writes it) is this computer.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn loopback(addr: &str, v6: bool) -> bool {
    if v6 {
        // ::1, or ::ffff:127.x mapped.
        addr == "00000000000000000000000001000000" || addr.starts_with("0000000000000000FFFF00007F")
    } else {
        addr.ends_with("7F") // 127.x.x.x, little-endian
    }
}

/// `ip:port` from /proc/net's hex form.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn shown(addr: &str, port: &str, v6: bool) -> String {
    let port = u16::from_str_radix(port, 16).unwrap_or(0);
    if v6 {
        return format!("[{addr}]:{port}");
    }
    let b = u32::from_str_radix(addr, 16).unwrap_or(0).to_le_bytes();
    format!("{}.{}.{}.{}:{port}", b[0], b[1], b[2], b[3])
}

/// Established connections in a /proc/net/tcp(6) table, for these sockets,
/// to anything off this computer.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn remote_in(table: &str, inodes: &HashSet<u64>, v6: bool) -> Vec<String> {
    table
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let (rem, st, inode) = (f.get(2)?, f.get(3)?, f.get(9)?.parse::<u64>().ok()?);
            let (addr, port) = rem.split_once(':')?;
            (*st == "01" && inodes.contains(&inode) && !loopback(addr, v6))
                .then(|| shown(addr, port, v6))
        })
        .collect()
}

/// The daemon's connections off this computer now; `None` when it isn't running.
#[cfg(target_os = "linux")]
pub fn daemon_connections() -> Option<Vec<String>> {
    let pids: Vec<String> = std::fs::read_dir("/proc")
        .ok()?
        .flatten()
        .filter_map(|e| {
            let pid = e.file_name().to_string_lossy().to_string();
            let comm = std::fs::read_to_string(e.path().join("comm")).ok()?;
            comm.trim().starts_with("friday-linux-da").then_some(pid)
        })
        .collect();
    if pids.is_empty() {
        return None;
    }
    let mut inodes = HashSet::new();
    for pid in &pids {
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
    let mut out = remote_in(&read("/proc/net/tcp"), &inodes, false);
    out.extend(remote_in(&read("/proc/net/tcp6"), &inodes, true));
    out.sort();
    out.dedup();
    Some(out)
}

/// The daemon's connections off this computer now (`netstat -ano`); `None`
/// when it isn't running.
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
    let list = run(
        "tasklist",
        &["/FI", "IMAGENAME eq friday-daemon.exe", "/FO", "CSV", "/NH"],
    )?;
    let pids: HashSet<String> = list
        .lines()
        .filter_map(|l| l.split("\",\"").nth(1).map(str::to_string))
        .collect();
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
            let local = remote.starts_with("127.") || remote.starts_with("[::1]");
            (*state == "ESTABLISHED" && pids.contains(*pid) && !local).then(|| remote.to_string())
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
        let r = report(&log, since, 24, true, Some(&[]));
        assert!(
            r.contains("nothing went to a provider off this computer (2 to the local model)"),
            "{r}"
        );
        assert!(r.contains("no connection to anything off this computer"));
        let log = format!("{log}\n{}", line("duckduckgo", false, 3));
        let r = report(&log, since, 24, false, Some(&["192.0.2.7:443".into()]));
        assert!(
            r.contains("1 went off this computer (duckduckgo 1)") && r.contains("192.0.2.7:443"),
            "{r}"
        );
        assert!(report(&log, since, 24, true, None).contains("isn't running"));
    }

    #[test]
    fn only_established_connections_off_this_computer_count() {
        let table = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n\
   0: 0100007F:B375 0100007F:4370 01 00000000:00000000 00:00000000 00000000  1000        0 11 1\n\
   1: 0F02000A:C000 070200C0:01BB 01 00000000:00000000 00:00000000 00000000  1000        0 12 1\n\
   2: 0F02000A:C001 080200C0:01BB 06 00000000:00000000 00:00000000 00000000  1000        0 13 1\n\
   3: 0F02000A:C002 090200C0:01BB 01 00000000:00000000 00:00000000 00000000  1000        0 99 1\n";
        let mine: HashSet<u64> = [11, 12, 13].into();
        assert_eq!(
            remote_in(table, &mine, false),
            vec!["192.0.2.7:443".to_string()]
        );
        let netstat = "  TCP    10.0.2.15:49700   192.0.2.7:443    ESTABLISHED     4242\n  TCP    127.0.0.1:49701   127.0.0.1:18432  ESTABLISHED     4242\n  TCP    10.0.2.15:49702   192.0.2.8:443    ESTABLISHED     9999\n";
        assert_eq!(
            windows_remote(netstat, &["4242".to_string()].into()),
            vec!["192.0.2.7:443".to_string()]
        );
    }
}
