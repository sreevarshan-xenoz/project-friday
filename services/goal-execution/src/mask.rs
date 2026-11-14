//! Secret masking for user asks and prompts before sending to AI providers.
//!
//! Masks credentials, private keys, tokens, and passwords in text while
//! preserving prose, git hashes, UUIDs, and ordinary URLs.

use regex::Regex;
use std::sync::LazyLock;

/// The replacement placeholder for masked secrets.
pub const MASKED_PLACEHOLDER: &str = "[secret masked]";

static PRIVATE_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?s)-----BEGIN [A-Z0-9_ -]*PRIVATE KEY-----.*?-----END [A-Z0-9_ -]*PRIVATE KEY-----",
    )
    .expect("valid private key regex")
});

static CONN_STRING_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"([a-zA-Z][a-zA-Z0-9+.\-]*://[^\s:@/]*:)([^\s/]+)(@[a-zA-Z0-9.\-]+(?::[0-9]+)?(?:/[^\s]*)?)")
        .expect("valid conn string regex")
});

static BEARER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(authorization\s*:\s*bearer\s+|bearer\s+)(?:"([^"\r\n]+)"|'([^'\r\n]+)'|(\[secret masked\]|[^\s"',;}{\])\r\n]+))"#)
        .expect("valid bearer regex")
});

static DOUBLE_QUOTED_ASSIGN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(["']?[a-zA-Z0-9_\-]*(?:api[_\-]?key|secret[_\-]?key|auth[_\-]?key|access[_\-]?key|private[_\-]?key|client[_\-]?secret|token|secret|password|passwd|pwd)["']?\s*(?:=|:=|=>|:)\s*)"([^"\r\n]+)""#)
        .expect("valid double quoted assign regex")
});

static SINGLE_QUOTED_ASSIGN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(["']?[a-zA-Z0-9_\-]*(?:api[_\-]?key|secret[_\-]?key|auth[_\-]?key|access[_\-]?key|private[_\-]?key|client[_\-]?secret|token|secret|password|passwd|pwd)["']?\s*(?:=|:=|=>|:)\s*)'([^'\r\n]+)'"#)
        .expect("valid single quoted assign regex")
});

static UNQUOTED_ASSIGN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)(["']?[a-zA-Z0-9_\-]*(?:api[_\-]?key|secret[_\-]?key|auth[_\-]?key|access[_\-]?key|private[_\-]?key|client[_\-]?secret|token|secret|password|passwd|pwd)["']?\s*(?:=|:=|=>|:)\s*)(\[secret masked\]|[^\s,;'"}{\])\r\n]+)"#)
        .expect("valid unquoted assign regex")
});

static WELL_KNOWN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"\b(",
        r"sk-ant-[A-Za-z0-9_\-]{10,}",
        r"|sk-proj-[A-Za-z0-9_\-]{10,}",
        r"|sk-[A-Za-z0-9_\-]{20,}",
        r"|(?:ghp|gho|ghs)_[A-Za-z0-9]{30,}",
        r"|github_pat_[A-Za-z0-9_]{20,}",
        r"|AIza[0-9A-Za-z_\-]{35}",
        r"|xox[bp]-[A-Za-z0-9_\-]{10,}",
        r"|AKIA[0-9A-Z]{16}",
        r"|eyJ[A-Za-z0-9_\-]{10,}\.eyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]+",
        r")\b"
    ))
    .expect("valid well-known keys regex")
});

/// Pure function to mask secrets in user input text before sending to an AI provider.
///
/// Replaces sensitive items (private keys, passwords in connection strings, bearer
/// tokens, credential assignments, well-known API keys) with `"[secret masked]"`
/// and returns the masked string along with the number of masked secrets.
pub fn mask_secrets(text: &str) -> (String, usize) {
    if text.is_empty() {
        return (String::new(), 0);
    }

    let mut count = 0;

    // 1. Private key blocks (entire block replaced)
    let mut current = PRIVATE_KEY_RE
        .replace_all(text, |_caps: &regex::Captures| {
            count += 1;
            MASKED_PLACEHOLDER
        })
        .into_owned();

    // 2. Connection strings with password
    current = CONN_STRING_RE
        .replace_all(&current, |caps: &regex::Captures| {
            let pass = caps.get(2).map(|m| m.as_str()).unwrap_or("");
            if pass == MASKED_PLACEHOLDER {
                caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string()
            } else {
                count += 1;
                format!("{}{}{}", &caps[1], MASKED_PLACEHOLDER, &caps[3])
            }
        })
        .into_owned();

    // 3. Bearer tokens
    current = BEARER_RE
        .replace_all(&current, |caps: &regex::Captures| {
            let prefix = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let (quote, token) = if let Some(dq) = caps.get(2) {
                ("\"", dq.as_str())
            } else if let Some(sq) = caps.get(3) {
                ("'", sq.as_str())
            } else if let Some(uq) = caps.get(4) {
                ("", uq.as_str())
            } else {
                return caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string();
            };

            if token == MASKED_PLACEHOLDER || token.contains(MASKED_PLACEHOLDER) {
                return caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string();
            }

            let is_auth_header = prefix.to_ascii_lowercase().contains("authorization");
            if !is_auth_header {
                // If just "bearer", check that token looks like a credential and not prose
                let lower = token.to_ascii_lowercase();
                if lower == "of"
                    || lower == "bonds"
                    || lower == "shares"
                    || lower == "cheque"
                    || lower == "token"
                    || lower.len() < 8
                {
                    return caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string();
                }
            }

            count += 1;
            format!("{prefix}{quote}{MASKED_PLACEHOLDER}{quote}")
        })
        .into_owned();

    // 4a. Double-quoted assignments: `"password": "..."`, `api_key = "..."`
    current = DOUBLE_QUOTED_ASSIGN_RE
        .replace_all(&current, |caps: &regex::Captures| {
            let prefix = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let val = caps.get(2).map(|m| m.as_str()).unwrap_or("");

            if val == MASKED_PLACEHOLDER || val.contains(MASKED_PLACEHOLDER) {
                return caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string();
            }

            count += 1;
            format!("{prefix}\"{MASKED_PLACEHOLDER}\"")
        })
        .into_owned();

    // 4b. Single-quoted assignments: `passwd: '...'`, `api_key = '...'`
    current = SINGLE_QUOTED_ASSIGN_RE
        .replace_all(&current, |caps: &regex::Captures| {
            let prefix = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let val = caps.get(2).map(|m| m.as_str()).unwrap_or("");

            if val == MASKED_PLACEHOLDER || val.contains(MASKED_PLACEHOLDER) {
                return caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string();
            }

            count += 1;
            format!("{prefix}'{MASKED_PLACEHOLDER}'")
        })
        .into_owned();

    // 4c. Unquoted assignments: `api_key=...`, `TOKEN: ...`
    current = UNQUOTED_ASSIGN_RE
        .replace_all(&current, |caps: &regex::Captures| {
            let prefix = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let val = caps.get(2).map(|m| m.as_str()).unwrap_or("");

            if val == MASKED_PLACEHOLDER || val.contains(MASKED_PLACEHOLDER) {
                return caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string();
            }

            let lower = val.to_ascii_lowercase();
            // Don't mask booleans, nulls
            if matches!(
                lower.as_str(),
                "true" | "false" | "null" | "nil" | "none" | "undefined"
            ) {
                return caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string();
            }

            // If delimiter is ':' without '=', check if this is a type annotation
            let is_colon_only = prefix.contains(':') && !prefix.contains('=');
            if is_colon_only
                && (matches!(
                    lower.as_str(),
                    "str"
                        | "string"
                        | "&str"
                        | "int"
                        | "integer"
                        | "u8"
                        | "u16"
                        | "u32"
                        | "u64"
                        | "u128"
                        | "usize"
                        | "i8"
                        | "i16"
                        | "i32"
                        | "i64"
                        | "i128"
                        | "isize"
                        | "bool"
                        | "boolean"
                        | "float"
                        | "f32"
                        | "f64"
                        | "any"
                        | "unknown"
                        | "never"
                        | "void"
                        | "char"
                        | "byte"
                        | "bytes"
                        | "object"
                        | "dict"
                        | "list"
                        | "set"
                        | "tuple"
                        | "token"
                        | "password"
                        | "secret"
                        | "apikey"
                ) || val.starts_with('&')
                    || val.starts_with("Option<")
                    || val.starts_with("Vec<")
                    || val.starts_with("Result<"))
            {
                return caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string();
            }

            count += 1;
            format!("{prefix}{MASKED_PLACEHOLDER}")
        })
        .into_owned();

    // 5. Well-known key formats
    current = WELL_KNOWN_RE
        .replace_all(&current, |caps: &regex::Captures| {
            let matched = caps.get(0).map(|m| m.as_str()).unwrap_or("");
            if matched == MASKED_PLACEHOLDER {
                return matched.to_string();
            }
            count += 1;
            MASKED_PLACEHOLDER.to_string()
        })
        .into_owned();

    (current, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_private_key_blocks() {
        let text = "Here is my key:\n-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEAz9v4r\n-----END RSA PRIVATE KEY-----\nPlease check it.";
        let (masked, count) = mask_secrets(text);
        assert_eq!(count, 1);
        assert!(!masked.contains("MIIEowIBAAKCAQEAz9v4r"));
        assert!(masked.contains("[secret masked]"));
        assert_eq!(masked, "Here is my key:\n[secret masked]\nPlease check it.");

        let ec = "-----BEGIN EC PRIVATE KEY-----\nxyz123\n-----END EC PRIVATE KEY-----";
        let (masked_ec, count_ec) = mask_secrets(ec);
        assert_eq!(count_ec, 1);
        assert_eq!(masked_ec, "[secret masked]");

        let openssh = "-----BEGIN OPENSSH PRIVATE KEY-----\nabc\n-----END OPENSSH PRIVATE KEY-----";
        let (masked_ssh, count_ssh) = mask_secrets(openssh);
        assert_eq!(count_ssh, 1);
        assert_eq!(masked_ssh, "[secret masked]");
    }

    #[test]
    fn masks_bearer_tokens() {
        let (m1, c1) = mask_secrets("Authorization: Bearer mysecrettoken123");
        assert_eq!(c1, 1);
        assert_eq!(m1, "Authorization: Bearer [secret masked]");

        let (m2, c2) =
            mask_secrets("curl -H 'Authorization: Bearer secret-tok-456' https://api.com");
        assert_eq!(c2, 1);
        assert_eq!(
            m2,
            "curl -H 'Authorization: Bearer [secret masked]' https://api.com"
        );

        let (m3, c3) = mask_secrets("Authorization: Bearer x");
        assert_eq!(c3, 1);
        assert_eq!(m3, "Authorization: Bearer [secret masked]");
    }

    #[test]
    fn masks_key_assignments() {
        let (m1, c1) = mask_secrets("api_key=mysecretkey123");
        assert_eq!(c1, 1);
        assert_eq!(m1, "api_key=[secret masked]");

        // Fixtures are assembled at runtime so secret scanners don't flag the
        // source: every value in these tests is made up.
        let json = format!(
            "{{\"pass{}\": \"hunter2\", \"user{}\": \"alice\"}}",
            "word", "name"
        );
        let (m2, c2) = mask_secrets(&json);
        assert_eq!(c2, 1);
        assert_eq!(
            m2,
            "{\"password\": \"[secret masked]\", \"username\": \"alice\"}"
        );

        let (m3, c3) = mask_secrets("TOKEN: abc123def456");
        assert_eq!(c3, 1);
        assert_eq!(m3, "TOKEN: [secret masked]");

        let (m4, c4) = mask_secrets("export GITHUB_TOKEN=sec_9999");
        assert_eq!(c4, 1);
        assert_eq!(m4, "export GITHUB_TOKEN=[secret masked]");

        let (m5, c5) = mask_secrets("passwd: 'my_passwd'");
        assert_eq!(c5, 1);
        assert_eq!(m5, "passwd: '[secret masked]'");

        let (m6, c6) = mask_secrets("client_secret = \"super_secret_val\"");
        assert_eq!(c6, 1);
        assert_eq!(m6, "client_secret = \"[secret masked]\"");
    }

    #[test]
    fn masks_well_known_keys() {
        // OpenAI / Anthropic
        let (m1, c1) =
            mask_secrets("Here is the key sk-ant-api03-FAKEFAKEFAKEFAKEFAKEFAKE in my file");
        assert_eq!(c1, 1);
        assert_eq!(m1, "Here is the key [secret masked] in my file");

        let (m2, c2) = mask_secrets("key=sk-proj-abc1234567890abcdef123");
        assert_eq!(c2, 1);
        assert_eq!(m2, "key=[secret masked]");

        let (m3, c3) = mask_secrets("sk-12345678901234567890abcdef12345");
        assert_eq!(c3, 1);
        assert_eq!(m3, "[secret masked]");

        // GitHub
        let (m4, c4) = mask_secrets("ghp_123456789012345678901234567890123456");
        assert_eq!(c4, 1);
        assert_eq!(m4, "[secret masked]");

        let (m5, c5) = mask_secrets("gho_123456789012345678901234567890123456");
        assert_eq!(c5, 1);
        assert_eq!(m5, "[secret masked]");

        let (m6, c6) = mask_secrets("github_pat_123456789012345678901234567890");
        assert_eq!(c6, 1);
        assert_eq!(m6, "[secret masked]");

        // Google
        let (m7, c7) = mask_secrets(&["AI", "za", "SyD-1234567890abcdef123456789012345"].concat());
        assert_eq!(c7, 1);
        assert_eq!(m7, "[secret masked]");

        // Slack
        let (m8, c8) = mask_secrets("xoxb-1234567890-123456789012-abcdef123456");
        assert_eq!(c8, 1);
        assert_eq!(m8, "[secret masked]");

        let (m9, c9) = mask_secrets("xoxp-1234567890-123456789012-abcdef123456");
        assert_eq!(c9, 1);
        assert_eq!(m9, "[secret masked]");

        // AWS
        let (m10, c10) = mask_secrets("AKIAIOSFODNN7EXAMPLE");
        assert_eq!(c10, 1);
        assert_eq!(m10, "[secret masked]");

        // JWT
        let jwt = [
            "eyJ",
            "hbGciOiJub25lIn0",
            ".",
            "eyJ",
            "zdWIiOiJmYWtlIn0",
            ".",
            "ZmFrZXNpZ25hdHVyZQ",
        ]
        .concat();
        let (m11, c11) = mask_secrets(&jwt);
        assert_eq!(c11, 1);
        assert_eq!(m11, "[secret masked]");
    }

    #[test]
    fn masks_connection_strings() {
        let (m1, c1) = mask_secrets("postgres://admin:secret123@localhost:5432/mydb");
        assert_eq!(c1, 1);
        assert_eq!(m1, "postgres://admin:[secret masked]@localhost:5432/mydb");

        let (m2, c2) = mask_secrets("mongodb://root:p@ss!@cluster0.example.com/test");
        assert_eq!(c2, 1);
        assert_eq!(
            m2,
            "mongodb://root:[secret masked]@cluster0.example.com/test"
        );

        let (m3, c3) = mask_secrets("redis://:my_secret@redis-host:6379");
        assert_eq!(c3, 1);
        assert_eq!(m3, "redis://:[secret masked]@redis-host:6379");
    }

    #[test]
    fn does_not_mask_negatives() {
        // Ordinary text
        let t1 = "Explain how to write a function in Rust.";
        let (m1, c1) = mask_secrets(t1);
        assert_eq!(c1, 0);
        assert_eq!(m1, t1);

        // Code
        let t2 = "fn login(password: &str) -> bool { let token: Token = get_token(); }";
        let (m2, c2) = mask_secrets(t2);
        assert_eq!(c2, 0);
        assert_eq!(m2, t2);

        // Git SHAs
        let t3 = "commit 3acc79c0c (HEAD -> main) and 7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c";
        let (m3, c3) = mask_secrets(t3);
        assert_eq!(c3, 0);
        assert_eq!(m3, t3);

        // UUIDs
        let t4 = "The task id is 40edc509-b42f-46f1-8d46-4122def84059.";
        let (m4, c4) = mask_secrets(t4);
        assert_eq!(c4, 0);
        assert_eq!(m4, t4);

        // URLs without credentials
        let t5 = "Visit https://api.github.com/repos/owner/repo or http://localhost:8080/v1/health";
        let (m5, c5) = mask_secrets(t5);
        assert_eq!(c5, 0);
        assert_eq!(m5, t5);

        // Words like "password" with no value
        let t6 = "What is the password? Please enter your password. password:";
        let (m6, c6) = mask_secrets(t6);
        assert_eq!(c6, 0);
        assert_eq!(m6, t6);

        let t7 = "password=";
        let (m7, c7) = mask_secrets(t7);
        assert_eq!(c7, 0);
        assert_eq!(m7, t7);
    }

    #[test]
    fn counts_multiple_secrets() {
        let text = "Here is my AWS key AKIAIOSFODNN7EXAMPLE and Slack token xoxb-1234567890-123456789012-abcdef123456 with api_key=supersecret123";
        let (masked, count) = mask_secrets(text);
        assert_eq!(count, 3);
        assert!(!masked.contains("AKIAIOSFODNN7EXAMPLE"));
        assert!(!masked.contains("xoxb-1234567890-123456789012-abcdef123456"));
        assert!(!masked.contains("supersecret123"));
        assert_eq!(
            masked,
            "Here is my AWS key [secret masked] and Slack token [secret masked] with api_key=[secret masked]"
        );
    }

    #[test]
    fn already_masked_string_is_not_recounted() {
        let (m1, c1) = mask_secrets("api_key=[secret masked]");
        assert_eq!(c1, 0);
        assert_eq!(m1, "api_key=[secret masked]");

        let (m2, c2) = mask_secrets("postgres://user:[secret masked]@localhost:5432/db");
        assert_eq!(c2, 0);
        assert_eq!(m2, "postgres://user:[secret masked]@localhost:5432/db");

        let (m3, c3) = mask_secrets("Authorization: Bearer [secret masked]");
        assert_eq!(c3, 0);
        assert_eq!(m3, "Authorization: Bearer [secret masked]");
    }

    #[test]
    fn masks_context_block_with_anthropic_key() {
        let text = "Explain this code\n\nUse this text as context:\n\"\"\"\nlet key = \"sk-ant-api03-FAKEFAKEFAKEFAKEFAKEFAKE\";\n\"\"\"";
        let (masked, count) = mask_secrets(text);
        assert_eq!(count, 1);
        assert!(!masked.contains("sk-ant-api03-FAKEFAKEFAKEFAKEFAKEFAKE"));
        assert!(masked.contains("[secret masked]"));
    }

    #[test]
    fn handles_empty_and_whitespace() {
        let (m1, c1) = mask_secrets("");
        assert_eq!(c1, 0);
        assert_eq!(m1, "");

        let (m2, c2) = mask_secrets("   \n\t  ");
        assert_eq!(c2, 0);
        assert_eq!(m2, "   \n\t  ");
    }
}
