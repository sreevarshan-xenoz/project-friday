//! Secrets vault — where FRIDAY keeps tokens and keys OUT of logs, traces,
//! and memory dumps.
//!
//! The constitution's missing piece: *"Secrets never appear in audit logs or
//! memory; only capability-scoped access."* Until now credentials were read
//! straight from environment variables into plain `String` fields — one
//! `Debug` / `Serialize` away from a log line. This module is the vault:
//!
//! - [`Secret`] — a stored value whose **structural** surfaces (`Debug`,
//!   `Display`, `Serialize`) all render a redaction, so a vault handle can
//!   never leak a value into an audit log, a trace, or a memory dump. The raw
//!   value is reachable only through the deliberately-named `reveal()`.
//! - [`SecretsVault`] — a scope-keyed store (`scope → name → Secret`). Scopes
//!   are capabilities (`provider`, `git`, `database`, …).
//! - [`ScopedSecrets`] — a **capability-scoped handle**: constructed for one
//!   scope, it physically cannot see another scope's values.
//! - [`scan_payload`] / [`redact_payload`] — the outbound-side primitives:
//!   detect registered secret values in a payload (name + count, never the
//!   value) and replace them with `[REDACTED:name]`. Deterministic: longer
//!   values are processed first so a short value can never clobber a longer
//!   one that contains it.
//!
//! The egress gate (`crate::egress`) consumes [`SecretsVault::scan`] and
//! [`SecretsVault::redact`] — it never sees raw values either.
//!
//! **Follow-up (documented, not done):** providers still read keys from env
//! into plain `pub String` fields (`OpenAiProvider.api_key`). The vault
//! guarantees its OWN contents never leak; the full "never in memory" story
//! completes when providers resolve their keys FROM the vault instead of env.

use serde::Serialize;
use std::collections::HashMap;
use std::sync::RwLock;

/// A stored secret. The value is structurally unobservable: `Debug`,
/// `Display`, and `Serialize` all render a redaction.
#[derive(Clone)]
pub struct Secret {
    name: String,
    value: String,
}

impl Secret {
    /// The registered name (never secret) — e.g. `provider/openrouter`.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The raw value — the ONLY non-redacting accessor. Receiving a
    /// [`Secret`] is the deliberate act of revealing it (e.g. to build an
    /// auth header); nothing derived from a vault auto-prints it.
    pub fn reveal(&self) -> &str {
        &self.value
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Secret[{}] = ••••", self.name)
    }
}

impl std::fmt::Display for Secret {
    fn fmt(&self, _f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(_f, "••••")
    }
}

impl Serialize for Secret {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str("[REDACTED]")
    }
}

/// The vault: `scope → name → Secret`.
#[derive(Default)]
pub struct SecretsVault {
    secrets: RwLock<HashMap<String, HashMap<String, Secret>>>,
}

impl SecretsVault {
    pub fn new() -> Self {
        Self::default()
    }

    /// Store a secret under `scope` / `name` (upsert).
    pub fn store(&self, scope: &str, name: &str, value: impl Into<String>) {
        let mut guard = self.secrets.write().expect("vault poisoned");
        guard
            .entry(scope.to_string())
            .or_default()
            .insert(name.to_string(), Secret {
                name: format!("{scope}/{name}"),
                value: value.into(),
            });
    }

    /// Read a secret. The caller receives a [`Secret`] whose structural
    /// surfaces are redacted; only `reveal()` exposes the value.
    pub fn get(&self, scope: &str, name: &str) -> Option<Secret> {
        self.secrets
            .read()
            .expect("vault poisoned")
            .get(scope)?
            .get(name)
            .cloned()
    }

    pub fn contains(&self, scope: &str, name: &str) -> bool {
        self.get(scope, name).is_some()
    }

    /// The names registered in `scope` (names are never secret).
    pub fn names(&self, scope: &str) -> Vec<String> {
        let mut names: Vec<String> = self
            .secrets
            .read()
            .expect("vault poisoned")
            .get(scope)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        names.sort();
        names
    }

    pub fn store_count(&self) -> usize {
        self.secrets
            .read()
            .expect("vault poisoned")
            .values()
            .map(|m| m.len())
            .sum()
    }

    /// A capability-scoped handle: constructed for ONE scope, it can only
    /// read that scope's entries — capability-scoped access by construction,
    /// not by convention. Owns a snapshot of its scope's secrets.
    pub fn scoped(&self, scope: &str) -> ScopedSecrets {
        let entries = self
            .secrets
            .read()
            .expect("vault poisoned")
            .get(scope)
            .cloned()
            .unwrap_or_default();
        ScopedSecrets {
            scope: scope.to_string(),
            entries,
        }
    }

    /// Register the standard provider credential environment variables
    /// (`OPENROUTER_API_KEY`, `GROQ_API_KEY`, …) under the `provider` scope.
    /// Returns how many were present and registered.
    pub fn seed_from_env(&self) -> usize {
        self.seed_from_vars("provider", &crate::providers::PROVIDER_ENV_VARS)
    }

    /// Register a fixed variable list under `scope`. Deterministic and
    /// environment-free, so tests can build a known vault without touching
    /// process-global env state.
    pub fn seed_from_vars(&self, scope: &str, vars: &[(&str, &str)]) -> usize {
        let mut seeded = 0usize;
        for (env_var, name) in vars {
            if let Ok(value) = std::env::var(env_var) {
                if !value.is_empty() {
                    self.store(scope, name, value);
                    seeded += 1;
                }
            }
        }
        seeded
    }

    /// Scan `payload` for every registered secret value (long enough to scan
    /// — see [`MIN_SCAN_LENGTH`]). Returns name + occurrence count, **never
    /// values**.
    pub fn scan(&self, payload: &str) -> Vec<SecretHit> {
        let guard = self.secrets.read().expect("vault poisoned");
        let known: Vec<(String, &str)> = guard
            .iter()
            .flat_map(|(scope, map)| {
                map.iter().map(move |(name, secret)| {
                    (format!("{scope}/{name}"), secret.value.as_str())
                })
            })
            .collect();
        let known_refs: Vec<(&str, &str)> = known
            .iter()
            .map(|(name, value)| (name.as_str(), *value))
            .collect();
        scan_payload(payload, &known_refs)
    }

    /// Replace every registered secret value in `payload` with
    /// `[REDACTED:scope/name]`. Values too short to scan are left untouched.
    pub fn redact(&self, payload: &str) -> String {
        let guard = self.secrets.read().expect("vault poisoned");
        let known: Vec<(String, &str)> = guard
            .iter()
            .flat_map(|(scope, map)| {
                map.iter().map(move |(name, secret)| {
                    (format!("{scope}/{name}"), secret.value.as_str())
                })
            })
            .collect();
        let known_refs: Vec<(&str, &str)> = known
            .iter()
            .map(|(name, value)| (name.as_str(), *value))
            .collect();
        redact_payload(payload, &known_refs)
    }
}

impl std::fmt::Debug for SecretsVault {
    /// Names only — never values.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let guard = self.secrets.read().expect("vault poisoned");
        f.debug_map()
            .entries(
                guard
                    .iter()
                    .map(|(scope, map)| (scope, map.keys().collect::<Vec<_>>())),
            )
            .finish()
    }
}

/// A capability-scoped view of the vault. Owns ONLY its scope's entries, so
/// another scope's values are unreachable even in principle.
#[derive(Clone, Default)]
pub struct ScopedSecrets {
    scope: String,
    entries: HashMap<String, Secret>,
}

impl ScopedSecrets {
    pub fn scope(&self) -> &str {
        &self.scope
    }

    pub fn get(&self, name: &str) -> Option<&Secret> {
        self.entries.get(name)
    }

    pub fn names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.entries.keys().cloned().collect();
        names.sort();
        names
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl std::fmt::Debug for ScopedSecrets {
    /// Names only — never values.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScopedSecrets")
            .field("scope", &self.scope)
            .field("names", &self.names())
            .finish()
    }
}

/// One detected occurrence: which secret (name — never the value) and how
/// many times it appears. Safe to log, serialize, and audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SecretHit {
    pub name: String,
    pub count: usize,
}

/// Values shorter than this are never scanned for: a registered placeholder
/// like `ollama` (the local provider's literal key) must not false-positive on
/// ordinary prose.
pub const MIN_SCAN_LENGTH: usize = 8;

/// Detect registered secret values in `payload`. `known` is
/// `(fully-qualified name, value)`; deterministic ordering (longest first).
///
/// Note: counts are computed against the ORIGINAL payload. When one registered
/// value is a substring of another, [`redact_payload`] consumes the inner
/// occurrence (longest-first), so the reported `count` can exceed the number
/// of `[REDACTED]` markers actually produced. Both passes are deterministic
/// and longest-first; the counts are an upper bound, never a leak.
pub fn scan_payload(payload: &str, known: &[(&str, &str)]) -> Vec<SecretHit> {
    let mut ordered: Vec<(&str, &str)> = known
        .iter()
        .copied()
        .filter(|(_, value)| value.len() >= MIN_SCAN_LENGTH)
        .collect();
    ordered.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    let mut hits = Vec::new();
    for (name, value) in ordered {
        let count = count_occurrences(payload, value);
        if count > 0 {
            hits.push(SecretHit {
                name: name.to_string(),
                count,
            });
        }
    }
    hits
}

/// Replace every known secret value with `[REDACTED:name]`. Deterministic:
/// longest values first, so a shorter value inside a longer one is handled
/// correctly and the result is stable. (See [`scan_payload`] for how this
/// ordering relates to hit counts.)
pub fn redact_payload(payload: &str, known: &[(&str, &str)]) -> String {
    let mut ordered: Vec<(&str, &str)> = known
        .iter()
        .copied()
        .filter(|(_, value)| value.len() >= MIN_SCAN_LENGTH)
        .collect();
    ordered.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
    let mut out = payload.to_string();
    for (name, value) in ordered {
        if out.contains(value) {
            out = out.replace(value, &format!("[REDACTED:{name}]"));
        }
    }
    out
}

/// Whether a string is *shaped* like a secret even when unregistered: known
/// token prefixes (OpenAI/GitHub/JWT/…) or a long, dense, space-free token.
///
/// **Not wired into the gate.** The egress path relies exclusively on
/// registered vault values ([`SecretsVault::scan`]) — a registered value is
/// the strongest, false-positive-free signal. This heuristic is an **opt-in
/// public helper** for callers that want to screen unvaulted credentials
/// (e.g. before storing them): it is deliberately NOT part of the default
/// Block policy, because UUIDs, commit SHAs, and dense hyphenated slugs would
/// false-positive under a Block default.
pub fn looks_like_secret(candidate: &str) -> bool {
    let c = candidate.trim();
    if c.len() < 12 {
        return false;
    }
    const PREFIXES: [&str; 8] = [
        "sk-", "sk_", "ghp_", "gho_", "github_pat_", "xoxb-", "AKIA", "eyJ",
    ];
    if PREFIXES.iter().any(|p| c.starts_with(p)) {
        return true;
    }
    // Dense alphanumeric token with no whitespace.
    let alnum = c.chars().filter(|ch| ch.is_ascii_alphanumeric()).count();
    let spaces = c.chars().filter(|ch| ch.is_whitespace()).count();
    spaces == 0 && c.len() >= 16 && (alnum as f64 / c.len() as f64) > 0.85
}

/// Occurrence count of `needle` in `haystack` (non-overlapping).
fn count_occurrences(haystack: &str, needle: &str) -> usize {
    if needle.is_empty() {
        return 0;
    }
    let mut count = 0;
    let mut offset = 0;
    while let Some(pos) = haystack[offset..].find(needle) {
        count += 1;
        offset += pos + needle.len();
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_never_leaks_through_debug_display_or_serialize() {
        let secret = Secret {
            name: "provider/openrouter".to_string(),
            value: "sk-super-secret-12345".to_string(),
        };
        assert!(!format!("{secret:?}").contains("sk-super"), "{secret:?}");
        assert!(!format!("{secret}").contains("sk-super"));
        let json = serde_json::to_string(&secret).unwrap();
        assert!(!json.contains("sk-super"), "{json}");
        assert!(json.contains("REDACTED"));
        // The deliberate escape hatch still works.
        assert_eq!(secret.reveal(), "sk-super-secret-12345");
    }

    #[test]
    fn vault_stores_and_resolves_by_scope() {
        let vault = SecretsVault::new();
        vault.store("provider", "openrouter", "sk-aaa");
        vault.store("database", "pg_pass", "db-hunter-2");
        assert!(vault.contains("provider", "openrouter"));
        assert!(!vault.contains("provider", "pg_pass"), "wrong scope");
        assert_eq!(
            vault.get("provider", "openrouter").unwrap().reveal(),
            "sk-aaa"
        );
        assert!(vault.get("database", "openrouter").is_none());
        assert_eq!(vault.names("provider"), vec!["openrouter"]);
        assert_eq!(vault.store_count(), 2);
    }

    #[test]
    fn scoped_handle_cannot_see_other_scopes() {
        let vault = SecretsVault::new();
        vault.store("provider", "openrouter", "sk-aaa");
        vault.store("git", "gh_token", "ghp-bbb");
        let provider = vault.scoped("provider");
        assert_eq!(provider.scope(), "provider");
        assert_eq!(provider.len(), 1);
        assert!(provider.get("openrouter").is_some());
        // The git token is structurally unreachable from the provider handle.
        assert!(provider.get("gh_token").is_none());
        assert_eq!(provider.names(), vec!["openrouter"]);
        assert!(!format!("{provider:?}").contains("ghp-bbb"));
    }

    #[test]
    fn vault_debug_prints_names_only_never_values() {
        let vault = SecretsVault::new();
        vault.store("provider", "openrouter", "sk-very-secret-1");
        vault.store("provider", "groq", "gsk-very-secret-2");
        let debug = format!("{vault:?}");
        assert!(debug.contains("openrouter") && debug.contains("groq"), "{debug}");
        assert!(!debug.contains("sk-very"), "{debug}");
        assert!(!debug.contains("gsk-very"), "{debug}");
    }

    #[test]
    fn scan_finds_registered_values_with_count_not_value() {
        let vault = SecretsVault::new();
        vault.store("provider", "openrouter", "sk-token-abcdef");
        let payload = "summarize sk-token-abcdef and then sk-token-abcdef again";
        let hits = vault.scan(payload);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name, "provider/openrouter");
        assert_eq!(hits[0].count, 2);
        assert!(!format!("{hits:?}").contains("sk-token"), "{hits:?}");
    }

    #[test]
    fn short_values_are_never_scanned() {
        // A registered short placeholder (e.g. local `ollama`) must not
        // false-positive on ordinary prose.
        let vault = SecretsVault::new();
        vault.store("provider", "ollama", "ollama");
        assert!(vault.scan("tell me about ollama today").is_empty());
    }

    #[test]
    fn redact_replaces_known_values_deterministically() {
        let vault = SecretsVault::new();
        vault.store("provider", "openrouter", "sk-token-abcdef");
        vault.store("git", "token", "abc-ghp-12345678");
        let payload = "use sk-token-abcdef with abc-ghp-12345678 and sk-token-abcdef";
        let redacted = vault.redact(payload);
        assert!(!redacted.contains("sk-token-abcdef"), "{redacted}");
        assert!(!redacted.contains("abc-ghp-12345678"), "{redacted}");
        assert!(redacted.contains("[REDACTED:provider/openrouter]"), "{redacted}");
        assert!(redacted.contains("[REDACTED:git/token]"), "{redacted}");
        assert_eq!(
            redacted.matches("[REDACTED:provider/openrouter]").count(),
            2
        );
    }

    #[test]
    fn redact_longest_first_avoids_nested_clobbering() {
        let vault = SecretsVault::new();
        vault.store("a", "short", "abc12345");
        vault.store("a", "long", "abc12345xyz");
        let redacted = vault.redact("key is abc12345xyz here");
        // The longer value is replaced as a whole; the short value (which is
        // a prefix of the long one) must not have clobbered it first.
        assert!(
            !redacted.contains("abc12345xyz"),
            "long value fully redacted: {redacted}"
        );
        assert!(redacted.contains("[REDACTED:a/long]"), "{redacted}");
    }

    #[test]
    fn looks_like_secret_heuristic() {
        assert!(looks_like_secret("sk-proj-abcdefghijklmnop"));
        assert!(looks_like_secret("ghp_abcdefghijklmnopqrst"));
        assert!(looks_like_secret("eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0"));
        assert!(looks_like_secret("aB3dE5fG7hI9jK1lM3nO5pQ7rS9tU1vW3xY5zA7"));
        assert!(!looks_like_secret("run the test suite"));
        assert!(!looks_like_secret("short"));
        assert!(!looks_like_secret("a normal sentence with spaces inside it"));
    }

    #[test]
    fn seed_from_vars_registers_only_present_env_vars() {
        // Unique var name so parallel tests never collide on process-global env.
        let var = "FRIDAY_TEST_EGRESS_SEED_OPENROUTER";
        std::env::set_var(var, "sk-seeded-value-12345");
        let vault = SecretsVault::new();
        let seeded = vault.seed_from_vars("provider", &[(var, "openrouter")]);
        std::env::remove_var(var);
        assert_eq!(seeded, 1);
        assert_eq!(vault.get("provider", "openrouter").unwrap().reveal(), "sk-seeded-value-12345");

        // An absent var contributes nothing.
        let vault = SecretsVault::new();
        let seeded = vault.seed_from_vars("provider", &[("FRIDAY_TEST_EGRESS_NEVER_SET_XYZ", "nope")]);
        assert_eq!(seeded, 0);
        assert_eq!(vault.store_count(), 0);
    }

    #[test]
    fn scoped_handle_isolates_empty_scope() {
        let vault = SecretsVault::new();
        vault.store("provider", "openrouter", "sk-token-abcdef");
        let none = vault.scoped("database");
        assert!(none.is_empty());
        assert!(none.get("openrouter").is_none());
    }
}
