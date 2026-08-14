use crate::{Attribution, EvidenceTrust, MEMORY_SCHEMA, Outcome};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(
    Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash, Default,
)]
#[serde(rename_all = "snake_case")]
pub enum MemoryScope {
    Conversation,
    #[default]
    Workflow,
    Project,
    Global,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Event,
    Episode,
    Knowledge,
    Pattern,
    Capability,
    Principle,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    #[default]
    Active,
    Candidate,
    Retired,
    Quarantined,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTrust {
    #[default]
    Untrusted,
    Observed,
    Reviewed,
}

impl From<EvidenceTrust> for MemoryTrust {
    fn from(value: EvidenceTrust) -> Self {
        match value {
            EvidenceTrust::Untrusted => Self::Untrusted,
            EvidenceTrust::Observed => Self::Observed,
            EvidenceTrust::Reviewed => Self::Reviewed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryItem {
    pub id: String,
    pub scope: MemoryScope,
    pub project_id: Option<String>,
    pub conversation_id: Option<String>,
    pub task_id: Option<String>,
    pub kind: MemoryKind,
    pub subject: String,
    pub content: String,
    pub confidence: f64,
    pub source_count: usize,
    pub status: MemoryStatus,
    pub trust: MemoryTrust,
    pub instruction_safe: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_verified_at: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub relevance: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RememberRequest {
    pub kind: MemoryKind,
    pub subject: String,
    pub content: String,
    pub scope: MemoryScope,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub conversation_id: Option<String>,
    #[serde(default)]
    pub task_id: Option<String>,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub evidence: BTreeMap<String, serde_json::Value>,
    #[serde(default = "default_confidence")]
    pub confidence: f64,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
    #[serde(default)]
    pub status: MemoryStatus,
    #[serde(default)]
    pub trust: MemoryTrust,
    #[serde(default)]
    pub instruction_safe: bool,
}

fn default_confidence() -> f64 {
    0.5
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryQuery {
    pub intent: String,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default = "default_scopes")]
    pub scopes: Vec<MemoryScope>,
    #[serde(default = "default_kinds")]
    pub kinds: Vec<MemoryKind>,
    #[serde(default)]
    pub exclude_ids: Vec<String>,
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub include_untrusted: bool,
}

fn default_scopes() -> Vec<MemoryScope> {
    vec![MemoryScope::Project, MemoryScope::Global]
}

fn default_kinds() -> Vec<MemoryKind> {
    vec![
        MemoryKind::Episode,
        MemoryKind::Knowledge,
        MemoryKind::Pattern,
        MemoryKind::Capability,
        MemoryKind::Principle,
    ]
}

fn default_limit() -> usize {
    3
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryQueryResult {
    pub schema: String,
    pub items: Vec<MemoryItem>,
    pub status: String,
    pub excluded_ids: Vec<String>,
}

impl MemoryQueryResult {
    pub fn new(items: Vec<MemoryItem>, excluded_ids: Vec<String>) -> Self {
        Self {
            schema: MEMORY_SCHEMA.to_owned(),
            status: if items.is_empty() {
                "no_novel_cognition".to_owned()
            } else {
                "ok".to_owned()
            },
            items,
            excluded_ids,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventRecord {
    pub id: Option<String>,
    pub event_type: String,
    #[serde(default = "default_workflow_scope")]
    pub scope: MemoryScope,
    pub project_id: String,
    pub task_id: String,
    #[serde(default)]
    pub conversation_id: Option<String>,
    pub subject: String,
    pub actual: String,
    #[serde(default)]
    pub expected: Option<String>,
    #[serde(default)]
    pub error_class: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub plan_id: Option<String>,
    #[serde(default)]
    pub step_id: Option<String>,
    #[serde(default)]
    pub trust: MemoryTrust,
}

fn default_workflow_scope() -> MemoryScope {
    MemoryScope::Workflow
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EpisodeRecord {
    pub id: String,
    pub at: String,
    pub goal: String,
    pub domain: String,
    pub project_id: String,
    pub strategy_id: String,
    pub outcome: Outcome,
    pub attribution: Attribution,
    pub lesson: Option<String>,
    pub evidence_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorldPatternRecord {
    pub id: String,
    pub project_id: String,
    pub domain: String,
    pub statement: String,
    pub evidence_count: usize,
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphNode {
    pub id: String,
    pub node_type: String,
    pub scope: MemoryScope,
    pub project_id: Option<String>,
    pub label: String,
    #[serde(default)]
    pub metadata: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphEdge {
    pub id: String,
    pub from_id: String,
    pub relation: String,
    pub to_id: String,
    pub confidence: f64,
    pub source_id: Option<String>,
}
