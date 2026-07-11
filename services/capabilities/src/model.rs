//! Rich capability model. One `Capability` describes a single thing a *service*
//! can do. Multiple services may advertise the same `id` (different providers);
//! the registry ranks them.

use serde::{Deserialize, Serialize};

/// Target platform a capability requires. `Any` means cross-platform.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Platform {
    Windows,
    Linux,
    MacOS,
    Android,
    IOS,
    Cloud,
    Any,
}

impl Platform {
    /// The platform this FRIDAY process is running on.
    pub fn current() -> Platform {
        if cfg!(windows) {
            Platform::Windows
        } else if cfg!(target_os = "linux") {
            Platform::Linux
        } else if cfg!(target_os = "macos") {
            Platform::MacOS
        } else {
            Platform::Any
        }
    }

    pub fn is_compatible(&self, other: Platform) -> bool {
        matches!(self, Platform::Any) || matches!(other, Platform::Any) || *self == other
    }
}

/// Live health of a provider capability. Drives availability + self-healing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Health {
    Healthy,
    Busy,
    Offline,
    Recovering,
    Disabled,
}

impl Health {
    /// A capability is usable unless it is Offline or Disabled.
    pub fn available(&self) -> bool {
        !matches!(self, Health::Offline | Health::Disabled)
    }
}

/// A single capability advertised by a service (a "provider").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    /// Fully-qualified id, e.g. `filesystem.create_file`.
    pub id: String,
    pub name: String,
    pub description: String,
    /// Graph grouping, e.g. `Files`, `Windows`, `Browser`.
    pub group: String,
    /// Verbs the provider handles under this capability.
    pub actions: Vec<String>,
    /// Permission strings the provider needs.
    pub permissions: Vec<String>,
    pub platform: Platform,
    /// Typical latency in milliseconds (for ranking).
    pub latency_ms: u64,
    /// Historical success rate, 0..1.
    pub reliability: f64,
    pub version: String,
    pub dependencies: Vec<String>,
    pub resource_requirements: Vec<String>,
    pub supports_parallel: bool,
    pub dry_run: bool,
    pub available: bool,
    pub health: Health,
    /// Arbitrary cost units (compute / quota).
    pub cost: f64,
    /// Registry confidence in this provider, 0..1.
    pub confidence: f64,
}

impl Capability {
    /// Create with sensible defaults; override fields as needed.
    pub fn new(id: impl Into<String>, name: impl Into<String>, group: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            group: group.into(),
            actions: Vec::new(),
            permissions: Vec::new(),
            platform: Platform::Any,
            latency_ms: 0,
            reliability: 1.0,
            version: "1.0".into(),
            dependencies: Vec::new(),
            resource_requirements: Vec::new(),
            supports_parallel: true,
            dry_run: false,
            available: true,
            health: Health::Healthy,
            cost: 0.0,
            confidence: 1.0,
        }
    }

    pub fn with_actions(mut self, actions: &[&str]) -> Self {
        self.actions = actions.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn with_description(mut self, d: impl Into<String>) -> Self {
        self.description = d.into();
        self
    }

    pub fn with_platform(mut self, p: Platform) -> Self {
        self.platform = p;
        self
    }

    pub fn with_latency(mut self, ms: u64) -> Self {
        self.latency_ms = ms;
        self
    }

    pub fn with_reliability(mut self, r: f64) -> Self {
        self.reliability = r.clamp(0.0, 1.0);
        self
    }

    pub fn with_version(mut self, v: impl Into<String>) -> Self {
        self.version = v.into();
        self
    }

    pub fn with_dependencies(mut self, deps: &[&str]) -> Self {
        self.dependencies = deps.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn with_resources(mut self, r: &[&str]) -> Self {
        self.resource_requirements = r.iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn with_cost(mut self, c: f64) -> Self {
        self.cost = c;
        self
    }

    pub fn with_confidence(mut self, c: f64) -> Self {
        self.confidence = c.clamp(0.0, 1.0);
        self
    }

    pub fn with_dry_run(mut self, b: bool) -> Self {
        self.dry_run = b;
        self
    }

    pub fn mark_unavailable(mut self) -> Self {
        self.available = false;
        self.health = Health::Offline;
        self
    }
}
