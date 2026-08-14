//! Provider-neutral planning boundary. Planner output is a proposal, never execution authority.

use cordis_contracts::{
    BoundaryReview, DifficultyProfile, FastRoute, FastRouteKind, PLAN_MODE_SCHEMA, PLAN_SCHEMA,
    PlanIr, PlanModeResult, PlannerRequest, PlannerRequirement, TaskContract,
};
use serde_json::{Value, json};
use std::sync::Arc;
use thiserror::Error;

pub const PLANNER_REQUEST_SCHEMA: &str = "cordis.planner-request.v1";
pub const FAST_ROUTE_SCHEMA: &str = "cordis.fast-route.v1";

#[derive(Debug, Error)]
pub enum PlannerError {
    #[error("invalid planner request: {0}")]
    InvalidRequest(String),
    #[error("planner callable failed: {0}")]
    Model(String),
    #[error("planner returned invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("planner returned invalid PlanIR: {0}")]
    Contract(#[from] cordis_contracts::ContractError),
}

pub type PlannerResult<T> = Result<T, PlannerError>;
pub type PlannerCallable = dyn Fn(&PlannerRequest) -> Result<Value, String> + Send + Sync;

#[derive(Clone)]
pub struct CordisPlanner {
    model: Arc<PlannerCallable>,
}

impl CordisPlanner {
    pub fn new<F>(model: F) -> Self
    where
        F: Fn(&PlannerRequest) -> Result<Value, String> + Send + Sync + 'static,
    {
        Self {
            model: Arc::new(model),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn request(
        workflow_id: &str,
        task: &TaskContract,
        difficulty: &DifficultyProfile,
        cognitive_ir: Value,
        boundary_review: Option<BoundaryReview>,
        prior_results: Vec<Value>,
        next_plan_version: u32,
    ) -> PlannerResult<PlannerRequest> {
        task.validate()?;
        difficulty.validate()?;
        if workflow_id.trim().is_empty() {
            return Err(PlannerError::InvalidRequest(
                "workflow_id must be non-empty".to_owned(),
            ));
        }
        if next_plan_version == 0 {
            return Err(PlannerError::InvalidRequest(
                "next_plan_version must be positive".to_owned(),
            ));
        }
        if let Some(review) = &boundary_review {
            validate_boundary(review, task)?;
        }
        let context = json!({
            "task": task,
            "difficulty": difficulty,
            "cognitive_ir": cognitive_ir,
            "boundary_review": boundary_review,
            "prior_results": prior_results.iter().rev().take(6).cloned().collect::<Vec<_>>(),
            "required_plan_schema": PLAN_SCHEMA,
            "next_plan_version": next_plan_version,
        });
        Ok(PlannerRequest {
            schema: PLANNER_REQUEST_SCHEMA.to_owned(),
            workflow_id: workflow_id.trim().to_owned(),
            task: task.clone(),
            difficulty: difficulty.clone(),
            cognitive_ir: context["cognitive_ir"].clone(),
            boundary_review,
            prior_results: prior_results
                .into_iter()
                .rev()
                .take(20)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect(),
            next_plan_version,
            plan_contract: json!({
                "schema": PLAN_SCHEMA,
                "required": ["plan_id", "version", "task_id", "workflow_id", "goal", "steps", "completion_gate"],
                "constraints": [
                    "Return one JSON object only; never execute tools.",
                    "Preserve task_id, workflow_id and goal exactly.",
                    "Keep every allowed_scope inside task.scope.in and outside task.scope.out.",
                    "Respect authorization allowed/denied actions, tools, targets and network profile.",
                    "Require approval on every high-risk change step.",
                    "Use one sequential success path; this runtime has no parallel scheduler.",
                    "Provide explicit observable required_evidence for every step.",
                    "Never claim evidence that has not been observed."
                ]
            }),
            prompt: format!(
                "Return one JSON object only: a valid cordis.plan.v1 proposal. You are a planner, not an executor. Do not claim tools were run or evidence exists. Respect task scope, authorization, control mode, boundary review and all explicit constraints. Use one sequential success path.\n{}",
                serde_json::to_string(&context)?
            ),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn propose(
        &self,
        workflow_id: &str,
        task: &TaskContract,
        difficulty: &DifficultyProfile,
        cognitive_ir: Value,
        boundary_review: Option<BoundaryReview>,
        prior_results: Vec<Value>,
        next_plan_version: u32,
    ) -> PlannerResult<PlanIr> {
        let request = Self::request(
            workflow_id,
            task,
            difficulty,
            cognitive_ir,
            boundary_review,
            prior_results,
            next_plan_version,
        )?;
        let raw = (self.model)(&request).map_err(PlannerError::Model)?;
        let plan: PlanIr = serde_json::from_value(raw)?;
        plan.validate()?;
        if plan.workflow_id != request.workflow_id {
            return Err(PlannerError::InvalidRequest(
                "planner returned a plan for another workflow".to_owned(),
            ));
        }
        if plan.task_id != request.task.task_id {
            return Err(PlannerError::InvalidRequest(
                "planner returned a plan for another task".to_owned(),
            ));
        }
        if plan.goal != request.task.goal {
            return Err(PlannerError::InvalidRequest(
                "planner changed the task goal".to_owned(),
            ));
        }
        if plan.version != request.next_plan_version {
            return Err(PlannerError::InvalidRequest(
                "planner returned an unexpected plan version".to_owned(),
            ));
        }
        Ok(plan)
    }

    pub fn fast_route(
        task: &TaskContract,
        difficulty: &DifficultyProfile,
        boundary_review: Option<&BoundaryReview>,
    ) -> PlannerResult<FastRoute> {
        task.validate()?;
        difficulty.validate()?;
        if let Some(review) = boundary_review {
            validate_boundary(review, task)?;
        }
        let components = &difficulty.components;
        let material_unknowns = task.unknowns.iter().any(|item| item.material);
        let high_impact = components.impact.score >= 0.8 || components.irreversibility.score >= 0.8;
        let high_complexity = components
            .complexity
            .score
            .max(components.uncertainty.score)
            .max(components.novelty.score)
            >= 0.75;

        let route = if task.authorization.status != cordis_contracts::AuthorizationStatus::Granted
            && (matches!(
                task.stakes,
                cordis_contracts::Stakes::High | cordis_contracts::Stakes::Critical
            ) || high_impact)
        {
            FastRoute {
                schema: FAST_ROUTE_SCHEMA.to_owned(),
                route: FastRouteKind::AuthorizationRequired,
                reason: "authorization is not granted for high-impact work".to_owned(),
                execution_allowed: false,
                planner_required: false,
            }
        } else if task.stakes == cordis_contracts::Stakes::Critical || high_impact {
            FastRoute {
                schema: FAST_ROUTE_SCHEMA.to_owned(),
                route: FastRouteKind::Approval,
                reason: "critical or irreversible work requires human approval".to_owned(),
                execution_allowed: false,
                planner_required: true,
            }
        } else if high_complexity {
            FastRoute {
                schema: FAST_ROUTE_SCHEMA.to_owned(),
                route: FastRouteKind::Plan,
                reason: "complex, uncertain or novel work requires a validated plan".to_owned(),
                execution_allowed: false,
                planner_required: true,
            }
        } else if components.complexity.score <= 0.3
            && matches!(
                task.stakes,
                cordis_contracts::Stakes::Low | cordis_contracts::Stakes::Medium
            )
            && !material_unknowns
        {
            FastRoute {
                schema: FAST_ROUTE_SCHEMA.to_owned(),
                route: FastRouteKind::Direct,
                reason: "low complexity, bounded scope and no material unknowns".to_owned(),
                execution_allowed: true,
                planner_required: false,
            }
        } else {
            FastRoute {
                schema: FAST_ROUTE_SCHEMA.to_owned(),
                route: FastRouteKind::Plan,
                reason: "default plan route for work that is not trivially direct".to_owned(),
                execution_allowed: false,
                planner_required: true,
            }
        };
        Ok(route)
    }
}

#[derive(Clone)]
pub struct CordisPlanMode {
    planner: CordisPlanner,
}

impl CordisPlanMode {
    pub fn new(planner: CordisPlanner) -> Self {
        Self { planner }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn propose(
        &self,
        workflow_id: &str,
        task: &TaskContract,
        difficulty: &DifficultyProfile,
        cognitive_ir: Value,
        boundary_review: Option<BoundaryReview>,
        prior_results: Vec<Value>,
        next_plan_version: u32,
    ) -> PlanModeResult {
        let may_fall_back = boundary_review
            .as_ref()
            .is_some_and(|review| review.planner.requirement != PlannerRequirement::Required);
        match self.planner.propose(
            workflow_id,
            task,
            difficulty,
            cognitive_ir,
            boundary_review,
            prior_results,
            next_plan_version,
        ) {
            Ok(plan) => PlanModeResult {
                schema: PLAN_MODE_SCHEMA.to_owned(),
                status: "plan_ready".to_owned(),
                next_route: "submit_plan".to_owned(),
                execution_allowed: false,
                planner_error: None,
                plan: Some(plan),
                route: None,
            },
            Err(error) => PlanModeResult {
                schema: PLAN_MODE_SCHEMA.to_owned(),
                status: "planner_failed".to_owned(),
                next_route: if may_fall_back {
                    "direct"
                } else {
                    "repair_planner"
                }
                .to_owned(),
                execution_allowed: may_fall_back,
                planner_error: Some(error.to_string()),
                plan: None,
                route: None,
            },
        }
    }

    pub fn fast(
        &self,
        task: &TaskContract,
        difficulty: &DifficultyProfile,
        boundary_review: Option<&BoundaryReview>,
    ) -> PlanModeResult {
        match CordisPlanner::fast_route(task, difficulty, boundary_review) {
            Ok(route) => {
                let (status, next_route) = match route.route {
                    FastRouteKind::Direct => ("fast_route", "direct"),
                    FastRouteKind::AuthorizationRequired => ("blocked", "authorization_required"),
                    FastRouteKind::Approval => ("approval_required", "approval"),
                    FastRouteKind::Plan => ("planner_required", "plan"),
                };
                PlanModeResult {
                    schema: PLAN_MODE_SCHEMA.to_owned(),
                    status: status.to_owned(),
                    next_route: next_route.to_owned(),
                    execution_allowed: route.execution_allowed,
                    planner_error: (!route.execution_allowed).then(|| route.reason.clone()),
                    plan: None,
                    route: Some(route),
                }
            }
            Err(error) => PlanModeResult {
                schema: PLAN_MODE_SCHEMA.to_owned(),
                status: "routing_failed".to_owned(),
                next_route: "repair_task".to_owned(),
                execution_allowed: false,
                planner_error: Some(error.to_string()),
                plan: None,
                route: None,
            },
        }
    }
}

fn validate_boundary(review: &BoundaryReview, task: &TaskContract) -> PlannerResult<()> {
    if review.schema != cordis_contracts::BOUNDARY_REVIEW_SCHEMA {
        return Err(PlannerError::InvalidRequest(
            "boundary_review has an unsupported schema".to_owned(),
        ));
    }
    if review.task_id != task.task_id || review.goal != task.goal {
        return Err(PlannerError::InvalidRequest(
            "boundary_review must preserve task_id and goal".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cordis_contracts::{
        AcceptanceCriterion, AuthorizationEnvelope, AuthorizationStatus, Completeness,
        DIFFICULTY_PROFILE_SCHEMA, DifficultyComponent, DifficultyComponents, KnownFact,
        NetworkProfile, Stakes, TASK_CONTRACT_SCHEMA, TaskScope,
    };

    fn task() -> TaskContract {
        let authorization = AuthorizationEnvelope {
            status: AuthorizationStatus::Granted,
            basis: "owner approval".to_owned(),
            network_profile: NetworkProfile::Offline,
            ..Default::default()
        };
        TaskContract {
            schema: TASK_CONTRACT_SCHEMA.to_owned(),
            task_id: "planner-task".to_owned(),
            goal: "Validate a planner proposal".to_owned(),
            project_id: "cordis".to_owned(),
            domain: "software".to_owned(),
            stakes: Stakes::Low,
            stakeholders: vec![],
            motivation: "test".to_owned(),
            scope: TaskScope {
                included: vec!["crates/cordis-planner".to_owned()],
                excluded: vec![],
            },
            authorization,
            constraints: vec![],
            acceptance_evidence: vec![AcceptanceCriterion {
                id: "test".to_owned(),
                description: "planner test passes".to_owned(),
                required: true,
            }],
            known_facts: vec![KnownFact {
                claim: "test model is available".to_owned(),
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
            control_mode: cordis_contracts::ControlMode::Fast,
            override_reason: None,
            aggregate_score: Some(complexity),
            policy_reasons: vec!["test".to_owned()],
        }
    }

    #[test]
    fn fast_route_direct_for_simple_authorized_work() {
        let route = CordisPlanner::fast_route(&task(), &difficulty(0.1), None).unwrap();
        assert_eq!(route.route, FastRouteKind::Direct);
        assert!(route.execution_allowed);
    }

    #[test]
    fn rejects_plan_for_other_task() {
        let planner = CordisPlanner::new(|request| {
            Ok(json!({
                "schema": "cordis.plan.v1", "plan_id": "p", "version": 1,
                "task_id": "other", "workflow_id": request.workflow_id.clone(), "goal": request.task.goal.clone(),
                "assumptions": [], "alternatives_considered": [], "steps": [], "completion_gate": ["x"]
            }))
        });
        assert!(
            planner
                .propose(
                    "workflow:planner-task",
                    &task(),
                    &difficulty(0.1),
                    json!({}),
                    None,
                    vec![],
                    1
                )
                .is_err()
        );
    }
}
