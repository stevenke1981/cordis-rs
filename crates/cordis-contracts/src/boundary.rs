use crate::{
    BOUNDARY_REVIEW_SCHEMA, ControlMode, DifficultyProfile, GOAL_MODE_SCHEMA, PLAN_MODE_SCHEMA,
    PlanIr, TaskContract,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryVerdict {
    Allow,
    NeedsClarification,
    NeedsApproval,
    NeedsPlanner,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PlannerRequirement {
    NotRequired,
    Recommended,
    Required,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BoundaryRoute {
    Direct,
    Planner,
    Clarify,
    Approval,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlannerBoundary {
    pub requirement: PlannerRequirement,
    pub route: BoundaryRoute,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct BoundarySet {
    #[serde(default)]
    pub capability: Vec<String>,
    #[serde(default)]
    pub permission: Vec<String>,
    #[serde(default)]
    pub scope_in: Vec<String>,
    #[serde(default)]
    pub scope_out: Vec<String>,
    #[serde(default)]
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoundaryQuestion {
    pub id: String,
    pub question: String,
    #[serde(default = "default_true")]
    pub blocking: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BoundaryReview {
    pub schema: String,
    pub task_id: String,
    pub goal: String,
    pub verdict: BoundaryVerdict,
    pub planner: PlannerBoundary,
    pub boundaries: BoundarySet,
    #[serde(default)]
    pub questions: Vec<BoundaryQuestion>,
    pub rationale: String,
    #[serde(default)]
    pub enforced: Vec<String>,
}

impl BoundaryReview {
    pub fn new_schema() -> String {
        BOUNDARY_REVIEW_SCHEMA.to_owned()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GoalModeResult {
    pub schema: String,
    pub task_id: String,
    pub goal: String,
    pub goal_mode: bool,
    pub planner_enabled: bool,
    pub next_route: GoalNextRoute,
    pub review: BoundaryReview,
    pub model_context: String,
}

impl GoalModeResult {
    pub fn new_schema() -> String {
        GOAL_MODE_SCHEMA.to_owned()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GoalNextRoute {
    Direct,
    Plan,
    Clarify,
    Approval,
    EnablePlanner,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlannerRequest {
    pub schema: String,
    pub workflow_id: String,
    pub task: TaskContract,
    pub difficulty: DifficultyProfile,
    pub cognitive_ir: serde_json::Value,
    pub boundary_review: Option<BoundaryReview>,
    pub prior_results: Vec<serde_json::Value>,
    pub next_plan_version: u32,
    pub plan_contract: serde_json::Value,
    pub prompt: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FastRouteKind {
    Direct,
    Plan,
    Approval,
    AuthorizationRequired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FastRoute {
    pub schema: String,
    pub route: FastRouteKind,
    pub reason: String,
    pub execution_allowed: bool,
    pub planner_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanModeResult {
    pub schema: String,
    pub status: String,
    pub next_route: String,
    pub execution_allowed: bool,
    pub planner_error: Option<String>,
    pub plan: Option<PlanIr>,
    pub route: Option<FastRoute>,
}

impl PlanModeResult {
    pub fn new_schema() -> String {
        PLAN_MODE_SCHEMA.to_owned()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DifficultyInputs {
    #[serde(default = "default_mid")]
    pub complexity: f64,
    #[serde(default)]
    pub irreversibility: f64,
    #[serde(default = "default_mid")]
    pub novelty: f64,
    #[serde(default)]
    pub novelty_reason: Option<String>,
}

fn default_mid() -> f64 {
    0.5
}

impl Default for DifficultyInputs {
    fn default() -> Self {
        Self {
            complexity: 0.5,
            irreversibility: 0.0,
            novelty: 0.5,
            novelty_reason: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DifficultyPolicySnapshot {
    pub control_mode: ControlMode,
    pub task: TaskContract,
    pub difficulty: DifficultyProfile,
}
