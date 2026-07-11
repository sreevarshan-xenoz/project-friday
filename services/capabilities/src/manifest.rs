//! Manifest-based service discovery. Services ship a `manifest.json`; at startup
//! FRIDAY scans `plugins/`, validates each, and registers the capabilities.
//! Adding or removing a capability becomes data, not code.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::model::{Capability, Platform};
use crate::registry::CapabilityRegistry;

/// On-disk service manifest. Mirrors what a service publishes at runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceManifest {
    pub service: String,
    pub version: String,
    /// String form of [`Platform`] (e.g. `Windows`).
    pub platform: String,
    pub capabilities: Vec<Capability>,
}

impl ServiceManifest {
    /// Validate required fields and parse the platform.
    pub fn validate(&self) -> Result<(), String> {
        if self.service.trim().is_empty() {
            return Err("service name required".into());
        }
        if self.capabilities.is_empty() {
            return Err(format!(
                "{}: manifest declares no capabilities",
                self.service
            ));
        }
        for c in &self.capabilities {
            if c.id.trim().is_empty() {
                return Err(format!("{}: capability missing id", self.service));
            }
            if c.actions.is_empty() {
                return Err(format!("{}: {} has no actions", self.service, c.id));
            }
        }
        let _ = self.platform()?;
        Ok(())
    }

    /// Parse the platform string into a [`Platform`].
    pub fn platform(&self) -> Result<Platform, String> {
        match self.platform.as_str() {
            "Windows" => Ok(Platform::Windows),
            "Linux" => Ok(Platform::Linux),
            "MacOS" => Ok(Platform::MacOS),
            "Android" => Ok(Platform::Android),
            "Cloud" => Ok(Platform::Cloud),
            "Any" => Ok(Platform::Any),
            other => Err(format!("unknown platform '{other}'")),
        }
    }

    /// Scan `root/*/manifest.json`, parse and validate each. Missing dir → empty.
    pub fn load_dir(root: &Path) -> Result<Vec<ServiceManifest>, String> {
        let mut out = Vec::new();
        if !root.exists() {
            return Ok(out);
        }
        for entry in std::fs::read_dir(root).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                let mf = path.join("manifest.json");
                if mf.exists() {
                    let text = std::fs::read_to_string(&mf).map_err(|e| e.to_string())?;
                    let m: ServiceManifest = serde_json::from_str(&text)
                        .map_err(|e| format!("{}: {}", mf.display(), e))?;
                    m.validate()?;
                    out.push(m);
                }
            }
        }
        Ok(out)
    }

    /// Register this manifest's capabilities under its service name.
    pub fn register_into(&self, registry: &CapabilityRegistry) {
        registry.register_provider(&self.service, self.capabilities.clone());
    }
}
