use thiserror::Error;

/// Errors produced by the Skill Engine.
#[derive(Debug, Error)]
pub enum SkillError {
    #[error("skill manifest parse error in {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_json::Error,
    },

    #[error("invalid skill manifest '{id}': {reason}")]
    Invalid { id: String, reason: String },

    #[error("skill '{0}' is already registered")]
    Duplicate(String),

    #[error("skill '{0}' is not registered")]
    NotFound(String),

    #[error("skill dependency '{skill}' (required by '{by}') is missing")]
    UnresolvedDependency { skill: String, by: String },

    #[error("skill dependency cycle detected involving: {0:?}")]
    DependencyCycle(Vec<String>),

    #[error("platform '{0}' is incompatible with this host")]
    Platform(String),

    #[error("manifest requires core version {manifest}, which is newer than {core}")]
    CoreVersionTooNew { manifest: String, core: String },

    #[error("i/o error on '{path}': {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}
