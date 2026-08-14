//! High-level composition root for embedding CORDIS in CLIs, MCP servers and other hosts.

mod migration;

pub use migration::*;

use cordis_capability::CapabilityIndex;
use cordis_contracts::{
    AcceptanceCriterion, AuthorizationEnvelope, Completeness, KnownFact, Stakes,
    TASK_CONTRACT_SCHEMA, TaskContract, TaskScope, UnknownQuestion,
};
use cordis_runtime::{CordisHostRuntime, CordisWorkflowRuntime};
use cordis_socrates::{CordisGoalMode, CordisSocrates};
use cordis_store::{CordisStore, StoreError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SdkError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Runtime(#[from] cordis_runtime::RuntimeError),
    #[error(transparent)]
    Capability(#[from] cordis_capability::CapabilityError),
    #[error(transparent)]
    Contract(#[from] cordis_contracts::ContractError),
    #[error("invalid SDK request: {0}")]
    Validation(String),
}

pub type SdkResult<T> = Result<T, SdkError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskContractInput {
    pub task_id: String,
    pub goal: String,
    #[serde(default = "default_project")]
    pub project_id: String,
    #[serde(default = "default_domain")]
    pub domain: String,
    #[serde(default)]
    pub stakes: Stakes,
    #[serde(default)]
    pub stakeholders: Vec<String>,
    #[serde(default = "default_motivation")]
    pub motivation: String,
    #[serde(default)]
    pub scope_in: Vec<String>,
    #[serde(default)]
    pub scope_out: Vec<String>,
    #[serde(default)]
    pub authorization: AuthorizationEnvelope,
    #[serde(default)]
    pub constraints: Vec<String>,
    pub acceptance_evidence: Vec<AcceptanceCriterion>,
    #[serde(default)]
    pub known_facts: Vec<KnownFact>,
    #[serde(default)]
    pub unknowns: Vec<UnknownQuestion>,
    #[serde(default)]
    pub completeness: Completeness,
}

impl TaskContractInput {
    pub fn build(self) -> SdkResult<TaskContract> {
        let task = TaskContract {
            schema: TASK_CONTRACT_SCHEMA.to_owned(),
            task_id: self.task_id,
            goal: self.goal,
            project_id: self.project_id,
            domain: self.domain,
            stakes: self.stakes,
            stakeholders: self.stakeholders,
            motivation: self.motivation,
            scope: TaskScope {
                included: self.scope_in,
                excluded: self.scope_out,
            },
            authorization: self.authorization,
            constraints: self.constraints,
            acceptance_evidence: self.acceptance_evidence,
            known_facts: self.known_facts,
            unknowns: self.unknowns,
            completeness: self.completeness,
        };
        task.validate()?;
        Ok(task)
    }
}

fn default_project() -> String {
    "global".to_owned()
}

fn default_domain() -> String {
    "general".to_owned()
}

fn default_motivation() -> String {
    "Complete the requested task with observable evidence.".to_owned()
}

#[derive(Clone)]
pub struct CordisEngine {
    data_dir: PathBuf,
    store: CordisStore,
    host: CordisHostRuntime,
    workflow: CordisWorkflowRuntime,
    capability: CapabilityIndex,
    goal_mode: CordisGoalMode,
}

impl CordisEngine {
    pub fn open(data_dir: impl AsRef<Path>) -> SdkResult<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();
        fs::create_dir_all(&data_dir)?;
        let store = CordisStore::open(data_dir.join("cordis.db"))?;
        store.initialize()?;
        let host = CordisHostRuntime::new(store.clone());
        Ok(Self {
            data_dir,
            workflow: CordisWorkflowRuntime::new(host.clone()),
            capability: CapabilityIndex::new(store.clone()),
            goal_mode: CordisGoalMode::new(CordisSocrates::rule_only()),
            host,
            store,
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn store(&self) -> &CordisStore {
        &self.store
    }

    pub fn host(&self) -> &CordisHostRuntime {
        &self.host
    }

    pub fn workflow(&self) -> &CordisWorkflowRuntime {
        &self.workflow
    }

    pub fn capability(&self) -> &CapabilityIndex {
        &self.capability
    }

    pub fn goal_mode(&self) -> &CordisGoalMode {
        &self.goal_mode
    }

    pub fn status(&self) -> SdkResult<Value> {
        Ok(json!({
            "schema": "cordis.sdk-status.v1",
            "version": env!("CARGO_PKG_VERSION"),
            "data_dir": self.data_dir.display().to_string(),
            "store": self.store.counts()?,
            "runtime": self.host.status()?,
            "workflow": self.workflow.status()?,
            "capability": self.capability.status()?,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn opens_one_transactional_database() {
        let directory = tempdir().unwrap();
        let engine = CordisEngine::open(directory.path()).unwrap();
        assert!(directory.path().join("cordis.db").exists());
        assert_eq!(engine.data_dir(), directory.path());
    }
}
