use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use std::path::Path;
use redb::{Database, TableDefinition, ReadableTable};

use crate::error::{FridayError, Result};

const EXPERIENCE_TABLE: TableDefinition<&str, &str> = TableDefinition::new("experiences");

/// A structured task execution metrics record inside an experience.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperienceTask {
    pub id: Uuid,
    pub name: String,
    pub capability: String,
    pub success: bool,
    pub duration_ms: u64,
    pub retry_count: u32,
    pub error: Option<String>,
}

/// A stored execution experience of a plan/intent.
/// This acts as FRIDAY's procedural memory tier.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionExperience {
    pub experience_id: Uuid,
    pub intent: String,
    pub plan_id: Uuid,
    pub success: bool,
    pub actual_duration_ms: u64,
    pub predicted_duration_ms: u64,
    /// Timing drift formula: (actual - predicted) / predicted
    pub timing_drift: f64,
    pub tasks: Vec<ExperienceTask>,
    /// Decoupled JSON representation of predicted ResourceForecast
    pub resource_forecast: serde_json::Value,
    /// Decoupled JSON representation of actual resource metrics
    pub resource_actual: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

/// The Experience Engine stores successful and failed workflows, timings,
/// retries, optimization history, and simulation accuracy metrics.
pub struct ExperienceEngine {
    db: Database,
}

impl ExperienceEngine {
    /// Open or create the experience database at the specified path.
    pub fn open(path: &Path) -> Result<Self> {
        let db = Database::create(path)
            .map_err(|e| FridayError::Memory("experience".to_string(), e.to_string()))?;
        
        // Initialize the table
        let txn = db.begin_write()
            .map_err(|e| FridayError::Memory("experience".to_string(), e.to_string()))?;
        {
            let _table = txn.open_table(EXPERIENCE_TABLE)
                .map_err(|e| FridayError::Memory("experience".to_string(), e.to_string()))?;
        }
        txn.commit()
            .map_err(|e| FridayError::Memory("experience".to_string(), e.to_string()))?;

        Ok(Self { db })
    }

    /// Record a completed execution experience.
    pub fn record(&self, exp: &ExecutionExperience) -> Result<()> {
        let json = serde_json::to_string(exp)?;
        let txn = self.db.begin_write()
            .map_err(|e| FridayError::Memory("experience".to_string(), e.to_string()))?;
        {
            let mut table = txn.open_table(EXPERIENCE_TABLE)
                .map_err(|e| FridayError::Memory("experience".to_string(), e.to_string()))?;
            let key = exp.experience_id.to_string();
            table.insert(key.as_str(), json.as_str())
                .map_err(|e| FridayError::Memory("experience".to_string(), e.to_string()))?;
        }
        txn.commit()
            .map_err(|e| FridayError::Memory("experience".to_string(), e.to_string()))?;
        Ok(())
    }

    /// Retrieve an experience by its ID.
    pub fn retrieve(&self, id: Uuid) -> Result<Option<ExecutionExperience>> {
        let txn = self.db.begin_read()
            .map_err(|e| FridayError::Memory("experience".to_string(), e.to_string()))?;
        let table = txn.open_table(EXPERIENCE_TABLE)
            .map_err(|e| FridayError::Memory("experience".to_string(), e.to_string()))?;
        match table.get(id.to_string().as_str())
            .map_err(|e| FridayError::Memory("experience".to_string(), e.to_string()))?
        {
            Some(v) => {
                let exp: ExecutionExperience = serde_json::from_str(v.value())?;
                Ok(Some(exp))
            }
            None => Ok(None),
        }
    }

    /// List all recorded experiences.
    pub fn list(&self) -> Result<Vec<ExecutionExperience>> {
        let txn = self.db.begin_read()
            .map_err(|e| FridayError::Memory("experience".to_string(), e.to_string()))?;
        let table = txn.open_table(EXPERIENCE_TABLE)
            .map_err(|e| FridayError::Memory("experience".to_string(), e.to_string()))?;
        let mut out = Vec::new();
        for kv in table.iter()
            .map_err(|e| FridayError::Memory("experience".to_string(), e.to_string()))?
        {
            let (_k, v) = kv.map_err(|e| FridayError::Memory("experience".to_string(), e.to_string()))?;
            let exp: ExecutionExperience = serde_json::from_str(v.value())?;
            out.push(exp);
        }
        Ok(out)
    }

    /// Query experiences matching a specific intent keyword.
    pub fn query_by_intent(&self, intent: &str) -> Result<Vec<ExecutionExperience>> {
        let all = self.list()?;
        let query = intent.to_lowercase();
        let out = all.into_iter()
            .filter(|e| e.intent.to_lowercase().contains(&query))
            .collect();
        Ok(out)
    }
}
