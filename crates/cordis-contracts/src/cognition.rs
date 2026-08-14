use crate::{
    AuthorizationEnvelope, COGNITIVE_IR_SCHEMA, ControlMode, Evidence, FEEDBACK_RESULT_SCHEMA,
    Stakes,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Success,
    Partial,
    Failure,
}

impl Outcome {
    pub fn score(self) -> f64 {
        match self {
            Self::Success => 1.0,
            Self::Partial => 0.5,
            Self::Failure => 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum Attribution {
    World,
    Strategy,
    Capability,
    Evidence,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CoreTask {
    #[serde(default)]
    pub id: Option<String>,
    pub goal: String,
    #[serde(default = "default_domain")]
    pub domain: String,
    #[serde(default = "default_project")]
    pub project_id: String,
    #[serde(default = "default_strategy")]
    pub strategy_id: String,
    #[serde(default)]
    pub stakes: Stakes,
    #[serde(default)]
    pub authorization: AuthorizationEnvelope,
}

fn default_domain() -> String {
    "general".to_owned()
}
fn default_project() -> String {
    "global".to_owned()
}
fn default_strategy() -> String {
    "default".to_owned()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreflightRequest {
    pub task: CoreTask,
    #[serde(default = "default_complexity")]
    pub complexity: f64,
    #[serde(default)]
    pub unknowns: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub current_step: Option<String>,
    #[serde(default)]
    pub acceptance_evidence: Vec<crate::AcceptanceCriterion>,
}

fn default_complexity() -> f64 {
    0.5
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelevantEpisode {
    pub id: String,
    pub outcome: Outcome,
    pub attribution: Attribution,
    pub lesson: Option<String>,
    pub evidence_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RelevantWorldPattern {
    pub id: String,
    pub statement: String,
    pub confidence: f64,
    pub evidence_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CognitiveState {
    pub relevant_memory: Vec<RelevantEpisode>,
    pub relevant_world_patterns: Vec<RelevantWorldPattern>,
    pub capability_uncertainty: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct StrategyEvidence {
    pub uses: u64,
    pub successes: u64,
    pub failures: u64,
    pub partials: u64,
    pub calibration_error: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Prediction {
    pub expected_success_probability: f64,
    pub risk_score: f64,
    pub strategy_entropy: Option<f64>,
    pub strategy_evidence: StrategyEvidence,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum StrategyStatus {
    Available,
    AvoidUntilRevalidated,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum StrategyPromotionStatus {
    Seed,
    Active,
    Quarantined,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StrategyAdvice {
    pub id: String,
    pub status: StrategyStatus,
    pub source: String,
    pub promotion_status: StrategyPromotionStatus,
    pub prefer: Vec<String>,
    pub avoid: Vec<String>,
    pub exploration_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VerificationContract {
    pub acceptance_evidence: Vec<crate::AcceptanceCriterion>,
    pub unknowns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Escalation {
    pub advisor_required: bool,
    pub authorization_required: bool,
    pub authorization: AuthorizationEnvelope,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CognitiveIr {
    pub schema: String,
    pub task: CoreTask,
    pub state: CognitiveState,
    pub prediction: Prediction,
    pub strategy: StrategyAdvice,
    pub verification: VerificationContract,
    pub escalation: Escalation,
}

impl CognitiveIr {
    pub fn new_schema() -> String {
        COGNITIVE_IR_SCHEMA.to_owned()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeedbackRequest {
    pub task_id: String,
    pub outcome: Outcome,
    #[serde(default)]
    pub attribution: Option<Attribution>,
    #[serde(default)]
    pub lesson: Option<String>,
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub outcome_score: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeedbackEvent {
    pub id: String,
    pub at: String,
    pub task_id: String,
    pub outcome: Outcome,
    pub outcome_score: f64,
    pub expected_success_probability: f64,
    pub difference: f64,
    pub attribution: Attribution,
    pub attribution_source: String,
    pub evidence: Vec<Evidence>,
    pub lesson: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FeedbackResult {
    pub schema: String,
    pub event: FeedbackEvent,
    pub state_updates: Vec<String>,
    pub next_preflight_effect: BTreeMap<String, serde_json::Value>,
}

impl FeedbackResult {
    pub fn schema() -> String {
        FEEDBACK_RESULT_SCHEMA.to_owned()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DomainState {
    pub project_id: String,
    pub domain: String,
    pub outcomes: u64,
    pub calibration_error: f64,
    pub review_pressure: f64,
    pub world_uncertainty: f64,
    pub capability_uncertainty: f64,
}

impl DomainState {
    pub fn new(project_id: impl Into<String>, domain: impl Into<String>) -> Self {
        Self {
            project_id: project_id.into(),
            domain: domain.into(),
            outcomes: 0,
            calibration_error: 0.0,
            review_pressure: 0.0,
            world_uncertainty: 0.0,
            capability_uncertainty: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StrategySeed {
    pub source_ref: String,
    pub evidence_ids: Vec<String>,
    pub applicability: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StrategyState {
    pub project_id: String,
    pub domain: String,
    pub strategy_id: String,
    pub uses: u64,
    pub successes: u64,
    pub failures: u64,
    pub partials: u64,
    pub calibration_error: f64,
    pub source: String,
    pub promotion_status: StrategyPromotionStatus,
    #[serde(default)]
    pub prefer: Vec<String>,
    #[serde(default)]
    pub avoid: Vec<String>,
    pub seed: Option<StrategySeed>,
}

impl StrategyState {
    pub fn new(
        project_id: impl Into<String>,
        domain: impl Into<String>,
        strategy_id: impl Into<String>,
    ) -> Self {
        Self {
            project_id: project_id.into(),
            domain: domain.into(),
            strategy_id: strategy_id.into(),
            uses: 0,
            successes: 0,
            failures: 0,
            partials: 0,
            calibration_error: 0.0,
            source: "default".to_owned(),
            promotion_status: StrategyPromotionStatus::Active,
            prefer: vec![],
            avoid: vec![],
            seed: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExplorationPolicy {
    pub mode: ExplorationMode,
    pub reason: String,
    pub requirements: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExplorationMode {
    Explore,
    Exploit,
    Revalidate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RuntimeControl {
    pub mode: ControlMode,
    pub advisor_required: bool,
    pub execution_allowed: bool,
    pub authorization_required: bool,
    pub permit_id: String,
    pub denial_reasons: Vec<String>,
}
