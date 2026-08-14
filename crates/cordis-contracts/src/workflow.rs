use crate::{
    ContractError, ContractResult, PLAN_SCHEMA, STEP_RESULT_SCHEMA, validate_text, validate_texts,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ActionClass {
    Read,
    Research,
    Change,
    Verify,
}

impl ActionClass {
    pub fn as_policy_name(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Research => "research",
            Self::Change => "change",
            Self::Verify => "verify",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Success,
    Partial,
    Failure,
    Blocked,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum FailureRoute {
    Replan,
    Block,
    Finish,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MethodPolicy {
    pub recommended: String,
    #[serde(default)]
    pub alternatives: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolPolicy {
    #[serde(default)]
    pub recommended: Vec<String>,
    pub allowed: Vec<String>,
    #[serde(default)]
    pub forbidden: Vec<String>,
}

impl ToolPolicy {
    pub fn validate(&self) -> ContractResult<()> {
        validate_texts(&self.recommended, "tool_policy.recommended", 100, 200)?;
        validate_texts(&self.allowed, "tool_policy.allowed", 100, 200)?;
        validate_texts(&self.forbidden, "tool_policy.forbidden", 100, 200)?;
        let allowed: HashSet<_> = self
            .allowed
            .iter()
            .map(|item| item.to_lowercase())
            .collect();
        let forbidden: HashSet<_> = self
            .forbidden
            .iter()
            .map(|item| item.to_lowercase())
            .collect();
        for item in &self.recommended {
            if !allowed.contains(&item.to_lowercase()) {
                return Err(ContractError::NotSubset {
                    field: "tool_policy.recommended",
                    other: "tool_policy.allowed",
                    value: item.clone(),
                });
            }
        }
        for item in &self.allowed {
            if forbidden.contains(&item.to_lowercase()) {
                return Err(ContractError::Overlap {
                    field: "tool_policy.allowed",
                    other: "tool_policy.forbidden",
                    value: item.clone(),
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ModelRequirements {
    #[serde(default)]
    pub reasoning: f64,
    #[serde(default)]
    pub coding: f64,
    #[serde(default)]
    pub tool_use: f64,
    #[serde(default)]
    pub context_capacity: f64,
    #[serde(default)]
    pub review: f64,
}

impl ModelRequirements {
    pub fn validate(&self) -> ContractResult<()> {
        for (name, score) in [
            ("reasoning", self.reasoning),
            ("coding", self.coding),
            ("tool_use", self.tool_use),
            ("context_capacity", self.context_capacity),
            ("review", self.review),
        ] {
            if !(0.0..=1.0).contains(&score) || score.is_nan() {
                return Err(ContractError::Inconsistent {
                    field: "model_requirements",
                    reason: format!("{name} must be between 0 and 1"),
                });
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanStep {
    pub id: String,
    pub objective: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    pub allowed_scope: Vec<String>,
    pub action_class: ActionClass,
    pub method: MethodPolicy,
    pub tool_policy: ToolPolicy,
    #[serde(default)]
    pub model_requirements: ModelRequirements,
    pub required_evidence: Vec<String>,
    pub approval_required: bool,
    pub retry_limit: u8,
    #[serde(default)]
    pub stop_when: Vec<String>,
    pub on_success: String,
    pub on_failure: FailureRoute,
}

impl PlanStep {
    pub fn validate(&self) -> ContractResult<()> {
        validate_text(&self.id, "steps.id", 200)?;
        validate_text(&self.objective, "steps.objective", 4_000)?;
        validate_texts(&self.depends_on, "steps.depends_on", 100, 200)?;
        validate_texts(&self.allowed_scope, "steps.allowed_scope", 100, 500)?;
        validate_text(&self.method.recommended, "steps.method.recommended", 200)?;
        validate_texts(
            &self.method.alternatives,
            "steps.method.alternatives",
            100,
            200,
        )?;
        self.tool_policy.validate()?;
        self.model_requirements.validate()?;
        if self.required_evidence.is_empty() {
            return Err(ContractError::Missing {
                field: "steps.required_evidence",
            });
        }
        validate_texts(
            &self.required_evidence,
            "steps.required_evidence",
            100,
            1_000,
        )?;
        if self.retry_limit > 5 {
            return Err(ContractError::Inconsistent {
                field: "steps.retry_limit",
                reason: "retry_limit must be between 0 and 5".to_owned(),
            });
        }
        validate_texts(&self.stop_when, "steps.stop_when", 100, 1_000)?;
        validate_text(&self.on_success, "steps.on_success", 200)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PlanAssumption {
    pub claim: String,
    pub confidence_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlanIr {
    pub schema: String,
    pub plan_id: String,
    pub version: u32,
    pub task_id: String,
    pub workflow_id: String,
    pub goal: String,
    #[serde(default)]
    pub assumptions: Vec<PlanAssumption>,
    #[serde(default)]
    pub alternatives_considered: Vec<String>,
    pub steps: Vec<PlanStep>,
    pub completion_gate: Vec<String>,
}

impl PlanIr {
    pub fn validate(&self) -> ContractResult<()> {
        if self.schema != PLAN_SCHEMA {
            return Err(ContractError::Unsupported {
                field: "plan.schema",
                value: self.schema.clone(),
            });
        }
        validate_text(&self.plan_id, "plan_id", 200)?;
        if self.version == 0 {
            return Err(ContractError::Inconsistent {
                field: "version",
                reason: "version must be positive".to_owned(),
            });
        }
        validate_text(&self.task_id, "task_id", 200)?;
        validate_text(&self.workflow_id, "workflow_id", 200)?;
        validate_text(&self.goal, "goal", 4_000)?;
        if self.steps.is_empty() {
            return Err(ContractError::Missing { field: "steps" });
        }
        validate_texts(
            &self.alternatives_considered,
            "alternatives_considered",
            100,
            1_000,
        )?;
        if self.completion_gate.is_empty() {
            return Err(ContractError::Missing {
                field: "completion_gate",
            });
        }
        validate_texts(&self.completion_gate, "completion_gate", 100, 1_000)?;
        for assumption in &self.assumptions {
            validate_text(&assumption.claim, "assumptions.claim", 4_000)?;
            validate_text(&assumption.confidence_id, "assumptions.confidence_id", 200)?;
        }
        let mut ids = HashSet::new();
        for step in &self.steps {
            step.validate()?;
            if !ids.insert(step.id.as_str()) {
                return Err(ContractError::Duplicate {
                    field: "steps.id",
                    value: step.id.clone(),
                });
            }
        }
        let known: HashSet<_> = self.steps.iter().map(|step| step.id.as_str()).collect();
        for step in &self.steps {
            if step
                .depends_on
                .iter()
                .any(|dependency| dependency == &step.id)
            {
                return Err(ContractError::Inconsistent {
                    field: "steps.depends_on",
                    reason: format!("{} may not depend on itself", step.id),
                });
            }
            for dependency in &step.depends_on {
                if !known.contains(dependency.as_str()) {
                    return Err(ContractError::UnknownReference {
                        field: "steps.depends_on",
                        value: dependency.clone(),
                    });
                }
            }
            if step.on_success != "finish"
                && step.on_success != "replan"
                && !known.contains(step.on_success.as_str())
            {
                return Err(ContractError::UnknownReference {
                    field: "steps.on_success",
                    value: step.on_success.clone(),
                });
            }
            if step.on_success == step.id {
                return Err(ContractError::Inconsistent {
                    field: "steps.on_success",
                    reason: format!("{} may not transition to itself", step.id),
                });
            }
        }
        self.validate_dag()
    }

    fn validate_dag(&self) -> ContractResult<()> {
        let by_id: HashMap<_, _> = self
            .steps
            .iter()
            .map(|step| (step.id.as_str(), step))
            .collect();
        let mut visiting = HashSet::new();
        let mut visited = HashSet::new();
        for step in &self.steps {
            visit(step.id.as_str(), &by_id, &mut visiting, &mut visited)?;
        }
        Ok(())
    }
}

fn visit<'a>(
    step_id: &'a str,
    by_id: &HashMap<&'a str, &'a PlanStep>,
    visiting: &mut HashSet<&'a str>,
    visited: &mut HashSet<&'a str>,
) -> ContractResult<()> {
    if visited.contains(step_id) {
        return Ok(());
    }
    if !visiting.insert(step_id) {
        return Err(ContractError::Cycle {
            field: "steps.depends_on",
        });
    }
    if let Some(step) = by_id.get(step_id) {
        for dependency in &step.depends_on {
            visit(dependency.as_str(), by_id, visiting, visited)?;
        }
    }
    visiting.remove(step_id);
    visited.insert(step_id);
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Evidence {
    #[serde(default)]
    pub id: Option<String>,
    pub kind: String,
    pub summary: String,
    pub passed: bool,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub acceptance_id: Option<String>,
    #[serde(default)]
    pub source_id: Option<String>,
    #[serde(default)]
    pub trust: EvidenceTrust,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceTrust {
    Untrusted,
    #[default]
    Observed,
    Reviewed,
}

impl Evidence {
    pub fn validate(&self) -> ContractResult<()> {
        validate_text(&self.kind, "evidence.kind", 100)?;
        validate_text(&self.summary, "evidence.summary", 4_000)?;
        if let Some(acceptance_id) = &self.acceptance_id {
            validate_text(acceptance_id, "evidence.acceptance_id", 200)?;
        }
        if let Some(source_id) = &self.source_id {
            validate_text(source_id, "evidence.source_id", 500)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepError {
    pub kind: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StepResult {
    pub schema: String,
    pub task_id: String,
    pub plan_id: String,
    pub plan_version: u32,
    pub step_id: String,
    pub status: StepStatus,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub observations: Vec<String>,
    #[serde(default)]
    pub artifacts: Vec<String>,
    pub evidence: Vec<Evidence>,
    #[serde(default)]
    pub errors: Vec<StepError>,
    #[serde(default)]
    pub proposed_next: Option<String>,
}

impl StepResult {
    pub fn validate(&self) -> ContractResult<()> {
        if self.schema != STEP_RESULT_SCHEMA {
            return Err(ContractError::Unsupported {
                field: "step_result.schema",
                value: self.schema.clone(),
            });
        }
        validate_text(&self.task_id, "task_id", 200)?;
        validate_text(&self.plan_id, "plan_id", 200)?;
        if self.plan_version == 0 {
            return Err(ContractError::Inconsistent {
                field: "plan_version",
                reason: "plan_version must be positive".to_owned(),
            });
        }
        validate_text(&self.step_id, "step_id", 200)?;
        validate_texts(&self.actions, "actions", 100, 1_000)?;
        validate_texts(&self.observations, "observations", 100, 1_000)?;
        validate_texts(&self.artifacts, "artifacts", 100, 2_000)?;
        if self.evidence.is_empty() {
            return Err(ContractError::Missing { field: "evidence" });
        }
        for evidence in &self.evidence {
            evidence.validate()?;
        }
        for error in &self.errors {
            validate_text(&error.kind, "errors.kind", 100)?;
            validate_text(&error.summary, "errors.summary", 4_000)?;
        }
        if let Some(proposed_next) = &self.proposed_next {
            validate_text(proposed_next, "proposed_next", 200)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatus {
    AwaitingAuthorization,
    AwaitingPlan,
    AwaitingApproval,
    Active,
    AwaitingReplan,
    Finished,
    Failed,
    Blocked,
    Closed,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(id: &str, depends_on: Vec<&str>, on_success: &str) -> PlanStep {
        PlanStep {
            id: id.to_owned(),
            objective: format!("run {id}"),
            depends_on: depends_on.into_iter().map(str::to_owned).collect(),
            allowed_scope: vec!["core".to_owned()],
            action_class: ActionClass::Verify,
            method: MethodPolicy {
                recommended: "test".to_owned(),
                alternatives: vec![],
            },
            tool_policy: ToolPolicy {
                recommended: vec!["cargo".to_owned()],
                allowed: vec!["cargo".to_owned()],
                forbidden: vec![],
            },
            model_requirements: ModelRequirements::default(),
            required_evidence: vec![format!("{id} passes")],
            approval_required: false,
            retry_limit: 0,
            stop_when: vec![],
            on_success: on_success.to_owned(),
            on_failure: FailureRoute::Block,
        }
    }

    #[test]
    fn sequential_plan_is_valid() {
        let plan = PlanIr {
            schema: PLAN_SCHEMA.to_owned(),
            plan_id: "p1".to_owned(),
            version: 1,
            task_id: "t1".to_owned(),
            workflow_id: "workflow:t1".to_owned(),
            goal: "test".to_owned(),
            assumptions: vec![],
            alternatives_considered: vec![],
            steps: vec![step("S1", vec![], "S2"), step("S2", vec!["S1"], "finish")],
            completion_gate: vec!["tests pass".to_owned()],
        };
        plan.validate().unwrap();
    }

    #[test]
    fn dependency_cycle_is_rejected() {
        let plan = PlanIr {
            schema: PLAN_SCHEMA.to_owned(),
            plan_id: "p1".to_owned(),
            version: 1,
            task_id: "t1".to_owned(),
            workflow_id: "workflow:t1".to_owned(),
            goal: "test".to_owned(),
            assumptions: vec![],
            alternatives_considered: vec![],
            steps: vec![
                step("S1", vec!["S2"], "S2"),
                step("S2", vec!["S1"], "finish"),
            ],
            completion_gate: vec!["tests pass".to_owned()],
        };
        assert!(matches!(plan.validate(), Err(ContractError::Cycle { .. })));
    }
}
