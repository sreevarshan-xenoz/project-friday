//! Moving provider keys and a model choice over from other assistants
//! (`friday import`): OpenCode's saved keys and model, Aider's config, and
//! keys already in the environment. A preview first; applying adds only
//! what `friday.env` doesn't have yet, and never prints a key.

use std::path::{Path, PathBuf};

/// One setting found elsewhere.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// FRIDAY's name for it (`GROQ_API_KEY`, `FRIDAY_MODEL_ASK`).
    pub name: String,
    pub value: String,
    /// Where it was found ("OpenCode", "Aider", "your environment").
    pub from: &'static str,
}

/// A provider's own name in other tools, and FRIDAY's key and model prefix.
const PROVIDERS: [(&str, &str, &str); 8] = [
    ("openrouter", "OPENROUTER_API_KEY", "openrouter"),
    ("groq", "GROQ_API_KEY", "groq"),
    ("google", "GEMINI_API_KEY", "gemini"),
    ("gemini", "GEMINI_API_KEY", "gemini"),
    ("cerebras", "CEREBRAS_API_KEY", "cerebras"),
    ("opencode", "OPENCODE_API_KEY", "opencode-zen"),
    ("nvidia", "NVIDIA_NIM_API_KEY", "nvidia"),
    ("zhipuai", "ZHIPU_API_KEY", "zhipu"),
];

fn key_for(provider: &str) -> Option<&'static str> {
    let p = provider.to_lowercase();
    PROVIDERS.iter().find(|(o, ..)| *o == p).map(|(_, k, _)| *k)
}

/// `provider/model` in another tool's naming, as FRIDAY names it, when the
/// provider is one FRIDAY knows.
fn model_for(model: &str) -> Option<String> {
    let (provider, rest) = model.split_once('/')?;
    let p = provider.to_lowercase();
    let (_, _, ours) = PROVIDERS.iter().find(|(o, ..)| *o == p)?;
    (!rest.trim().is_empty()).then(|| format!("{ours}/{}", rest.trim()))
}

fn plausible(value: &str) -> bool {
    let v = value.trim();
    v.len() >= 8 && v.len() <= 400 && !v.contains(char::is_whitespace) && !v.starts_with('$')
}

/// OpenCode: `auth.json` (`{"groq": {"type": "api", "key": "…"}}`) and the
/// `model` of `opencode.json`.
pub fn from_opencode(auth: &str, config: Option<&str>) -> Vec<Found> {
    let mut out = Vec::new();
    if let Ok(serde_json::Value::Object(map)) = serde_json::from_str(auth) {
        for (provider, entry) in map {
            let (Some(name), Some(key)) = (
                key_for(&provider),
                entry.get("key").and_then(|k| k.as_str()),
            ) else {
                continue;
            };
            if entry.get("type").and_then(|t| t.as_str()).unwrap_or("api") == "api"
                && plausible(key)
            {
                out.push(Found {
                    name: name.into(),
                    value: key.trim().into(),
                    from: "OpenCode",
                });
            }
        }
    }
    if let Some(model) = config
        .and_then(|c| serde_json::from_str::<serde_json::Value>(c).ok())
        .and_then(|c| c.get("model").and_then(|m| m.as_str()).and_then(model_for))
    {
        out.push(Found {
            name: "FRIDAY_MODEL_ASK".into(),
            value: model,
            from: "OpenCode",
        });
    }
    out
}

/// Aider: `.aider.conf.yml`'s `<provider>-api-key:` lines, `api-key:` list
/// items (`- groq=…`) and `model:`.
pub fn from_aider(conf: &str) -> Vec<Found> {
    let mut out = Vec::new();
    for line in conf.lines() {
        let l = line.trim();
        let l = l.strip_prefix("- ").unwrap_or(l);
        let unquote = |v: &str| v.trim().trim_matches(['"', '\'']).to_string();
        if let Some((k, v)) = l.split_once(':') {
            let k = k.trim();
            if let Some(provider) = k.strip_suffix("-api-key") {
                if let (Some(name), v) = (key_for(provider), unquote(v)) {
                    if plausible(&v) {
                        out.push(Found {
                            name: name.into(),
                            value: v,
                            from: "Aider",
                        });
                    }
                }
                continue;
            }
            if k == "model" {
                if let Some(m) = model_for(&unquote(v)) {
                    out.push(Found {
                        name: "FRIDAY_MODEL_ASK".into(),
                        value: m,
                        from: "Aider",
                    });
                }
                continue;
            }
        }
        if let Some((provider, v)) = l.split_once('=') {
            if let (Some(name), v) = (key_for(provider.trim()), unquote(v)) {
                if plausible(&v) {
                    out.push(Found {
                        name: name.into(),
                        value: v,
                        from: "Aider",
                    });
                }
            }
        }
    }
    out
}

/// Keys already in the environment under FRIDAY's names (and `GOOGLE_API_KEY`).
pub fn from_env(vars: impl Iterator<Item = (String, String)>) -> Vec<Found> {
    let ours: Vec<&str> = PROVIDERS.iter().map(|(_, k, _)| *k).collect();
    vars.filter_map(|(k, v)| {
        let name = if k == "GOOGLE_API_KEY" {
            "GEMINI_API_KEY".to_string()
        } else {
            k
        };
        (ours.contains(&name.as_str()) && plausible(&v)).then(|| Found {
            name,
            value: v.trim().to_string(),
            from: "your environment",
        })
    })
    .collect()
}

/// Everything found on this computer, first find of each name kept.
pub fn find_all(home: &Path) -> Vec<Found> {
    let read = |p: PathBuf| std::fs::read_to_string(p).ok();
    let mut all = Vec::new();
    if let Some(auth) = read(home.join(".local/share/opencode/auth.json")) {
        all.extend(from_opencode(
            &auth,
            read(home.join(".config/opencode/opencode.json")).as_deref(),
        ));
    }
    if let Some(conf) = read(home.join(".aider.conf.yml")) {
        all.extend(from_aider(&conf));
    }
    all.extend(from_env(std::env::vars()));
    let mut seen = std::collections::HashSet::new();
    all.retain(|f| seen.insert(f.name.clone()));
    all
}

/// What `friday.env` (its text) already says for `name`, if anything.
fn current<'a>(env: &'a str, name: &str) -> Option<&'a str> {
    env.lines()
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| k.trim() == name)
        .map(|(_, v)| v.trim().trim_matches('"'))
        .next_back()
        .filter(|v| !v.is_empty())
}

/// The preview: one line per setting, keys shown by their last 4 characters.
pub fn preview(found: &[Found], env: &str) -> String {
    if found.is_empty() {
        return "Nothing to import: no OpenCode or Aider settings, and no provider keys in the environment.".into();
    }
    let mut out = String::new();
    for f in found {
        let shown = if f.name.ends_with("_API_KEY") {
            let tail: String = f
                .value
                .chars()
                .rev()
                .take(4)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("…{tail}")
        } else {
            f.value.clone()
        };
        let what = match current(env, &f.name) {
            None => "will be added",
            Some(v) if v == f.value => "already set",
            Some(_) => "kept as it is (FRIDAY has its own)",
        };
        out.push_str(&format!("{} ({shown}) from {}: {what}\n", f.name, f.from));
    }
    out
}

/// `env` with the settings it doesn't have yet added; and how many.
pub fn apply(found: &[Found], env: &str) -> (String, usize) {
    let mut text = env.to_string();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    let mut added = 0;
    for f in found.iter().filter(|f| current(env, &f.name).is_none()) {
        // A line that was there with no value is replaced.
        text = text
            .lines()
            .filter(|l| l.split_once('=').is_none_or(|(k, _)| k.trim() != f.name))
            .map(|l| format!("{l}\n"))
            .collect();
        text.push_str(&format!("{}={}\n", f.name, f.value));
        added += 1;
    }
    (text, added)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opencode_keys_and_model_are_found() {
        let auth = r#"{"groq": {"type": "api", "key": "gsk_example0123456789"},
                       "anthropic": {"type": "oauth", "refresh": "x"},
                       "openrouter": {"type": "api", "key": "sk-or-v1-example0123"}}"#;
        let found = from_opencode(auth, Some(r#"{"model": "openrouter/qwen/qwen3-coder"}"#));
        let names: Vec<&str> = found.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(
            names,
            ["GROQ_API_KEY", "OPENROUTER_API_KEY", "FRIDAY_MODEL_ASK"]
        );
        assert_eq!(found[2].value, "openrouter/qwen/qwen3-coder");
        // An unknown provider's model isn't taken.
        assert!(from_opencode("{}", Some(r#"{"model": "anthropic/claude"}"#)).is_empty());
    }

    #[test]
    fn aider_keys_and_model_are_found() {
        let conf = "model: groq/llama-3.3-70b\nopenrouter-api-key: sk-or-v1-example0123\napi-key:\n  - gemini=AIzaExample0123456789\n  - unknown=whatever123\n";
        let found = from_aider(conf);
        let got: Vec<(&str, &str)> = found
            .iter()
            .map(|f| (f.name.as_str(), f.value.as_str()))
            .collect();
        assert_eq!(
            got,
            [
                ("FRIDAY_MODEL_ASK", "groq/llama-3.3-70b"),
                ("OPENROUTER_API_KEY", "sk-or-v1-example0123"),
                ("GEMINI_API_KEY", "AIzaExample0123456789"),
            ]
        );
    }

    #[test]
    fn the_preview_hides_keys_and_applying_never_overwrites() {
        let found = vec![
            Found {
                name: "GROQ_API_KEY".into(),
                value: "gsk_example0123456789".into(),
                from: "OpenCode",
            },
            Found {
                name: "GEMINI_API_KEY".into(),
                value: "AIzaNEW0123456789".into(),
                from: "your environment",
            },
            Found {
                name: "OPENROUTER_API_KEY".into(),
                value: "sk-or-v1-example0123".into(),
                from: "Aider",
            },
        ];
        let env = "GEMINI_API_KEY=AIzaMINE0123456\nOPENROUTER_API_KEY=\nFRIDAY_PRIVATE=0\n";
        let p = preview(&found, env);
        assert!(
            p.contains("GROQ_API_KEY (…6789) from OpenCode: will be added"),
            "{p}"
        );
        assert!(
            p.contains("GEMINI_API_KEY (…6789) from your environment: kept as it is"),
            "{p}"
        );
        assert!(
            p.contains("OPENROUTER_API_KEY (…0123) from Aider: will be added"),
            "{p}"
        );
        assert!(!p.contains("gsk_example"), "{p}");
        let (after, added) = apply(&found, env);
        assert_eq!(added, 2);
        assert!(
            after.contains("GEMINI_API_KEY=AIzaMINE0123456\n"),
            "{after}"
        );
        assert!(
            after.contains("GROQ_API_KEY=gsk_example0123456789\n"),
            "{after}"
        );
        assert_eq!(after.matches("OPENROUTER_API_KEY=").count(), 1, "{after}");
        assert!(after.contains("FRIDAY_PRIVATE=0\n"));
        assert_eq!(apply(&found, &after).1, 0, "applying twice adds nothing");
    }

    #[test]
    fn only_known_keys_come_from_the_environment() {
        let vars = [
            ("GROQ_API_KEY", "gsk_example0123456789"),
            ("GOOGLE_API_KEY", "AIzaExample0123456789"),
            ("AWS_SECRET_ACCESS_KEY", "notours0123456789"),
            ("OPENROUTER_API_KEY", "short"),
        ]
        .map(|(k, v)| (k.to_string(), v.to_string()));
        let names: Vec<String> = from_env(vars.into_iter())
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(names, ["GROQ_API_KEY", "GEMINI_API_KEY"]);
    }
}
