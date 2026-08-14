//! Goal-mode boundary review. A model may propose boundaries but can never relax hard gates.

use cordis_contracts::{
    BOUNDARY_REVIEW_SCHEMA, BoundaryQuestion, BoundaryReview, BoundaryRoute, BoundarySet,
    BoundaryVerdict, DifficultyProfile, GOAL_MODE_SCHEMA, GoalModeResult, GoalNextRoute,
    PlannerRequirement, TaskContract,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::Arc;
use thiserror::Error;

pub const SOCRATES_REQUEST_SCHEMA: &str = "cordis.socrates-request.v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SocratesRequest {
    pub schema: String,
    pub task: TaskContract,
    pub difficulty: DifficultyProfile,
    pub cognitive_ir: Value,
    pub planner_enabled: bool,
    pub required_output_schema: String,
    pub prompt: String,
}

#[derive(Debug, Error)]
pub enum SocratesError {
    #[error("invalid Socrates request: {0}")]
    InvalidRequest(String),
    #[error("Socrates callable failed: {0}")]
    Model(String),
    #[error("Socrates returned invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Socrates contract validation failed: {0}")]
    Contract(#[from] cordis_contracts::ContractError),
}

pub type SocratesResult<T> = Result<T, SocratesError>;
pub type SocratesCallable = dyn Fn(&SocratesRequest) -> Result<Value, String> + Send + Sync;

#[derive(Clone)]
pub struct CordisSocrates {
    model: Arc<SocratesCallable>,
}

impl CordisSocrates {
    pub fn new<F>(model: F) -> Self
    where
        F: Fn(&SocratesRequest) -> Result<Value, String> + Send + Sync + 'static,
    {
        Self {
            model: Arc::new(model),
        }
    }

    /// A no-model reviewer useful for deterministic local hosts and tests.
    pub fn rule_only() -> Self {
        Self::new(|request| {
            let (verdict, requirement, route, reason) =
                minimum_gate(&request.task, &request.difficulty);
            Ok(json!({
                "schema": BOUNDARY_REVIEW_SCHEMA,
                "task_id": request.task.task_id.clone(),
                "goal": request.task.goal.clone(),
                "verdict": verdict,
                "planner": {"requirement": requirement, "route": route, "reason": reason},
                "boundaries": default_boundaries(&request.task),
                "questions": request.task.unknowns.iter().filter(|item| item.material).enumerate().map(|(index, item)| json!({
                    "id": format!("unknown-{}", index + 1), "question": item.question.clone(), "blocking": true
                })).collect::<Vec<_>>(),
                "rationale": "Deterministic CORDIS boundary policy.",
                "enforced": ["goal", "scope", "capability", "permission", "evidence", "planner_mode"]
            }))
        })
    }

    pub fn request(
        task: &TaskContract,
        difficulty: &DifficultyProfile,
        cognitive_ir: Value,
        planner_enabled: bool,
    ) -> SocratesResult<SocratesRequest> {
        task.validate()?;
        difficulty.validate()?;
        let context = json!({
            "task": task,
            "difficulty": difficulty,
            "cognitive_ir": cognitive_ir,
            "mode": {"goal_mode": true, "planner_enabled": planner_enabled},
        });
        Ok(SocratesRequest {
            schema: SOCRATES_REQUEST_SCHEMA.to_owned(),
            task: task.clone(),
            difficulty: difficulty.clone(),
            cognitive_ir: context["cognitive_ir"].clone(),
            planner_enabled,
            required_output_schema: BOUNDARY_REVIEW_SCHEMA.to_owned(),
            prompt: format!(
                "Return one JSON object only for a CORDIS boundary review. Do not execute tools, infer authorization, grant permissions or claim evidence. Preserve task_id and goal exactly. Review capability, permission, scope, evidence and whether the separate planner is required.\n{}",
                serde_json::to_string(&context)?
            ),
        })
    }

    pub fn review(
        &self,
        task: &TaskContract,
        difficulty: &DifficultyProfile,
        cognitive_ir: Value,
        planner_enabled: bool,
    ) -> SocratesResult<BoundaryReview> {
        let request = Self::request(task, difficulty, cognitive_ir, planner_enabled)?;
        let raw = (self.model)(&request).map_err(SocratesError::Model)?;
        let mut review: BoundaryReview = serde_json::from_value(raw)?;
        validate_review(&review, &request.task)?;

        let (minimum_verdict, minimum_requirement, minimum_route, minimum_reason) =
            minimum_gate(&request.task, &request.difficulty);
        if verdict_rank(minimum_verdict) > verdict_rank(review.verdict) {
            review.verdict = minimum_verdict;
            review.planner.reason = minimum_reason;
        }
        if requirement_rank(minimum_requirement) > requirement_rank(review.planner.requirement) {
            review.planner.requirement = minimum_requirement;
        }

        review.planner.route = match review.verdict {
            BoundaryVerdict::NeedsClarification => BoundaryRoute::Clarify,
            BoundaryVerdict::NeedsApproval => BoundaryRoute::Approval,
            BoundaryVerdict::NeedsPlanner => BoundaryRoute::Planner,
            BoundaryVerdict::Allow
                if review.planner.requirement == PlannerRequirement::Required =>
            {
                if planner_enabled {
                    BoundaryRoute::Planner
                } else {
                    review.verdict = BoundaryVerdict::NeedsPlanner;
                    BoundaryRoute::Planner
                }
            }
            BoundaryVerdict::Allow => {
                if review.planner.requirement == PlannerRequirement::Recommended && planner_enabled
                {
                    BoundaryRoute::Planner
                } else {
                    minimum_route
                }
            }
        };

        merge_boundaries(&mut review.boundaries, default_boundaries(&request.task));
        let existing_questions: Vec<_> = review
            .questions
            .iter()
            .map(|item| item.question.clone())
            .collect();
        for (index, unknown) in request
            .task
            .unknowns
            .iter()
            .filter(|item| item.material)
            .enumerate()
        {
            if !existing_questions.contains(&unknown.question) {
                review.questions.push(BoundaryQuestion {
                    id: format!("material-unknown-{}", index + 1),
                    question: unknown.question.clone(),
                    blocking: true,
                });
            }
        }
        for item in [
            "goal",
            "scope",
            "capability",
            "permission",
            "evidence",
            "planner_mode",
        ] {
            if !review.enforced.iter().any(|existing| existing == item) {
                review.enforced.push(item.to_owned());
            }
        }
        Ok(review)
    }
}

#[derive(Clone)]
pub struct CordisGoalMode {
    reviewer: CordisSocrates,
}

impl CordisGoalMode {
    pub fn new(reviewer: CordisSocrates) -> Self {
        Self { reviewer }
    }

    pub fn begin(
        &self,
        task: &TaskContract,
        difficulty: &DifficultyProfile,
        cognitive_ir: Value,
        planner_enabled: bool,
    ) -> SocratesResult<GoalModeResult> {
        let review = self
            .reviewer
            .review(task, difficulty, cognitive_ir, planner_enabled)?;
        let next_route = match review.verdict {
            BoundaryVerdict::NeedsClarification => GoalNextRoute::Clarify,
            BoundaryVerdict::NeedsApproval => GoalNextRoute::Approval,
            BoundaryVerdict::NeedsPlanner => GoalNextRoute::EnablePlanner,
            BoundaryVerdict::Allow
                if planner_enabled || review.planner.route == BoundaryRoute::Planner =>
            {
                GoalNextRoute::Plan
            }
            BoundaryVerdict::Allow => GoalNextRoute::Direct,
        };
        Ok(GoalModeResult {
            schema: GOAL_MODE_SCHEMA.to_owned(),
            task_id: review.task_id.clone(),
            goal: review.goal.clone(),
            goal_mode: true,
            planner_enabled,
            next_route,
            model_context: model_context(&review),
            review,
        })
    }
}

fn validate_review(review: &BoundaryReview, task: &TaskContract) -> SocratesResult<()> {
    if review.schema != BOUNDARY_REVIEW_SCHEMA {
        return Err(SocratesError::InvalidRequest(
            "boundary review has an unsupported schema".to_owned(),
        ));
    }
    if review.task_id != task.task_id || review.goal != task.goal {
        return Err(SocratesError::InvalidRequest(
            "boundary review must preserve task_id and goal".to_owned(),
        ));
    }
    if review.planner.reason.trim().is_empty() || review.rationale.trim().is_empty() {
        return Err(SocratesError::InvalidRequest(
            "planner reason and rationale must be non-empty".to_owned(),
        ));
    }
    Ok(())
}

fn minimum_gate(
    task: &TaskContract,
    difficulty: &DifficultyProfile,
) -> (BoundaryVerdict, PlannerRequirement, BoundaryRoute, String) {
    let components = &difficulty.components;
    if task.unknowns.iter().any(|item| item.material) {
        (
            BoundaryVerdict::NeedsClarification,
            PlannerRequirement::Recommended,
            BoundaryRoute::Clarify,
            "material unknowns must be clarified before execution".to_owned(),
        )
    } else if task.stakes == cordis_contracts::Stakes::Critical
        || components.impact.score >= 0.8
        || components.irreversibility.score >= 0.8
    {
        (
            BoundaryVerdict::NeedsApproval,
            PlannerRequirement::Required,
            BoundaryRoute::Approval,
            "critical or irreversible work requires human approval".to_owned(),
        )
    } else if components.complexity.score >= 0.75
        || components.uncertainty.score >= 0.75
        || components.novelty.score >= 0.75
    {
        (
            BoundaryVerdict::Allow,
            PlannerRequirement::Required,
            BoundaryRoute::Planner,
            "complex, uncertain or novel work requires Plan Mode".to_owned(),
        )
    } else {
        (
            BoundaryVerdict::Allow,
            PlannerRequirement::NotRequired,
            BoundaryRoute::Direct,
            "bounded task may use the direct route".to_owned(),
        )
    }
}

fn default_boundaries(task: &TaskContract) -> BoundarySet {
    BoundarySet {
        capability: Vec::new(),
        permission: vec![
            format!(
                "authorization={:?}; network={:?}",
                task.authorization.status, task.authorization.network_profile
            )
            .to_lowercase(),
        ],
        scope_in: task.scope.included.clone(),
        scope_out: task.scope.excluded.clone(),
        evidence: task
            .acceptance_evidence
            .iter()
            .map(|criterion| format!("{}: {}", criterion.id, criterion.description))
            .collect(),
    }
}

fn merge_boundaries(target: &mut BoundarySet, defaults: BoundarySet) {
    merge(&mut target.capability, defaults.capability);
    merge(&mut target.permission, defaults.permission);
    merge(&mut target.scope_in, defaults.scope_in);
    merge(&mut target.scope_out, defaults.scope_out);
    merge(&mut target.evidence, defaults.evidence);
}

fn merge(target: &mut Vec<String>, values: Vec<String>) {
    for value in values {
        if !target.contains(&value) {
            target.push(value);
        }
    }
}

fn verdict_rank(value: BoundaryVerdict) -> u8 {
    match value {
        BoundaryVerdict::Allow => 0,
        BoundaryVerdict::NeedsPlanner => 1,
        BoundaryVerdict::NeedsApproval => 2,
        BoundaryVerdict::NeedsClarification => 3,
    }
}

fn requirement_rank(value: PlannerRequirement) -> u8 {
    match value {
        PlannerRequirement::NotRequired => 0,
        PlannerRequirement::Recommended => 1,
        PlannerRequirement::Required => 2,
    }
}

fn model_context(review: &BoundaryReview) -> String {
    let boundaries = &review.boundaries;
    let mut lines = vec![
        "[CORDIS GOAL CONTEXT]".to_owned(),
        format!("Goal: {}", review.goal),
        format!("Boundary verdict: {:?}", review.verdict).to_lowercase(),
        format!(
            "Planner: {:?} ({:?})",
            review.planner.requirement, review.planner.route
        )
        .to_lowercase(),
        format!("Scope in: {}", value_or_none(&boundaries.scope_in)),
        format!("Scope out: {}", value_or_none(&boundaries.scope_out)),
        format!("Permissions: {}", value_or_none(&boundaries.permission)),
        format!(
            "Capability limits: {}",
            value_or_none(&boundaries.capability)
        ),
        format!("Evidence required: {}", value_or_none(&boundaries.evidence)),
    ];
    let blocking: Vec<_> = review
        .questions
        .iter()
        .filter(|question| question.blocking)
        .map(|question| question.question.clone())
        .collect();
    if !blocking.is_empty() {
        lines.push(format!("Blocking questions: {}", blocking.join(" | ")));
    }
    lines.push(
        "Do not execute outside these boundaries, infer authorization or claim unobserved evidence."
            .to_owned(),
    );
    lines.join("\n")
}

fn value_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "none declared".to_owned()
    } else {
        values.join(" | ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cordis_contracts::{
        AcceptanceCriterion, AuthorizationEnvelope, Completeness, ControlMode,
        DIFFICULTY_PROFILE_SCHEMA, DifficultyComponent, DifficultyComponents, KnownFact, Stakes,
        TASK_CONTRACT_SCHEMA, TaskScope,
    };

    fn task() -> TaskContract {
        TaskContract {
            schema: TASK_CONTRACT_SCHEMA.to_owned(),
            task_id: "goal-task".to_owned(),
            goal: "Review a goal".to_owned(),
            project_id: "cordis".to_owned(),
            domain: "software".to_owned(),
            stakes: Stakes::Low,
            stakeholders: vec![],
            motivation: "test".to_owned(),
            scope: TaskScope {
                included: vec!["src".to_owned()],
                excluded: vec!["production".to_owned()],
            },
            authorization: AuthorizationEnvelope::default(),
            constraints: vec![],
            acceptance_evidence: vec![AcceptanceCriterion {
                id: "verified".to_owned(),
                description: "verified".to_owned(),
                required: true,
            }],
            known_facts: vec![KnownFact {
                claim: "fixture".to_owned(),
                evidence_ids: vec!["fixture".to_owned()],
            }],
            unknowns: vec![],
            completeness: Completeness::default(),
        }
    }

    fn difficulty(complexity: f64) -> DifficultyProfile {
        let component = |score| DifficultyComponent {
            score,
            reasons: vec!["test".to_owned()],
        };
        DifficultyProfile {
            schema: DIFFICULTY_PROFILE_SCHEMA.to_owned(),
            components: DifficultyComponents {
                complexity: component(complexity),
                uncertainty: component(0.1),
                impact: component(0.2),
                irreversibility: component(0.0),
                novelty: component(0.0),
                evidence_deficit: component(0.1),
            },
            control_mode: ControlMode::Fast,
            override_reason: None,
            aggregate_score: Some(complexity),
            policy_reasons: vec!["test".to_owned()],
        }
    }

    #[test]
    fn rule_only_review_preserves_scope_and_evidence() {
        let review = CordisSocrates::rule_only()
            .review(&task(), &difficulty(0.1), json!({}), false)
            .unwrap();
        assert_eq!(review.verdict, BoundaryVerdict::Allow);
        assert!(review.boundaries.scope_in.contains(&"src".to_owned()));
        assert!(
            review
                .boundaries
                .evidence
                .iter()
                .any(|item| item.contains("verified"))
        );
    }

    #[test]
    fn model_cannot_relax_high_complexity_planner_gate() {
        let reviewer = CordisSocrates::new(|request| {
            Ok(json!({
                "schema": BOUNDARY_REVIEW_SCHEMA,
                "task_id": request.task.task_id.clone(),
                "goal": request.task.goal.clone(),
                "verdict": "allow",
                "planner": {"requirement": "not_required", "route": "direct", "reason": "model says direct"},
                "boundaries": {}, "questions": [], "rationale": "model proposal", "enforced": []
            }))
        });
        let review = reviewer
            .review(&task(), &difficulty(0.9), json!({}), false)
            .unwrap();
        assert_eq!(review.verdict, BoundaryVerdict::NeedsPlanner);
        assert_eq!(review.planner.requirement, PlannerRequirement::Required);
    }
}
