//! Package name validation and protected package policy.
//!
//! Guarantees that package arguments cannot be manipulated into command-line
//! options, path traversals, or shell metacharacters, and prevents the removal
//! of critical system packages.

/// Explicitly protected packages that can never be removed.
pub const PROTECTED: &[&str] = &[
    "base",
    "base-devel",
    "pacman",
    "pacman-contrib",
    "glibc",
    "bash",
    "coreutils",
    "filesystem",
    "polkit",
    "sudo",
    "util-linux",
    "shadow",
    "dbus",
    "networkmanager",
    "grub",
    "efibootmgr",
    "mkinitcpio",
    "friday-git",
    "friday-os-helper",
    "hyprland",
    "quickshell",
    "sddm",
];

/// Validates that a package name is safe and conforms to the pacman naming convention.
///
/// Rules:
/// - Matching pattern `^[a-z0-9@._+-]{1,64}$`
/// - Must not start with `'-'` or `'.'`
/// - Must not contain `".."`
/// - Cannot look like an option or path.
pub fn valid_package_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 64 {
        return false;
    }
    if name.starts_with('-') || name.starts_with('.') {
        return false;
    }
    if name.contains("..") {
        return false;
    }
    name.chars().all(|c| {
        c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '@' | '.' | '_' | '+' | '-')
    })
}

/// Checks whether a package is protected and must NEVER be removed.
///
/// Refuses removal of `PROTECTED` packages and any package whose name
/// starts with `"linux"`, `"systemd"`, or `"friday"`.
pub fn is_protected_package(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    if lower.starts_with("linux") || lower.starts_with("systemd") || lower.starts_with("friday") {
        return true;
    }
    PROTECTED.contains(&lower.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_package_name_table() {
        // Valid package names
        assert!(valid_package_name("htop"));
        assert!(valid_package_name("gtk4"));
        assert!(valid_package_name("python-yaml"));
        assert!(valid_package_name("lib32-mesa"));
        assert!(valid_package_name("g++"));

        // Invalid package names
        assert!(!valid_package_name("")); // empty
        assert!(!valid_package_name("-S")); // starts with -
        assert!(!valid_package_name("--foo")); // option
        assert!(!valid_package_name(".x")); // starts with .
        assert!(!valid_package_name("a b")); // whitespace
        assert!(!valid_package_name("a;b")); // metacharacter
        assert!(!valid_package_name("../x")); // path traversal
        assert!(!valid_package_name("x/y")); // path separator
        assert!(!valid_package_name("A")); // uppercase
        assert!(!valid_package_name(&"a".repeat(65))); // 65 chars
        assert!(!valid_package_name("$(x)")); // shell command substitution
    }

    #[test]
    fn test_protected_packages() {
        // Explicitly listed packages
        for pkg in &[
            "base",
            "base-devel",
            "pacman",
            "pacman-contrib",
            "glibc",
            "bash",
            "coreutils",
            "filesystem",
            "polkit",
            "sudo",
            "util-linux",
            "shadow",
            "dbus",
            "networkmanager",
            "grub",
            "efibootmgr",
            "mkinitcpio",
            "friday-git",
            "friday-os-helper",
            "hyprland",
            "quickshell",
            "sddm",
        ] {
            assert!(is_protected_package(pkg), "expected {pkg} to be protected");
        }

        // Prefix rules: linux*, systemd*, friday*
        assert!(is_protected_package("linux"));
        assert!(is_protected_package("linux-lts"));
        assert!(is_protected_package("linux-firmware"));
        assert!(is_protected_package("linux-headers"));
        assert!(is_protected_package("systemd"));
        assert!(is_protected_package("systemd-libs"));
        assert!(is_protected_package("systemd-sysvcompat"));
        assert!(is_protected_package("friday-git"));
        assert!(is_protected_package("friday-custom"));

        // Non-protected packages allowed
        assert!(!is_protected_package("htop"));
        assert!(!is_protected_package("neovim"));
        assert!(!is_protected_package("curl"));
    }
}
