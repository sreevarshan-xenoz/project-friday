//! A model picked per job: what you ask, questions about the screen, and the
//! steps FRIDAY plans behind the scenes (chores). `FRIDAY_MODEL_ASK`,
//! `FRIDAY_MODEL_SCREEN` and `FRIDAY_MODEL_CHORES` name a model as
//! "provider/model" (`friday model`, Settings); it's tried first and the usual
//! ranking covers it when it fails. Unset or "auto": routed as always. The
//! answer card names the model that answered.

use std::sync::Arc;

use async_trait::async_trait;
use friday_intelligence::{InferenceFabric, RequestRequirements};
use friday_investigation::LlmStrategyClient;

/// The jobs and what each covers.
pub const JOBS: [(&str, &str); 3] = [
    ("ask", "answers to what you ask, typed or spoken"),
    ("screen", "questions about what's on the screen"),
    (
        "chores",
        "steps FRIDAY plans behind the scenes: desktop, system, agents, standing tasks",
    ),
];

/// The model picked for `job`, if one is and the switch is on.
pub fn pick(job: &str) -> Option<String> {
    if !crate::features::on("model-per-job") {
        return None;
    }
    parse(std::env::var(format!("FRIDAY_MODEL_{}", job.to_uppercase())).ok())
}

fn parse(value: Option<String>) -> Option<String> {
    let v = value?.trim().to_string();
    (v.contains('/') && !v.starts_with('/') && !v.ends_with('/')).then_some(v)
}

/// LAI-3 (switch battery-model): on battery, a picked local Ollama model
/// gives way to a smaller one of the same family that is already installed
/// (qwen2.5:7b to qwen2.5:1.5b), so a laptop lasts longer. Nothing is
/// downloaded, nothing of another family is chosen, and with no smaller
/// sibling, or on mains power, the pick stands.
async fn for_power(prefer: Option<String>) -> Option<String> {
    let p = prefer?;
    let Some(model) = p.strip_prefix("ollama/") else {
        return Some(p);
    };
    if !crate::features::on("battery-model") {
        return Some(p);
    }
    match crate::standing::read_battery() {
        Some(b) if !b.charging => {}
        _ => return Some(p),
    }
    let installed = installed_ollama().await;
    Some(match smaller_sibling(model, &installed) {
        Some(small) => format!("ollama/{small}"),
        None => p,
    })
}

/// Ollama's installed models, asked at most once a minute (a goal asks the
/// model many times); none when it doesn't answer within 800 ms.
async fn installed_ollama() -> Vec<crate::local_ai::FoundModel> {
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};
    type Seen = Option<(Instant, Vec<crate::local_ai::FoundModel>)>;
    static SEEN: Mutex<Seen> = Mutex::new(None);
    static HTTP: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    if let Some((at, models)) = SEEN.lock().unwrap().as_ref() {
        if at.elapsed() < Duration::from_secs(60) {
            return models.clone();
        }
    }
    let Some(http) = HTTP
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(Duration::from_millis(800))
                .build()
                .ok()
        })
        .as_ref()
    else {
        return Vec::new();
    };
    let tags = async {
        let r = http
            .get(format!("{}/api/tags", crate::local_ai::OLLAMA_URL))
            .send()
            .await
            .ok()?;
        r.text().await.ok()
    }
    .await;
    let models = tags
        .as_deref()
        .map(crate::local_ai::parse_ollama_tags)
        .unwrap_or_default();
    *SEEN.lock().unwrap() = Some((Instant::now(), models.clone()));
    models
}

/// The largest installed model of `model`'s family at most 70% of its size;
/// `None` when it isn't installed or has no such sibling.
fn smaller_sibling(model: &str, installed: &[crate::local_ai::FoundModel]) -> Option<String> {
    // A model by its own name, whatever registry or namespace is in front
    // (library/qwen2.5:7b, hf.co/x/qwen2.5:7b, host:5000/team/qwen2.5:7b);
    // no tag is :latest. Its family is that name without the tag.
    let own = |name: &str| {
        let base = name.rsplit('/').next().unwrap_or(name).to_string();
        if base.contains(':') {
            base
        } else {
            format!("{base}:latest")
        }
    };
    let family = |name: &str| own(name).split(':').next().unwrap_or("").to_string();
    let named = |m: &&crate::local_ai::FoundModel| own(&m.name) == own(model);
    let current = installed.iter().find(named)?.size_mb;
    installed
        .iter()
        .filter(|m| family(&m.name) == family(model))
        .filter(|m| m.size_mb > 0 && m.size_mb * 10 <= current * 7)
        .max_by_key(|m| m.size_mb)
        .map(|m| m.name.clone())
}

/// The fabric, trying `job`'s model first.
pub fn for_job(fabric: &Arc<InferenceFabric>, job: &'static str) -> Arc<dyn LlmStrategyClient> {
    Arc::new(JobModel {
        fabric: fabric.clone(),
        job,
    })
}

struct JobModel {
    fabric: Arc<InferenceFabric>,
    job: &'static str,
}

#[async_trait]
impl LlmStrategyClient for JobModel {
    async fn complete(&self, prompt: &str) -> Result<String, String> {
        let prefer = for_power(pick(self.job)).await;
        self.fabric
            .complete_preferring(
                &RequestRequirements::default(),
                prefer.as_deref(),
                None,
                prompt,
                None,
            )
            .await
    }

    async fn complete_json(&self, prompt: &str) -> Result<String, String> {
        let prefer = for_power(pick(self.job)).await;
        self.fabric
            .complete_json_preferring(&RequestRequirements::default(), prefer.as_deref(), prompt)
            .await
    }

    async fn complete_with_progress(
        &self,
        prompt: &str,
        progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
    ) -> Result<String, String> {
        let prefer = for_power(pick(self.job)).await;
        self.fabric
            .complete_preferring(
                &RequestRequirements::default(),
                prefer.as_deref(),
                None,
                prompt,
                Some(progress),
            )
            .await
    }

    /// "Try a better model": the strongest one, whatever the job's pick.
    async fn complete_thorough_with_progress(
        &self,
        prompt: &str,
        avoid: Option<(&str, &str)>,
        progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
    ) -> Result<String, String> {
        self.fabric
            .complete_thorough_with_progress(prompt, avoid, progress)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pick_is_provider_slash_model_and_anything_else_routes_as_always() {
        let p = |v: &str| parse(Some(v.to_string()));
        assert_eq!(p(" groq/llama-3.3-70b "), Some("groq/llama-3.3-70b".into()));
        assert_eq!(
            p("openrouter/nvidia/nemotron:free"),
            Some("openrouter/nvidia/nemotron:free".into())
        );
        for routed in ["", "auto", "groq", "/x", "groq/"] {
            assert_eq!(p(routed), None, "{routed:?}");
        }
        assert_eq!(parse(None), None);
    }

    #[test]
    fn on_battery_a_smaller_installed_sibling_takes_over() {
        let m = |name: &str, size_mb: u64| crate::local_ai::FoundModel {
            name: name.to_string(),
            size_mb,
            vision: false,
        };
        let installed = [
            m("qwen2.5:7b", 4700),
            m("qwen2.5:3b", 1900),
            m("qwen2.5:1.5b", 986),
            m("llama3.2:1b", 1300),
            m("qwen2.5:6b", 4000),
        ];
        // The largest of its family at most 70% of its size.
        assert_eq!(
            smaller_sibling("qwen2.5:7b", &installed),
            Some("qwen2.5:3b".into())
        );
        // Already the smallest of its family: it stays.
        assert_eq!(smaller_sibling("qwen2.5:1.5b", &installed), None);
        // Never another family, never one that isn't installed.
        assert_eq!(smaller_sibling("llama3.2:1b", &installed), None);
        assert_eq!(smaller_sibling("mistral:7b", &installed), None);
        // A name without a tag is its :latest.
        let latest = [m("gemma3:latest", 3300), m("gemma3:1b", 815)];
        assert_eq!(smaller_sibling("gemma3", &latest), Some("gemma3:1b".into()));
        // A namespace or a registry with a port in front changes nothing.
        assert_eq!(
            smaller_sibling("library/qwen2.5:7b", &installed),
            Some("qwen2.5:3b".into())
        );
        let hosted = [
            m("localhost:5000/team/qwen2.5:7b", 4700),
            m("localhost:5000/team/llama3.2:1b", 1300),
        ];
        assert_eq!(
            smaller_sibling("localhost:5000/team/qwen2.5:7b", &hosted),
            None
        );
    }

    #[test]
    fn every_job_says_what_it_covers() {
        assert!(JOBS
            .iter()
            .all(|(j, what)| !j.is_empty() && !what.is_empty()));
    }
}
