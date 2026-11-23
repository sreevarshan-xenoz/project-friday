//! Egress policy — the outbound-data gate that answers the autonomy
//! prerequisite questions: *"can this secret leave the machine? which
//! provider is receiving it?"*
//!
//! The constitution's missing piece: *"No 'scan payload before sending to an
//! external model' gate … A payload containing a detected secret is
//! blocked/redacted before the provider call; policy is enforceable
//! per-provider."* The permission engine gates what FRIDAY *does*; this gate
//! gates what FRIDAY *sends*.
//!
//! ```text
//! outbound payload (prompt) + provider name
//!      ↓
//! EgressGate::process
//!      ↓  SecretsVault::scan  (registered values + name/count only)
//!      ↓  per-provider EgressMode
//!      ↓
//!   Allow  ──→ original payload leaves
//!   Redact ──→ [REDACTED:scope/name] payload leaves
//!   Block  ──→ Err — the payload never leaves
//! ```
//!
//! **Safe default:** [`EgressGate::new`] defaults to [`EgressMode::Block`] —
//! an attached gate never lets a detected secret out to an unlisted provider.
//! Redact / LogOnly are explicit per-provider opt-ins. The gate is **off
//! until attached** (`InferenceFabric.egress = Some(…)`), so existing
//! behavior is unchanged by default; enabling it is one line.
//!
//! The verdict's [`SecretHit`]s carry names and counts only — the gate never
//! sees raw values (the vault's scan is the only contact point).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::secrets::{SecretHit, SecretsVault};

/// One outbound provider request logged to `egress.jsonl`.
///
/// Records metadata and counts only — NEVER the prompt, answer, key, URL with
/// secrets, or any content.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EgressEntry {
    /// RFC 3339 UTC timestamp.
    pub ts: String,
    /// Goal ID if known (e.g. "goal-123").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goal_id: Option<String>,
    /// Provider identifier (e.g. "opencode", "llamacpp", "claude-code").
    pub provider: String,
    /// Model identifier (e.g. "inclusionai/ling-3.0-flash:free", "gemma4:e4b").
    pub model: String,
    /// Content kinds: `["text"]` or `["text", "image"]`.
    pub kinds: Vec<String>,
    /// Character count of text payload.
    pub text_chars: usize,
    /// Byte count of image payload (0 if none).
    #[serde(default)]
    pub image_bytes: usize,
    /// Number of secrets masked in that ask, if known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub masked: Option<usize>,
    /// Attempt outcome: `"ok"` or `"error"`.
    pub outcome: String,
    /// `true` for local providers (llama.cpp/Ollama on localhost); omitted otherwise.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<bool>,
    /// How long the call took, in milliseconds (`friday usage`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ms: Option<u64>,
    /// How long until its first words, when it streamed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_ms: Option<u64>,
    /// The tokens the provider said it used, when it says.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<u64>,
    /// Why it failed, in one word: `free_limit`, `timeout` or `other`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A failed call's error in one word for the usage page (never its text).
pub fn error_kind(error: &str) -> &'static str {
    let e = error.to_lowercase();
    if e.contains("freeusagelimit")
        || e.contains("429")
        || e.contains("rate limit")
        || e.contains("quota")
    {
        "free_limit"
    } else if e.contains("timed out") || e.contains("timeout") {
        "timeout"
    } else {
        "other"
    }
}

/// Context for the currently running ask, propagated through tokio task-local storage.
#[derive(Clone, Debug, Default)]
pub struct EgressContext {
    pub goal_id: Option<String>,
    pub masked: Option<usize>,
}

tokio::task_local! {
    pub static CURRENT_EGRESS_CONTEXT: EgressContext;
}

/// Retrieve the current task's egress context if set, or default empty.
pub fn current_egress_context() -> EgressContext {
    CURRENT_EGRESS_CONTEXT
        .try_with(|c| c.clone())
        .unwrap_or_default()
}

/// Run a future scoped to `ctx`.
pub async fn with_egress_context<F, R>(ctx: EgressContext, f: F) -> R
where
    F: std::future::Future<Output = R>,
{
    CURRENT_EGRESS_CONTEXT.scope(ctx, f).await
}

/// Default maximum size of `egress.jsonl` before compaction (~1 MB).
pub const DEFAULT_MAX_EGRESS_BYTES: u64 = 1024 * 1024;

/// Resolve the state directory where `state.json` and `egress.jsonl` live.
///
/// Precedence: parent of `FRIDAY_STATE_FILE` (explicit override), then
/// `$XDG_STATE_HOME/friday`, then on Windows `%LOCALAPPDATA%\friday`, then
/// `$HOME/.local/state/friday`, falling back to temp dir.
pub fn state_dir_from(var: impl Fn(&str) -> Option<String>, windows: bool) -> PathBuf {
    if let Some(p) = var("FRIDAY_STATE_FILE") {
        let path = PathBuf::from(p);
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                return parent.to_path_buf();
            }
        }
        return PathBuf::from(".");
    }
    let base = if let Some(x) = var("XDG_STATE_HOME") {
        PathBuf::from(x)
    } else if let Some(local) = var("LOCALAPPDATA").filter(|_| windows) {
        PathBuf::from(local)
    } else if let Some(home) = var("HOME") {
        PathBuf::from(home).join(".local").join("state")
    } else if windows {
        std::env::temp_dir()
    } else {
        PathBuf::from("/tmp")
    };
    base.join("friday")
}

/// The default state directory for the current platform and environment.
pub fn default_state_dir() -> PathBuf {
    state_dir_from(|k| std::env::var(k).ok(), cfg!(windows))
}

/// The default path to `egress.jsonl` (`<state dir>/egress.jsonl` or `FRIDAY_EGRESS_FILE`).
pub fn default_egress_path() -> PathBuf {
    if let Ok(p) = std::env::var("FRIDAY_EGRESS_FILE") {
        PathBuf::from(p)
    } else {
        default_state_dir().join("egress.jsonl")
    }
}

/// Append one JSON line to `egress.jsonl` with default bounding (~1 MB).
/// Writing must never fail an ask (logs warning and continues).
pub fn append_entry(path: &Path, entry: &EgressEntry) {
    append_entry_bounded(path, entry, DEFAULT_MAX_EGRESS_BYTES);
}

/// Append one JSON line to `egress.jsonl` with an explicit size bound.
pub fn append_entry_bounded(path: &Path, entry: &EgressEntry, max_bytes: u64) {
    let result = (|| -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let line = serde_json::to_string(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        {
            use std::io::Write;
            let mut file = options.open(path)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
            }
            writeln!(file, "{}", line)?;
            file.flush()?;
        }

        if let Ok(meta) = std::fs::metadata(path) {
            if meta.len() > max_bytes {
                compact_egress_file(path)?;
            }
        }
        Ok(())
    })();

    if let Err(e) = result {
        tracing::warn!("failed to write egress log at {}: {e}", path.display());
    }
}

/// Compact `egress.jsonl` keeping only the newest half of the lines.
pub fn compact_egress_file(path: &Path) -> std::io::Result<()> {
    let content = std::fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    if lines.is_empty() {
        return Ok(());
    }
    let keep_from = lines.len() / 2;
    let kept = &lines[keep_from..];

    let tmp_path = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4().simple()));
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let write_res = (|| -> std::io::Result<()> {
        use std::io::Write;
        let mut file = options.open(&tmp_path)?;
        for line in kept {
            writeln!(file, "{}", line)?;
        }
        file.flush()?;
        std::fs::rename(&tmp_path, path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    })();

    if write_res.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    write_res
}

/// What the gate does when an outbound payload carries a detected secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EgressMode {
    /// Refuse to send — the payload never leaves the machine.
    Block,
    /// Replace every detected secret with `[REDACTED:scope/name]` and send
    /// the sanitized payload.
    Redact,
    /// Send, but record the hits on the telemetry/audit path for the policy
    /// owner to review. Never the default.
    LogOnly,
}

/// The gate's decision for one outbound payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum EgressAction {
    Allow,
    /// Refused before the call — the reason names the hits (never values).
    Block {
        reason: String,
    },
    Redact,
}

/// One inspection result — safe to log, serialize, and audit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EgressVerdict {
    pub provider: String,
    pub action: EgressAction,
    /// What was found: names and counts only, never values.
    pub hits: Vec<SecretHit>,
}

/// The outbound-data gate. Owns the vault reference and the per-provider
/// policy table; pure (no I/O), so every decision is deterministic and
/// testable.
pub struct EgressGate {
    vault: Arc<SecretsVault>,
    policies: HashMap<String, EgressMode>,
    default_mode: EgressMode,
}

impl EgressGate {
    /// `default_mode` is [`EgressMode::Block`]: with a gate attached, an
    /// unlisted provider never sends a detected secret out. Opt into
    /// Redact / LogOnly per provider via [`EgressGate::with_provider_policy`].
    pub fn new(vault: Arc<SecretsVault>) -> Self {
        Self {
            vault,
            policies: HashMap::new(),
            default_mode: EgressMode::Block,
        }
    }

    /// Per-provider policy — `provider` names the outbound destination
    /// (`openrouter`, `groq`, …), matching the fabric's provider registry.
    pub fn with_provider_policy(mut self, provider: impl Into<String>, mode: EgressMode) -> Self {
        self.policies.insert(provider.into(), mode);
        self
    }

    /// Override the default for unlisted providers.
    pub fn with_default_mode(mut self, mode: EgressMode) -> Self {
        self.default_mode = mode;
        self
    }

    /// The effective policy for `provider`.
    pub fn mode_for(&self, provider: &str) -> EgressMode {
        self.policies
            .get(provider)
            .copied()
            .unwrap_or(self.default_mode)
    }

    /// Inspect an outbound payload: scan for registered secrets and apply the
    /// provider's policy. Pure — the caller decides what to send.
    pub fn inspect(&self, provider: &str, payload: &str) -> EgressVerdict {
        let hits = self.vault.scan(payload);
        let action = if hits.is_empty() {
            EgressAction::Allow
        } else {
            match self.mode_for(provider) {
                EgressMode::Block => EgressAction::Block {
                    reason: format!(
                        "{} detected secret(s): {}",
                        hits.len(),
                        describe_hits(&hits)
                    ),
                },
                EgressMode::Redact => EgressAction::Redact,
                EgressMode::LogOnly => {
                    tracing::warn!(
                        provider,
                        hits = ?hits,
                        "egress: secrets detected in outbound payload (log-only policy)"
                    );
                    EgressAction::Allow
                }
            }
        };
        EgressVerdict {
            provider: provider.to_string(),
            action,
            hits,
        }
    }

    /// The outbound seam: process a payload about to leave to `provider`.
    /// - `Block` → `Err` with a reason (the payload never leaves).
    /// - `Redact` → `Ok` with every detected secret replaced.
    /// - `Allow` / `LogOnly` → `Ok` unchanged.
    pub fn process(&self, provider: &str, payload: &str) -> Result<String, String> {
        let verdict = self.inspect(provider, payload);
        match verdict.action {
            EgressAction::Allow => Ok(payload.to_string()),
            EgressAction::Block { reason } => Err(format!(
                "egress policy blocked outbound payload to '{provider}': {reason}"
            )),
            EgressAction::Redact => Ok(self.vault.redact(payload)),
        }
    }
}

impl std::fmt::Debug for EgressGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EgressGate")
            .field("vault", &self.vault)
            .field("policies", &self.policies)
            .field("default_mode", &self.default_mode)
            .finish()
    }
}

/// Human-readable hit summary — names and counts only.
fn describe_hits(hits: &[SecretHit]) -> String {
    hits.iter()
        .map(|h| format!("{}×{}", h.name, h.count))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gate_with_secret(secret_value: &str) -> (Arc<SecretsVault>, EgressGate) {
        let vault = Arc::new(SecretsVault::new());
        vault.store("provider", "openrouter", secret_value);
        let gate = EgressGate::new(Arc::clone(&vault));
        (vault, gate)
    }

    #[test]
    fn clean_payload_always_allows() {
        let (_, gate) = gate_with_secret("sk-token-abcdef");
        let verdict = gate.inspect("openrouter", "summarize the quarterly report");
        assert_eq!(verdict.action, EgressAction::Allow);
        assert!(verdict.hits.is_empty());
        assert_eq!(
            gate.process("openrouter", "summarize the quarterly report")
                .unwrap(),
            "summarize the quarterly report"
        );
    }

    #[test]
    fn block_mode_blocks_a_registered_secret_before_the_call() {
        let (_, gate) = gate_with_secret("sk-token-abcdef");
        let payload = "here is the key sk-token-abcdef please use it";
        let verdict = gate.inspect("openrouter", payload);
        assert!(matches!(verdict.action, EgressAction::Block { .. }));
        assert_eq!(verdict.hits.len(), 1);
        assert_eq!(verdict.hits[0].name, "provider/openrouter");

        let err = gate.process("openrouter", payload).unwrap_err();
        assert!(err.contains("egress policy blocked"), "{err}");
        assert!(err.contains("provider/openrouter"), "{err}");
        assert!(
            !err.contains("sk-token-abcdef"),
            "reason never carries the value: {err}"
        );
    }

    #[test]
    fn redact_mode_redacts_and_still_sends() {
        let (_, gate) = gate_with_secret("sk-token-abcdef");
        let gate = gate.with_provider_policy("openrouter", EgressMode::Redact);
        let payload = "use sk-token-abcdef and again sk-token-abcdef";
        let verdict = gate.inspect("openrouter", payload);
        assert_eq!(verdict.action, EgressAction::Redact);
        assert_eq!(verdict.hits[0].count, 2);

        let sent = gate.process("openrouter", payload).unwrap();
        assert!(!sent.contains("sk-token-abcdef"), "{sent}");
        assert_eq!(
            sent.matches("[REDACTED:provider/openrouter]").count(),
            2,
            "{sent}"
        );
    }

    #[test]
    fn log_only_policy_allows_but_reports() {
        let (_, gate) = gate_with_secret("sk-token-abcdef");
        let gate = gate.with_provider_policy("openrouter", EgressMode::LogOnly);
        let verdict = gate.inspect("openrouter", "mention sk-token-abcdef here");
        assert_eq!(
            verdict.action,
            EgressAction::Allow,
            "payload still goes out"
        );
        assert_eq!(verdict.hits.len(), 1, "but the hits are reported");
    }

    #[test]
    fn policy_is_enforced_per_provider() {
        let (_, gate) = gate_with_secret("sk-token-abcdef");
        // openrouter: Redact; groq unlisted → default Block.
        let gate = gate.with_provider_policy("openrouter", EgressMode::Redact);
        let payload = "contains sk-token-abcdef";

        let verdict = gate.inspect("openrouter", payload);
        assert_eq!(verdict.action, EgressAction::Redact);
        let verdict = gate.inspect("groq", payload);
        assert!(matches!(verdict.action, EgressAction::Block { .. }));
    }

    #[test]
    fn default_mode_can_be_softened() {
        let (_, gate) = gate_with_secret("sk-token-abcdef");
        let gate = gate.with_default_mode(EgressMode::Redact);
        let verdict = gate.inspect("unlisted-provider", "has sk-token-abcdef");
        assert_eq!(verdict.action, EgressAction::Redact);
    }

    #[test]
    fn verdict_never_carries_secret_values() {
        let (_, gate) = gate_with_secret("sk-token-abcdef");
        let verdict = gate.inspect("openrouter", "the secret sk-token-abcdef is here");
        let debug = format!("{verdict:?}");
        assert!(!debug.contains("sk-token-abcdef"), "{debug}");
        let json = serde_json::to_string(&verdict).unwrap();
        assert!(!json.contains("sk-token-abcdef"), "{json}");
    }

    #[test]
    fn gate_debug_shows_policies_but_never_values() {
        let (_, gate) = gate_with_secret("sk-token-abcdef");
        let gate = gate.with_provider_policy("openrouter", EgressMode::Redact);
        let debug = format!("{gate:?}");
        assert!(debug.contains("openrouter"), "{debug}");
        assert!(!debug.contains("sk-token-abcdef"), "{debug}");
    }

    #[test]
    fn egress_entry_serialization_fields_and_no_content() {
        let entry = EgressEntry {
            ts: "2026-11-14T18:00:00Z".to_string(),
            goal_id: Some("goal-12345".to_string()),
            provider: "opencode".to_string(),
            model: "inclusionai/ling-3.0-flash:free".to_string(),
            kinds: vec!["text".to_string()],
            text_chars: 42,
            image_bytes: 0,
            masked: Some(1),
            outcome: "ok".to_string(),
            local: None,
            ..Default::default()
        };
        let json = serde_json::to_string(&entry).unwrap();
        // Check fields
        assert!(json.contains("\"ts\":\"2026-11-14T18:00:00Z\""));
        assert!(json.contains("\"goal_id\":\"goal-12345\""));
        assert!(json.contains("\"provider\":\"opencode\""));
        assert!(json.contains("\"model\":\"inclusionai/ling-3.0-flash:free\""));
        assert!(json.contains("\"kinds\":[\"text\"]"));
        assert!(json.contains("\"text_chars\":42"));
        assert!(json.contains("\"image_bytes\":0"));
        assert!(json.contains("\"masked\":1"));
        assert!(json.contains("\"outcome\":\"ok\""));
        assert!(!json.contains("\"local\""));
        // Never contains prompt or content
        assert!(!json.contains("prompt"));
        assert!(!json.contains("content"));
        assert!(!json.contains("answer"));

        // Roundtrip
        let parsed: EgressEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, entry);

        // Local provider serialization
        let local_entry = EgressEntry {
            local: Some(true),
            ..entry
        };
        let local_json = serde_json::to_string(&local_entry).unwrap();
        assert!(local_json.contains("\"local\":true"));
    }

    #[test]
    fn egress_entry_file_mode_0600_on_unix() {
        let dir = std::env::temp_dir().join(format!(
            "egress_perm_test_{}",
            uuid::Uuid::new_v4().simple()
        ));
        let path = dir.join("egress.jsonl");

        let entry = EgressEntry {
            ts: "2026-11-14T18:00:00Z".to_string(),
            goal_id: None,
            provider: "llamacpp".to_string(),
            model: "gemma4:e4b".to_string(),
            kinds: vec!["text".to_string()],
            text_chars: 10,
            image_bytes: 0,
            masked: None,
            outcome: "ok".to_string(),
            local: Some(true),
            ..Default::default()
        };

        append_entry(&path, &entry);
        assert!(path.exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&path).unwrap();
            let mode = meta.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "file mode must be 0600 on Unix, got {mode:o}");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn egress_entry_bounding_keeps_newest_half() {
        let dir = std::env::temp_dir().join(format!(
            "egress_bound_test_{}",
            uuid::Uuid::new_v4().simple()
        ));
        let path = dir.join("egress.jsonl");

        // Write 6 entries with a bound that triggers after a couple lines
        for i in 0..6 {
            let entry = EgressEntry {
                ts: "2026-11-14T18:00:00Z".to_string(),
                goal_id: Some(format!("goal-{i}")),
                provider: "test-prov".to_string(),
                model: "test-model".to_string(),
                kinds: vec!["text".to_string()],
                text_chars: i * 100,
                image_bytes: 0,
                masked: None,
                outcome: "ok".to_string(),
                local: None,
                ..Default::default()
            };
            // Use max_bytes = 1000 so initial writes fit, but by entry 5 it exceeds ~600 bytes
            // Let's use max_bytes = 400
            append_entry_bounded(&path, &entry, 400);
        }

        let content = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
        // It must have compacted, keeping newest half
        assert!(
            lines.len() < 6,
            "should have compacted, got {} lines",
            lines.len()
        );
        // The last entry (goal-5) must be present in the kept lines
        assert!(lines.last().unwrap().contains("goal-5"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let meta = std::fs::metadata(&path).unwrap();
            let mode = meta.permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "compacted file mode must remain 0600 on Unix");
        }

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn egress_entry_writing_never_panics_on_error() {
        // Appending to an invalid path must not panic
        let invalid_path = PathBuf::from("/dev/null/impossible/egress.jsonl");
        let entry = EgressEntry {
            ts: "2026-11-14T18:00:00Z".to_string(),
            goal_id: None,
            provider: "prov".to_string(),
            model: "model".to_string(),
            kinds: vec!["text".to_string()],
            text_chars: 1,
            image_bytes: 0,
            masked: None,
            outcome: "ok".to_string(),
            local: None,
            ..Default::default()
        };
        append_entry(&invalid_path, &entry);
    }
}
