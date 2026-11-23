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
        let prefer = pick(self.job);
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
        let prefer = pick(self.job);
        self.fabric
            .complete_json_preferring(&RequestRequirements::default(), prefer.as_deref(), prompt)
            .await
    }

    async fn complete_with_progress(
        &self,
        prompt: &str,
        progress: &(dyn for<'s> Fn(&'s str) + Send + Sync),
    ) -> Result<String, String> {
        let prefer = pick(self.job);
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
    fn every_job_says_what_it_covers() {
        assert!(JOBS
            .iter()
            .all(|(j, what)| !j.is_empty() && !what.is_empty()));
    }
}
