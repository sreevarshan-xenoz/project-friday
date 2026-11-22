//! Features the person switched off (`friday feature <name> off`, the
//! Features page). The list is read each time, so a switch counts from the
//! next ask without a restart.

use std::path::PathBuf;

/// `features-off` in FRIDAY's config folder (`$XDG_CONFIG_HOME/friday`), one
/// name per line. On Windows the tray lists them in `FRIDAY_FEATURES_OFF`,
/// comma-separated, instead.
fn off_file() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
    base.unwrap_or_default().join("friday").join("features-off")
}

/// Whether `name` is on: not a line of the file, nor in the variable.
pub fn on(name: &str) -> bool {
    let file = std::fs::read_to_string(off_file()).unwrap_or_default();
    let var = std::env::var("FRIDAY_FEATURES_OFF").unwrap_or_default();
    on_in(&file, &var, name)
}

fn on_in(file: &str, var: &str, name: &str) -> bool {
    !file.lines().any(|l| l.trim() == name) && !var.split(',').any(|v| v.trim() == name)
}

#[cfg(test)]
mod tests {
    use super::on_in;

    #[test]
    fn a_feature_is_on_unless_listed() {
        assert!(on_in("", "", "area-text"));
        assert!(!on_in("mentions\narea-text\n", "", "area-text"));
        assert!(!on_in("", "update-check, couldnt-check", "couldnt-check"));
        // Whole names only.
        assert!(on_in("area-text-old\n", "area", "area-text"));
    }
}
