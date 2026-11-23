//! API keys kept in the desktop's keyring instead of `friday.env`
//! (`friday secrets keychain`). The file then holds `NAME=keyring:` and the
//! value lives in the Secret Service (libsecret, through `secret-tool`) under
//! `service friday name NAME`. At start the daemon and friday-ctl swap each
//! marker for its value. On Windows the tray does the same from Credential
//! Manager before it starts them, so nothing here runs there.

/// What a key's line holds once its value is in the keyring.
pub const MARKER: &str = "keyring:";

/// Puts each keyring-held key's value in the environment, for the providers
/// to read. A key the keyring can't give (locked, gone) is left unset, never
/// passed on as the marker, and the log says which.
pub fn resolve_env() {
    #[cfg(unix)]
    for (name, value) in std::env::vars_os() {
        let (Some(name), true) = (name.to_str().map(str::to_string), value == MARKER) else {
            continue;
        };
        match lookup(&name) {
            Some(v) => std::env::set_var(&name, v),
            None => {
                std::env::remove_var(&name);
                tracing::warn!("{name} is kept in the keyring, which didn't give it (locked?)");
            }
        }
    }
}

/// The keyring's value for `name`, from `secret-tool lookup`. A locked
/// keyring may wait for an unlock prompt: after five seconds the key counts
/// as missing, so FRIDAY still starts.
#[cfg(unix)]
fn lookup(name: &str) -> Option<String> {
    let out = std::process::Command::new("timeout")
        .args([
            "5",
            "secret-tool",
            "lookup",
            "service",
            "friday",
            "name",
            name,
        ])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let value = String::from_utf8(out.stdout).ok()?;
    let value = value.trim_end_matches('\n');
    (out.status.success() && !value.is_empty()).then(|| value.to_string())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A stand-in secret-tool on PATH answers for one key and not another.
    #[test]
    fn markers_become_values_and_a_missing_one_is_dropped() {
        let dir = std::env::temp_dir().join(format!("friday-secrets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let tool = dir.join("secret-tool");
        std::fs::write(
            &tool,
            "#!/bin/sh\n[ \"$5\" = FRIDAY_TEST_ONE_API_KEY ] && { echo kept-value; exit 0; }\nexit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&tool, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        let path = std::env::var("PATH").unwrap_or_default();
        std::env::set_var("PATH", format!("{}:{path}", dir.display()));
        std::env::set_var("FRIDAY_TEST_ONE_API_KEY", MARKER);
        std::env::set_var("FRIDAY_TEST_TWO_API_KEY", MARKER);
        resolve_env();
        assert_eq!(
            std::env::var("FRIDAY_TEST_ONE_API_KEY").as_deref(),
            Ok("kept-value")
        );
        assert!(
            std::env::var("FRIDAY_TEST_TWO_API_KEY").is_err(),
            "no marker passed on"
        );
        std::env::set_var("PATH", path);
        std::env::remove_var("FRIDAY_TEST_ONE_API_KEY");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
