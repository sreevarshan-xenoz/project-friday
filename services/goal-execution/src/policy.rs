use crate::Capability;
use serde::{Deserialize, Serialize};

// ── Policy ──────────────────────────────────────────────────────────

/// A policy — an immutable, evidence-backed record that informs Planning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub id: String,
    pub kind: PolicyKind,
    pub statement: String,
    pub capability: Option<Capability>,
    pub config: serde_json::Value,
    pub confidence: f32,
    pub support: u32,
    pub created_from: Vec<String>,
    pub expires_at: Option<u64>,
    pub created_at_ms: u64,
}

impl Policy {
    pub fn prefer_tool(
        capability: Capability,
        preferred_tool: impl Into<String>,
        confidence: f32,
        support: u32,
    ) -> Self {
        let tool_name = preferred_tool.into();
        Self {
            id: format!("policy_{}", uuid::Uuid::new_v4()),
            kind: PolicyKind::PreferTool,
            statement: format!("Prefer {} for {}", tool_name, capability),
            capability: Some(capability),
            config: serde_json::json!({ "preferred_tool": tool_name }),
            confidence,
            support,
            created_from: Vec::new(),
            expires_at: None,
            created_at_ms: chrono::Utc::now().timestamp_millis() as u64,
        }
    }

    pub fn retry(capability: Capability, max_retries: u32, confidence: f32, support: u32) -> Self {
        Self {
            id: format!("policy_{}", uuid::Uuid::new_v4()),
            kind: PolicyKind::Retry,
            statement: format!("Retry {} up to {} times", capability, max_retries),
            capability: Some(capability),
            config: serde_json::json!({ "max_retries": max_retries }),
            confidence,
            support,
            created_from: Vec::new(),
            expires_at: None,
            created_at_ms: chrono::Utc::now().timestamp_millis() as u64,
        }
    }

    pub fn sequence(from: Capability, then: Capability, confidence: f32, support: u32) -> Self {
        Self {
            id: format!("policy_{}", uuid::Uuid::new_v4()),
            kind: PolicyKind::Sequence,
            statement: format!("{} usually follows {}", then, from),
            capability: Some(from),
            config: serde_json::json!({ "then": then }),
            confidence,
            support,
            created_from: Vec::new(),
            expires_at: None,
            created_at_ms: chrono::Utc::now().timestamp_millis() as u64,
        }
    }

    pub fn is_expired(&self, now_ms: u64) -> bool {
        match self.expires_at {
            Some(exp) => now_ms > exp,
            None => false,
        }
    }

    pub fn with_created_from(mut self, insight_ids: Vec<String>) -> Self {
        self.created_from = insight_ids;
        self
    }

    pub fn with_expires_at(mut self, expires_at_ms: u64) -> Self {
        self.expires_at = Some(expires_at_ms);
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PolicyKind {
    PreferTool,
    Retry,
    Sequence,
    Skip,
    Custom(String),
}
