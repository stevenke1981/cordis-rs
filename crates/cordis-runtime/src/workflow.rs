use crate::{CordisHostRuntime, RuntimeError, RuntimeResult, assess_difficulty};
use cordis_contracts::{
    ActionClass, AuthorizationEnvelope, AuthorizationStatus, ControlMode, DifficultyInputs,
    DifficultyProfile, EventRecord, Evidence, EvidenceTrust, FailureRoute, FeedbackRequest,
    FeedbackResult, MemoryScope, MemoryTrust, Outcome, PlanIr, PlanStep, PreflightRequest,
    StepResult, StepStatus, TaskContract, WORKFLOW_RUNTIME_SCHEMA, WorkflowStatus,
};
use cordis_policy::{ActionProposal, ExecutionPermit, PolicyContext, PolicyEngine};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowBeginRequest {
    pub task: TaskContract,
    #[serde(default)]
    pub difficulty: DifficultyInputs,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApprovalRecord {
    pub step_id: String,
    #[serde(default)]
    pub plan_id: String,
    #[serde(default)]
    pub plan_version: u32,
    pub approved_by: String,
    pub at: String,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct WorkflowRecord {
    workflow_id: String,
    task_id: String,
    created_at: String,
    updated_at: String,
    status: WorkflowStatus,
    task: TaskContract,
    difficulty: DifficultyProfile,
    plan: Option<PlanIr>,
    current_step_id: Option<String>,
    approvals: Vec<ApprovalRecord>,
    results: BTreeMap<String, Vec<StepResult>>,
    terminal_feedback: Option<FeedbackResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowSnapshot {
    pub schema: String,
    pub workflow_id: String,
    pub task_id: String,
    pub status: WorkflowStatus,
    pub control_mode: ControlMode,
    pub task: TaskContract,
    pub difficulty: DifficultyProfile,
    pub plan: Option<PlanIr>,
    pub current_step: Option<PlanStep>,
    pub eligible_step_ids: Vec<String>,
    pub result_count: usize,
    pub approvals: Vec<ApprovalRecord>,
    pub terminal_feedback: Option<FeedbackResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowFinishRequest {
    #[serde(default)]
    pub outcome: Option<Outcome>,
    #[serde(default)]
    pub lesson: Option<String>,
    #[serde(default)]
    pub attribution: Option<cordis_contracts::Attribution>,
}

#[derive(Clone)]
pub struct CordisWorkflowRuntime {
    host: CordisHostRuntime,
    policy: PolicyEngine,
}

impl CordisWorkflowRuntime {
    pub fn new(host: CordisHostRuntime) -> Self {
        Self {
            host,
            policy: PolicyEngine,
        }
    }

    pub fn begin(&self, request: WorkflowBeginRequest) -> RuntimeResult<WorkflowSnapshot> {
        request.task.validate()?;
        let workflow_id = workflow_id(&request.task.task_id);
        if self.load_optional(&workflow_id)?.is_some() {
            return Err(RuntimeError::Transition(
                "task_id already has a workflow; task IDs are immutable".to_owned(),
            ));
        }
        let difficulty = assess_difficulty(&request.task, &request.difficulty)?;
        let runtime = self.host.begin(PreflightRequest {
            task: cordis_contracts::CoreTask {
                id: Some(request.task.task_id.clone()),
                goal: request.task.goal.clone(),
                domain: request.task.domain.clone(),
                project_id: request.task.project_id.clone(),
                strategy_id: "workflow_default".to_owned(),
                stakes: request.task.stakes,
                authorization: request.task.authorization.clone(),
            },
            complexity: request.difficulty.complexity,
            unknowns: request
                .task
                .unknowns
                .iter()
                .map(|item| item.question.clone())
                .collect(),
            constraints: request.task.constraints.clone(),
            current_step: Some(request.task.goal.clone()),
            acceptance_evidence: request.task.acceptance_evidence.clone(),
        })?;
        if runtime.task_id != request.task.task_id {
            return Err(RuntimeError::Transition(
                "host runtime did not preserve workflow task_id".to_owned(),
            ));
        }
        self.host.update_scope(
            &request.task.task_id,
            request.task.scope.included.clone(),
            request.task.scope.excluded.clone(),
        )?;
        let status = if request.task.authorization.status == AuthorizationStatus::Denied {
            WorkflowStatus::Blocked
        } else if runtime.control.authorization_required && !runtime.control.execution_allowed {
            WorkflowStatus::AwaitingAuthorization
        } else {
            WorkflowStatus::AwaitingPlan
        };
        let now = cordis_contracts::now_rfc3339();
        let record = WorkflowRecord {
            workflow_id: workflow_id.clone(),
            task_id: request.task.task_id.clone(),
            created_at: now.clone(),
            updated_at: now,
            status,
            task: request.task,
            difficulty,
            plan: None,
            current_step_id: None,
            approvals: Vec::new(),
            results: BTreeMap::new(),
            terminal_feedback: None,
        };
        self.save(&record)?;
        self.host
            .store()
            .audit("workflow_begin", Some(&record.task_id), &record)?;
        self.snapshot(&record)
    }

    pub fn set_authorization(
        &self,
        workflow_id: &str,
        authorization: AuthorizationEnvelope,
    ) -> RuntimeResult<WorkflowSnapshot> {
        authorization.validate()?;
        let mut record = self.load(workflow_id)?;
        if !matches!(
            record.status,
            WorkflowStatus::AwaitingAuthorization | WorkflowStatus::AwaitingPlan
        ) {
            return Err(RuntimeError::Transition(
                "authorization may only change before a plan is activated".to_owned(),
            ));
        }
        record.task.authorization = authorization.clone();
        self.host
            .update_authorization(&record.task_id, authorization.clone())?;
        record.status = match authorization.status {
            AuthorizationStatus::Granted => WorkflowStatus::AwaitingPlan,
            AuthorizationStatus::Pending => WorkflowStatus::AwaitingAuthorization,
            AuthorizationStatus::Denied => WorkflowStatus::Blocked,
        };
        record.updated_at = cordis_contracts::now_rfc3339();
        self.save(&record)?;
        self.host.store().audit(
            "workflow_authorization_updated",
            Some(&record.task_id),
            &authorization,
        )?;
        self.snapshot(&record)
    }

    pub fn submit_plan(&self, workflow_id: &str, plan: PlanIr) -> RuntimeResult<WorkflowSnapshot> {
        plan.validate()?;
        let mut record = self.load(workflow_id)?;
        if record.status != WorkflowStatus::AwaitingPlan {
            return Err(RuntimeError::Transition(
                "a plan may only be submitted while awaiting_plan".to_owned(),
            ));
        }
        self.validate_plan_against_task(&record, &plan)?;
        let root = plan
            .steps
            .iter()
            .find(|step| step.depends_on.is_empty())
            .ok_or_else(|| RuntimeError::Validation("plan has no root step".to_owned()))?;
        record.current_step_id = Some(root.id.clone());
        record.status = status_for_step(root);
        record.plan = Some(plan.clone());
        record.updated_at = cordis_contracts::now_rfc3339();
        self.host.observe(
            &record.task_id,
            EventRecord {
                id: None,
                event_type: "plan_submitted".to_owned(),
                scope: MemoryScope::Workflow,
                project_id: record.task.project_id.clone(),
                task_id: record.task_id.clone(),
                conversation_id: None,
                subject: plan.goal.clone(),
                actual: format!("Plan with {} step(s) accepted.", plan.steps.len()),
                expected: Some("scope-safe, policy-safe, dependency-valid PlanIR".to_owned()),
                error_class: None,
                tool: None,
                model: None,
                environment: None,
                uri: None,
                plan_id: Some(plan.plan_id.clone()),
                step_id: None,
                trust: MemoryTrust::Observed,
            },
        )?;
        self.save(&record)?;
        self.snapshot(&record)
    }

    pub fn approve_step(
        &self,
        workflow_id: &str,
        step_id: &str,
        approved_by: &str,
        reason: Option<String>,
    ) -> RuntimeResult<WorkflowSnapshot> {
        if approved_by.trim().is_empty() {
            return Err(RuntimeError::Validation(
                "approved_by must be non-empty".to_owned(),
            ));
        }
        let mut record = self.load(workflow_id)?;
        if record.status != WorkflowStatus::AwaitingApproval
            || record.current_step_id.as_deref() != Some(step_id)
        {
            return Err(RuntimeError::Transition(
                "only the current approval-gated step may be approved".to_owned(),
            ));
        }
        let step = plan_step(&record, step_id)?.clone();
        if !step.approval_required {
            return Err(RuntimeError::Transition(
                "current step does not require approval".to_owned(),
            ));
        }
        let (active_plan_id, active_plan_version) = {
            let active_plan = record.plan.as_ref().ok_or_else(|| {
                RuntimeError::Transition("workflow has no active plan".to_owned())
            })?;
            (active_plan.plan_id.clone(), active_plan.version)
        };
        record.approvals.push(ApprovalRecord {
            step_id: step_id.to_owned(),
            plan_id: active_plan_id,
            plan_version: active_plan_version,
            approved_by: approved_by.trim().to_owned(),
            at: cordis_contracts::now_rfc3339(),
            reason: reason.map(|value| value.trim().to_owned()),
        });
        record.status = WorkflowStatus::Active;
        record.updated_at = cordis_contracts::now_rfc3339();
        self.host.observe(
            &record.task_id,
            EventRecord {
                id: None,
                event_type: "step_approved".to_owned(),
                scope: MemoryScope::Workflow,
                project_id: record.task.project_id.clone(),
                task_id: record.task_id.clone(),
                conversation_id: None,
                subject: step.objective,
                actual: format!("Approved by {}.", approved_by.trim()),
                expected: Some("explicit approval before a gated step".to_owned()),
                error_class: None,
                tool: None,
                model: None,
                environment: None,
                uri: None,
                plan_id: record.plan.as_ref().map(|plan| plan.plan_id.clone()),
                step_id: Some(step_id.to_owned()),
                trust: MemoryTrust::Reviewed,
            },
        )?;
        self.save(&record)?;
        self.snapshot(&record)
    }

    pub fn current_step_permit(&self, workflow_id: &str) -> RuntimeResult<ExecutionPermit> {
        let record = self.load(workflow_id)?;
        if !matches!(
            record.status,
            WorkflowStatus::Active | WorkflowStatus::AwaitingApproval
        ) {
            return Err(RuntimeError::Transition(
                "workflow has no executable current step".to_owned(),
            ));
        }
        let step_id = record
            .current_step_id
            .as_deref()
            .ok_or_else(|| RuntimeError::Transition("workflow has no current step".to_owned()))?;
        let step = plan_step(&record, step_id)?;
        self.permit_step(&record, step, has_approval(&record, step_id))
    }

    pub fn submit_step_result(
        &self,
        workflow_id: &str,
        result: StepResult,
    ) -> RuntimeResult<WorkflowSnapshot> {
        result.validate()?;
        let mut record = self.load(workflow_id)?;
        if record.status != WorkflowStatus::Active {
            return Err(RuntimeError::Transition(
                "workflow is not accepting step results".to_owned(),
            ));
        }
        let plan = record
            .plan
            .clone()
            .ok_or_else(|| RuntimeError::Transition("workflow has no submitted plan".to_owned()))?;
        if result.task_id != record.task_id {
            return Err(RuntimeError::Validation(
                "step result task_id must match workflow task_id".to_owned(),
            ));
        }
        if result.plan_id != plan.plan_id || result.plan_version != plan.version {
            return Err(RuntimeError::Validation(
                "step result must target the active plan id and version".to_owned(),
            ));
        }
        if record.current_step_id.as_deref() != Some(result.step_id.as_str()) {
            return Err(RuntimeError::Validation(
                "step result must target the current active step".to_owned(),
            ));
        }
        let step = plan_step(&record, &result.step_id)?.clone();
        if !step
            .depends_on
            .iter()
            .all(|dependency| complete_steps(&record).contains(dependency))
        {
            return Err(RuntimeError::Transition(
                "step dependencies are not complete".to_owned(),
            ));
        }
        let permit = self.permit_step(&record, &step, has_approval(&record, &step.id))?;
        if !permit.allowed {
            return Err(RuntimeError::Transition(format!(
                "step execution is denied by policy: {}",
                permit.reasons.join("; ")
            )));
        }
        validate_step_evidence(&step, &result)?;
        let prior_failures = failed_attempts(&record, &step.id);
        record
            .results
            .entry(step.id.clone())
            .or_default()
            .push(result.clone());
        self.host.observe(
            &record.task_id,
            EventRecord {
                id: None,
                event_type: format!("step_{:?}", result.status).to_lowercase(),
                scope: MemoryScope::Workflow,
                project_id: record.task.project_id.clone(),
                task_id: record.task_id.clone(),
                conversation_id: None,
                subject: step.objective.clone(),
                actual: result
                    .observations
                    .first()
                    .or_else(|| result.actions.first())
                    .cloned()
                    .unwrap_or_else(|| format!("{:?}", result.status).to_lowercase()),
                expected: Some(step.required_evidence.join("; ")),
                error_class: result.errors.first().map(|error| error.kind.clone()),
                tool: step.tool_policy.recommended.first().cloned(),
                model: None,
                environment: None,
                uri: result.artifacts.first().cloned(),
                plan_id: Some(plan.plan_id.clone()),
                step_id: Some(step.id.clone()),
                trust: MemoryTrust::Observed,
            },
        )?;

        match result.status {
            StepStatus::Failure if prior_failures < usize::from(step.retry_limit) => {
                record.status = WorkflowStatus::Active;
                record.current_step_id = Some(step.id);
            }
            StepStatus::Success => self.advance_success(&mut record, &step)?,
            StepStatus::Partial | StepStatus::Failure | StepStatus::Blocked => {
                record.current_step_id = None;
                record.status = match step.on_failure {
                    FailureRoute::Replan => WorkflowStatus::AwaitingReplan,
                    FailureRoute::Block => WorkflowStatus::Blocked,
                    FailureRoute::Finish => WorkflowStatus::Failed,
                };
            }
        }
        record.updated_at = cordis_contracts::now_rfc3339();
        self.save(&record)?;
        self.snapshot(&record)
    }

    pub fn replan(
        &self,
        workflow_id: &str,
        replacement: PlanIr,
    ) -> RuntimeResult<WorkflowSnapshot> {
        replacement.validate()?;
        let mut record = self.load(workflow_id)?;
        if record.status != WorkflowStatus::AwaitingReplan {
            return Err(RuntimeError::Transition(
                "replacement plan is only allowed after an explicit replan transition".to_owned(),
            ));
        }
        let previous = record
            .plan
            .as_ref()
            .ok_or_else(|| RuntimeError::Transition("workflow has no prior plan".to_owned()))?;
        if replacement.version <= previous.version {
            return Err(RuntimeError::Validation(
                "replacement plan version must increase".to_owned(),
            ));
        }
        self.validate_plan_against_task(&record, &replacement)?;
        let root = replacement
            .steps
            .iter()
            .find(|step| step.depends_on.is_empty())
            .ok_or_else(|| RuntimeError::Validation("replacement plan has no root".to_owned()))?;
        record.current_step_id = Some(root.id.clone());
        record.status = status_for_step(root);
        record.plan = Some(replacement.clone());
        record.updated_at = cordis_contracts::now_rfc3339();
        self.host.observe(
            &record.task_id,
            EventRecord {
                id: None,
                event_type: "plan_replaced".to_owned(),
                scope: MemoryScope::Workflow,
                project_id: record.task.project_id.clone(),
                task_id: record.task_id.clone(),
                conversation_id: None,
                subject: replacement.goal.clone(),
                actual: format!("Plan version {} activated.", replacement.version),
                expected: Some("a revised policy-safe plan after observable evidence".to_owned()),
                error_class: None,
                tool: None,
                model: None,
                environment: None,
                uri: None,
                plan_id: Some(replacement.plan_id.clone()),
                step_id: None,
                trust: MemoryTrust::Observed,
            },
        )?;
        self.save(&record)?;
        self.snapshot(&record)
    }

    pub fn finish(
        &self,
        workflow_id: &str,
        request: WorkflowFinishRequest,
    ) -> RuntimeResult<WorkflowSnapshot> {
        let mut record = self.load(workflow_id)?;
        if record.terminal_feedback.is_some() || record.status == WorkflowStatus::Closed {
            return Err(RuntimeError::Transition(
                "workflow feedback is already finalized".to_owned(),
            ));
        }
        if !matches!(
            record.status,
            WorkflowStatus::Finished
                | WorkflowStatus::Blocked
                | WorkflowStatus::Failed
                | WorkflowStatus::AwaitingReplan
        ) {
            return Err(RuntimeError::Transition(
                "workflow must reach a terminal or replan state before finish".to_owned(),
            ));
        }
        let derived = derive_terminal_outcome(&record);
        let outcome = request.outcome.unwrap_or(derived);
        if outcome != derived {
            return Err(RuntimeError::Validation(format!(
                "requested outcome conflicts with workflow state; expected {derived:?}"
            )));
        }
        let mut evidence: Vec<Evidence> = record
            .results
            .values()
            .flatten()
            .flat_map(|result| result.evidence.clone())
            .collect();
        if evidence.is_empty()
            && record.status == WorkflowStatus::Blocked
            && record.task.authorization.status == AuthorizationStatus::Denied
        {
            evidence.push(Evidence {
                id: None,
                kind: "authorization".to_owned(),
                summary: "workflow was blocked because authorization was explicitly denied"
                    .to_owned(),
                passed: false,
                uri: None,
                acceptance_id: None,
                source_id: record
                    .task
                    .authorization
                    .grant_id
                    .clone()
                    .or_else(|| Some(record.workflow_id.clone())),
                trust: EvidenceTrust::Reviewed,
            });
        } else if evidence.is_empty() {
            return Err(RuntimeError::Validation(
                "workflow cannot finish without step evidence".to_owned(),
            ));
        }
        let required: BTreeSet<_> = record
            .task
            .acceptance_evidence
            .iter()
            .filter(|criterion| criterion.required)
            .map(|criterion| criterion.id.clone())
            .collect();
        let passed_acceptance: BTreeSet<_> = evidence
            .iter()
            .filter(|item| item.passed)
            .filter_map(|item| item.acceptance_id.clone())
            .collect();
        if outcome == Outcome::Success && !required.is_subset(&passed_acceptance) {
            return Err(RuntimeError::Validation(
                "workflow success must explicitly prove every required acceptance_id".to_owned(),
            ));
        }
        if outcome == Outcome::Failure && evidence.iter().all(|item| item.passed) {
            evidence.push(Evidence {
                id: None,
                kind: "workflow".to_owned(),
                summary: "workflow ended without all required evidence".to_owned(),
                passed: false,
                uri: None,
                acceptance_id: None,
                source_id: Some(record.workflow_id.clone()),
                trust: EvidenceTrust::Observed,
            });
        }
        let learned = self.host.finish(FeedbackRequest {
            task_id: record.task_id.clone(),
            outcome,
            attribution: request.attribution,
            lesson: request.lesson,
            evidence,
            outcome_score: None,
        })?;
        record.terminal_feedback = Some(learned);
        record.status = WorkflowStatus::Closed;
        record.current_step_id = None;
        record.updated_at = cordis_contracts::now_rfc3339();
        self.save(&record)?;
        self.snapshot(&record)
    }

    pub fn get(&self, workflow_id: &str) -> RuntimeResult<WorkflowSnapshot> {
        let record = self.load(workflow_id)?;
        self.snapshot(&record)
    }

    pub fn status(&self) -> RuntimeResult<Value> {
        let counts = self.host.store().workflow_status_counts()?;
        Ok(json!({
            "schema": WORKFLOW_RUNTIME_SCHEMA,
            "workflow_count": counts.iter().map(|(_, count)| count).sum::<usize>(),
            "status_counts": counts.into_iter().collect::<BTreeMap<_, _>>(),
        }))
    }

    fn advance_success(&self, record: &mut WorkflowRecord, step: &PlanStep) -> RuntimeResult<()> {
        match step.on_success.as_str() {
            "finish" => {
                let plan = record
                    .plan
                    .as_ref()
                    .ok_or_else(|| RuntimeError::Transition("workflow has no plan".to_owned()))?;
                let incomplete: BTreeSet<_> = plan
                    .steps
                    .iter()
                    .map(|item| item.id.clone())
                    .collect::<BTreeSet<_>>()
                    .difference(&complete_steps(record))
                    .cloned()
                    .collect();
                if !incomplete.is_empty() {
                    return Err(RuntimeError::Transition(format!(
                        "plan requested finish before all steps completed: {}",
                        incomplete.into_iter().collect::<Vec<_>>().join(", ")
                    )));
                }
                record.status = WorkflowStatus::Finished;
                record.current_step_id = None;
            }
            "replan" => {
                record.status = WorkflowStatus::AwaitingReplan;
                record.current_step_id = None;
            }
            successor => {
                let next = plan_step(record, successor)?.clone();
                if !next
                    .depends_on
                    .iter()
                    .all(|dependency| complete_steps(record).contains(dependency))
                {
                    return Err(RuntimeError::Transition(
                        "successor dependencies are not complete".to_owned(),
                    ));
                }
                record.current_step_id = Some(next.id.clone());
                record.status = status_for_step(&next);
            }
        }
        Ok(())
    }

    fn validate_plan_against_task(
        &self,
        record: &WorkflowRecord,
        plan: &PlanIr,
    ) -> RuntimeResult<()> {
        if plan.task_id != record.task_id
            || plan.workflow_id != record.workflow_id
            || plan.goal != record.task.goal
        {
            return Err(RuntimeError::Validation(
                "plan must preserve workflow_id, task_id and goal exactly".to_owned(),
            ));
        }
        let roots: Vec<_> = plan
            .steps
            .iter()
            .filter(|step| step.depends_on.is_empty())
            .collect();
        if roots.len() != 1 {
            return Err(RuntimeError::Validation(
                "minimum runtime requires exactly one root step".to_owned(),
            ));
        }
        let mut incoming: BTreeMap<String, Vec<String>> = plan
            .steps
            .iter()
            .map(|step| (step.id.clone(), Vec::new()))
            .collect();
        for step in &plan.steps {
            if !matches!(step.on_success.as_str(), "finish" | "replan") {
                incoming
                    .get_mut(&step.on_success)
                    .ok_or_else(|| RuntimeError::Validation("unknown successor".to_owned()))?
                    .push(step.id.clone());
            }
        }
        for step in &plan.steps {
            if step.id == roots[0].id {
                continue;
            }
            let parents = incoming.get(&step.id).cloned().unwrap_or_default();
            if parents.len() != 1 || step.depends_on != parents {
                return Err(RuntimeError::Validation(
                    "minimum runtime requires one sequential success path".to_owned(),
                ));
            }
        }
        let scope_in: BTreeSet<_> = record.task.scope.included.iter().cloned().collect();
        let scope_out: BTreeSet<_> = record.task.scope.excluded.iter().cloned().collect();
        for step in &plan.steps {
            let step_scope: BTreeSet<_> = step.allowed_scope.iter().cloned().collect();
            if !step_scope.is_subset(&scope_in) {
                return Err(RuntimeError::Validation(format!(
                    "step {} allowed_scope exceeds task scope.in",
                    step.id
                )));
            }
            if !step_scope.is_disjoint(&scope_out) {
                return Err(RuntimeError::Validation(format!(
                    "step {} allowed_scope overlaps task scope.out",
                    step.id
                )));
            }
            if matches!(
                record.difficulty.control_mode,
                ControlMode::HighIntervention | ControlMode::Takeover
            ) && step.action_class == ActionClass::Change
                && !step.approval_required
            {
                return Err(RuntimeError::Validation(
                    "high-risk change steps require approval".to_owned(),
                ));
            }
            if record.difficulty.control_mode == ControlMode::Takeover
                && step.action_class == ActionClass::Change
            {
                return Err(RuntimeError::Validation(
                    "takeover workflows may not include change steps".to_owned(),
                ));
            }
            self.validate_step_policy(record, step)?;
        }
        Ok(())
    }

    fn validate_step_policy(&self, record: &WorkflowRecord, step: &PlanStep) -> RuntimeResult<()> {
        let tools: Vec<Option<String>> = if step.tool_policy.allowed.is_empty() {
            vec![None]
        } else {
            step.tool_policy.allowed.iter().cloned().map(Some).collect()
        };
        let targets: Vec<Option<String>> = if step.allowed_scope.is_empty() {
            vec![None]
        } else {
            step.allowed_scope.iter().cloned().map(Some).collect()
        };
        for tool in &tools {
            for target in &targets {
                let permit = self.policy.evaluate(
                    &PolicyContext {
                        stakes: record.task.stakes,
                        risk_score: record.difficulty.aggregate_score.unwrap_or(0.5),
                        control_mode: record.difficulty.control_mode,
                        authorization: record.task.authorization.clone(),
                        approval_required: step.approval_required,
                        scope_in: record.task.scope.included.clone(),
                        scope_out: record.task.scope.excluded.clone(),
                    },
                    &ActionProposal {
                        action_id: Some(format!("plan:{}:{}", record.workflow_id, step.id)),
                        action_class: step.action_class,
                        action_name: step.action_class.as_policy_name().to_owned(),
                        description: step.objective.clone(),
                        purpose: "validate proposed workflow step".to_owned(),
                        tool: tool.clone(),
                        target: target.clone(),
                        network_access: step_uses_network(step, target.as_deref()),
                        destructive: false,
                        // Plan admission proves an approval gate exists. Runtime checks the actual
                        // approval again immediately before accepting a step result.
                        approval_granted: step.approval_required,
                    },
                )?;
                if !permit.allowed {
                    return Err(RuntimeError::Validation(format!(
                        "step {} violates authorization or scope policy: {}",
                        step.id,
                        permit.reasons.join("; ")
                    )));
                }
            }
        }
        Ok(())
    }

    fn permit_step(
        &self,
        record: &WorkflowRecord,
        step: &PlanStep,
        approval_granted: bool,
    ) -> RuntimeResult<ExecutionPermit> {
        let tool = step
            .tool_policy
            .recommended
            .first()
            .or_else(|| step.tool_policy.allowed.first())
            .cloned();
        let target = step.allowed_scope.first().cloned();
        self.policy
            .evaluate(
                &PolicyContext {
                    stakes: record.task.stakes,
                    risk_score: record.difficulty.aggregate_score.unwrap_or(0.5),
                    control_mode: record.difficulty.control_mode,
                    authorization: record.task.authorization.clone(),
                    approval_required: step.approval_required,
                    scope_in: record.task.scope.included.clone(),
                    scope_out: record.task.scope.excluded.clone(),
                },
                &ActionProposal {
                    action_id: Some(format!("step:{}:{}", record.workflow_id, step.id)),
                    action_class: step.action_class,
                    action_name: step.action_class.as_policy_name().to_owned(),
                    description: step.objective.clone(),
                    purpose: "execute the current validated workflow step".to_owned(),
                    tool,
                    target: target.clone(),
                    network_access: step_uses_network(step, target.as_deref()),
                    destructive: false,
                    approval_granted,
                },
            )
            .map_err(RuntimeError::from)
    }

    fn snapshot(&self, record: &WorkflowRecord) -> RuntimeResult<WorkflowSnapshot> {
        let current_step = record
            .current_step_id
            .as_deref()
            .map(|step_id| plan_step(record, step_id).cloned())
            .transpose()?;
        Ok(WorkflowSnapshot {
            schema: WORKFLOW_RUNTIME_SCHEMA.to_owned(),
            workflow_id: record.workflow_id.clone(),
            task_id: record.task_id.clone(),
            status: record.status,
            control_mode: record.difficulty.control_mode,
            task: record.task.clone(),
            difficulty: record.difficulty.clone(),
            plan: record.plan.clone(),
            current_step,
            eligible_step_ids: eligible_steps(record),
            result_count: record.results.values().map(Vec::len).sum(),
            approvals: record.approvals.clone(),
            terminal_feedback: record.terminal_feedback.clone(),
        })
    }

    fn load(&self, workflow_id: &str) -> RuntimeResult<WorkflowRecord> {
        self.load_optional(workflow_id)?
            .ok_or_else(|| RuntimeError::Transition("unknown workflow_id".to_owned()))
    }

    fn load_optional(&self, workflow_id: &str) -> RuntimeResult<Option<WorkflowRecord>> {
        self.host
            .store()
            .load_workflow(workflow_id)
            .map_err(RuntimeError::from)
    }

    fn save(&self, record: &WorkflowRecord) -> RuntimeResult<()> {
        self.host.store().save_workflow(
            &record.workflow_id,
            &record.task_id,
            &enum_name(record.status)?,
            record,
        )?;
        Ok(())
    }
}

fn workflow_id(task_id: &str) -> String {
    format!("workflow:{task_id}")
}

fn enum_name<T: Serialize>(value: T) -> RuntimeResult<String> {
    Ok(serde_json::to_string(&value)?.trim_matches('"').to_owned())
}

fn plan_step<'a>(record: &'a WorkflowRecord, step_id: &str) -> RuntimeResult<&'a PlanStep> {
    record
        .plan
        .as_ref()
        .and_then(|plan| plan.steps.iter().find(|step| step.id == step_id))
        .ok_or_else(|| RuntimeError::Transition(format!("unknown plan step: {step_id}")))
}

fn complete_steps(record: &WorkflowRecord) -> BTreeSet<String> {
    record
        .results
        .iter()
        .filter(|(_, results)| {
            results
                .last()
                .is_some_and(|result| result.status == StepStatus::Success)
        })
        .map(|(step_id, _)| step_id.clone())
        .collect()
}

fn failed_attempts(record: &WorkflowRecord, step_id: &str) -> usize {
    record
        .results
        .get(step_id)
        .into_iter()
        .flatten()
        .filter(|result| result.status != StepStatus::Success)
        .count()
}

fn eligible_steps(record: &WorkflowRecord) -> Vec<String> {
    let Some(plan) = &record.plan else {
        return Vec::new();
    };
    let completed = complete_steps(record);
    plan.steps
        .iter()
        .filter(|step| {
            !completed.contains(&step.id)
                && step
                    .depends_on
                    .iter()
                    .all(|dependency| completed.contains(dependency))
        })
        .map(|step| step.id.clone())
        .collect()
}

fn has_approval(record: &WorkflowRecord, step_id: &str) -> bool {
    let Some(plan) = record.plan.as_ref() else {
        return false;
    };
    record.approvals.iter().any(|approval| {
        approval.step_id == step_id
            && approval.plan_id == plan.plan_id
            && approval.plan_version == plan.version
    })
}

fn status_for_step(step: &PlanStep) -> WorkflowStatus {
    if step.approval_required {
        WorkflowStatus::AwaitingApproval
    } else {
        WorkflowStatus::Active
    }
}

fn validate_step_evidence(step: &PlanStep, result: &StepResult) -> RuntimeResult<()> {
    if result.status == StepStatus::Success {
        let passed: BTreeSet<_> = result
            .evidence
            .iter()
            .filter(|item| item.passed)
            .map(|item| item.summary.as_str())
            .collect();
        let missing: Vec<_> = step
            .required_evidence
            .iter()
            .filter(|required| !passed.contains(required.as_str()))
            .cloned()
            .collect();
        if !missing.is_empty() {
            return Err(RuntimeError::Validation(format!(
                "successful step result is missing required evidence: {}",
                missing.join("; ")
            )));
        }
    }
    Ok(())
}

fn derive_terminal_outcome(record: &WorkflowRecord) -> Outcome {
    match record.status {
        WorkflowStatus::Finished => Outcome::Success,
        WorkflowStatus::AwaitingReplan => Outcome::Partial,
        WorkflowStatus::Blocked | WorkflowStatus::Failed => Outcome::Failure,
        _ => Outcome::Failure,
    }
}

fn step_uses_network(step: &PlanStep, target: Option<&str>) -> bool {
    let method = step.method.recommended.to_lowercase();
    let target_network = target.is_some_and(|value| {
        let value = value.to_lowercase();
        value.starts_with("http://")
            || value.starts_with("https://")
            || value.starts_with("ssh://")
            || value.starts_with("git://")
    });
    target_network
        || ["network", "http", "remote", "web", "api"]
            .iter()
            .any(|item| method.contains(item))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cordis_contracts::{
        AcceptanceCriterion, AuthorizationStatus, Completeness, KnownFact, MethodPolicy,
        ModelRequirements, NetworkProfile, PLAN_SCHEMA, STEP_RESULT_SCHEMA, Stakes,
        TASK_CONTRACT_SCHEMA, TaskScope, ToolPolicy, UnknownQuestion,
    };
    use cordis_store::CordisStore;
    use tempfile::tempdir;

    fn workflow_runtime() -> CordisWorkflowRuntime {
        let path = tempdir().unwrap().keep().join("workflow.db");
        CordisWorkflowRuntime::new(CordisHostRuntime::new(CordisStore::open(path).unwrap()))
    }

    fn task(stakes: Stakes) -> TaskContract {
        let mut authorization = AuthorizationEnvelope::default();
        if stakes != Stakes::High && stakes != Stakes::Critical {
            authorization.status = AuthorizationStatus::Granted;
            authorization.basis = "test owner approval".to_owned();
            authorization.allowed_actions = vec!["verify".to_owned(), "read".to_owned()];
            authorization.allowed_tools = vec!["cargo".to_owned()];
            authorization.allowed_targets = vec!["crates/cordis-runtime".to_owned()];
            authorization.network_profile = NetworkProfile::Offline;
        }
        TaskContract {
            schema: TASK_CONTRACT_SCHEMA.to_owned(),
            task_id: "workflow-test".to_owned(),
            goal: "Validate the workflow controller".to_owned(),
            project_id: "cordis".to_owned(),
            domain: "software".to_owned(),
            stakes,
            stakeholders: vec![],
            motivation: "Prove durable transitions".to_owned(),
            scope: TaskScope {
                included: vec!["crates/cordis-runtime".to_owned()],
                excluded: vec!["production".to_owned()],
            },
            authorization,
            constraints: vec!["Do not call external services".to_owned()],
            acceptance_evidence: vec![AcceptanceCriterion {
                id: "verified".to_owned(),
                description: "workflow test passes".to_owned(),
                required: true,
            }],
            known_facts: vec![KnownFact {
                claim: "cargo is available".to_owned(),
                evidence_ids: vec!["fixture".to_owned()],
            }],
            unknowns: Vec::<UnknownQuestion>::new(),
            completeness: Completeness::default(),
        }
    }

    fn plan(snapshot: &WorkflowSnapshot, approval_required: bool) -> PlanIr {
        PlanIr {
            schema: PLAN_SCHEMA.to_owned(),
            plan_id: "plan-1".to_owned(),
            version: 1,
            task_id: snapshot.task_id.clone(),
            workflow_id: snapshot.workflow_id.clone(),
            goal: snapshot.task.goal.clone(),
            assumptions: vec![],
            alternatives_considered: vec![],
            steps: vec![PlanStep {
                id: "S1".to_owned(),
                objective: "Run workflow tests".to_owned(),
                depends_on: vec![],
                allowed_scope: vec!["crates/cordis-runtime".to_owned()],
                action_class: ActionClass::Verify,
                method: MethodPolicy {
                    recommended: "cargo_test".to_owned(),
                    alternatives: vec![],
                },
                tool_policy: ToolPolicy {
                    recommended: vec!["cargo".to_owned()],
                    allowed: vec!["cargo".to_owned()],
                    forbidden: vec![],
                },
                model_requirements: ModelRequirements::default(),
                required_evidence: vec!["S1 passes".to_owned()],
                approval_required,
                retry_limit: 0,
                stop_when: vec!["tests fail".to_owned()],
                on_success: "finish".to_owned(),
                on_failure: FailureRoute::Block,
            }],
            completion_gate: vec!["workflow test passes".to_owned()],
        }
    }

    fn result(snapshot: &WorkflowSnapshot, plan: &PlanIr) -> StepResult {
        StepResult {
            schema: STEP_RESULT_SCHEMA.to_owned(),
            task_id: snapshot.task_id.clone(),
            plan_id: plan.plan_id.clone(),
            plan_version: plan.version,
            step_id: "S1".to_owned(),
            status: StepStatus::Success,
            actions: vec!["cargo test".to_owned()],
            observations: vec!["all tests passed".to_owned()],
            artifacts: vec!["target/test-results".to_owned()],
            evidence: vec![Evidence {
                id: None,
                kind: "test".to_owned(),
                summary: "S1 passes".to_owned(),
                passed: true,
                uri: None,
                acceptance_id: Some("verified".to_owned()),
                source_id: Some("cargo-test-1".to_owned()),
                trust: EvidenceTrust::Observed,
            }],
            errors: vec![],
            proposed_next: None,
        }
    }

    #[test]
    fn persists_plan_and_closes_learning_loop() {
        let runtime = workflow_runtime();
        let started = runtime
            .begin(WorkflowBeginRequest {
                task: task(Stakes::Low),
                difficulty: DifficultyInputs::default(),
            })
            .unwrap();
        let plan = plan(&started, false);
        runtime
            .submit_plan(&started.workflow_id, plan.clone())
            .unwrap();
        let finished = runtime
            .submit_step_result(&started.workflow_id, result(&started, &plan))
            .unwrap();
        assert_eq!(finished.status, WorkflowStatus::Finished);
        let closed = runtime
            .finish(
                &started.workflow_id,
                WorkflowFinishRequest {
                    outcome: None,
                    lesson: Some("tests pass".to_owned()),
                    attribution: None,
                },
            )
            .unwrap();
        assert_eq!(closed.status, WorkflowStatus::Closed);
        assert_eq!(
            closed.terminal_feedback.unwrap().event.outcome,
            Outcome::Success
        );
    }

    #[test]
    fn high_stakes_workflow_waits_for_authorization() {
        let runtime = workflow_runtime();
        let started = runtime
            .begin(WorkflowBeginRequest {
                task: task(Stakes::High),
                difficulty: DifficultyInputs::default(),
            })
            .unwrap();
        assert_eq!(started.status, WorkflowStatus::AwaitingAuthorization);
        let mut authorization = started.task.authorization;
        authorization.status = AuthorizationStatus::Granted;
        authorization.basis = "maintainer approval".to_owned();
        authorization.allowed_actions = vec!["verify".to_owned()];
        authorization.allowed_tools = vec!["cargo".to_owned()];
        authorization.allowed_targets = vec!["crates/cordis-runtime".to_owned()];
        let authorized = runtime
            .set_authorization(&started.workflow_id, authorization)
            .unwrap();
        assert_eq!(authorized.status, WorkflowStatus::AwaitingPlan);
    }

    #[test]
    fn explicitly_denied_workflow_can_close_with_policy_evidence() {
        let runtime = workflow_runtime();
        let mut denied_task = task(Stakes::High);
        denied_task.authorization.status = AuthorizationStatus::Denied;
        denied_task.authorization.basis = "owner denied the requested operation".to_owned();
        denied_task.authorization.grant_id = Some("denial-1".to_owned());
        let started = runtime
            .begin(WorkflowBeginRequest {
                task: denied_task,
                difficulty: DifficultyInputs::default(),
            })
            .unwrap();
        assert_eq!(started.status, WorkflowStatus::Blocked);
        let closed = runtime
            .finish(
                &started.workflow_id,
                WorkflowFinishRequest {
                    outcome: Some(Outcome::Failure),
                    lesson: Some("Do not proceed after explicit denial.".to_owned()),
                    attribution: None,
                },
            )
            .unwrap();
        assert_eq!(closed.status, WorkflowStatus::Closed);
        assert_eq!(
            closed.terminal_feedback.unwrap().event.outcome,
            Outcome::Failure
        );
    }

    #[test]
    fn explicit_approval_gate_is_enforced() {
        let runtime = workflow_runtime();
        let started = runtime
            .begin(WorkflowBeginRequest {
                task: task(Stakes::Low),
                difficulty: DifficultyInputs::default(),
            })
            .unwrap();
        let plan = plan(&started, true);
        let waiting = runtime
            .submit_plan(&started.workflow_id, plan.clone())
            .unwrap();
        assert_eq!(waiting.status, WorkflowStatus::AwaitingApproval);
        assert!(
            runtime
                .submit_step_result(&started.workflow_id, result(&started, &plan))
                .is_err()
        );
        runtime
            .approve_step(
                &started.workflow_id,
                "S1",
                "maintainer",
                Some("reviewed".to_owned()),
            )
            .unwrap();
        assert!(
            runtime
                .submit_step_result(&started.workflow_id, result(&started, &plan))
                .is_ok()
        );
    }

    #[test]
    fn approval_is_bound_to_the_active_plan_version() {
        let runtime = workflow_runtime();
        let started = runtime
            .begin(WorkflowBeginRequest {
                task: task(Stakes::Low),
                difficulty: DifficultyInputs::default(),
            })
            .unwrap();

        let mut first_plan = plan(&started, true);
        first_plan.steps[0].on_failure = FailureRoute::Replan;
        runtime
            .submit_plan(&started.workflow_id, first_plan.clone())
            .unwrap();
        runtime
            .approve_step(
                &started.workflow_id,
                "S1",
                "maintainer",
                Some("approved plan version 1".to_owned()),
            )
            .unwrap();

        let failed = StepResult {
            schema: STEP_RESULT_SCHEMA.to_owned(),
            task_id: started.task_id.clone(),
            plan_id: first_plan.plan_id.clone(),
            plan_version: first_plan.version,
            step_id: "S1".to_owned(),
            status: StepStatus::Failure,
            actions: vec!["cargo test".to_owned()],
            observations: vec!["workflow test failed".to_owned()],
            artifacts: vec![],
            evidence: vec![Evidence {
                id: None,
                kind: "test".to_owned(),
                summary: "S1 failed".to_owned(),
                passed: false,
                uri: None,
                acceptance_id: None,
                source_id: Some("cargo-test-failure-1".to_owned()),
                trust: EvidenceTrust::Observed,
            }],
            errors: vec![cordis_contracts::StepError {
                kind: "test_failure".to_owned(),
                summary: "workflow test failed".to_owned(),
            }],
            proposed_next: Some("replan".to_owned()),
        };
        let awaiting_replan = runtime
            .submit_step_result(&started.workflow_id, failed)
            .unwrap();
        assert_eq!(awaiting_replan.status, WorkflowStatus::AwaitingReplan);

        let mut replacement = plan(&started, true);
        replacement.plan_id = "plan-2".to_owned();
        replacement.version = 2;
        replacement.steps[0].on_failure = FailureRoute::Block;
        let waiting = runtime.replan(&started.workflow_id, replacement).unwrap();
        assert_eq!(waiting.status, WorkflowStatus::AwaitingApproval);

        let permit = runtime.current_step_permit(&started.workflow_id).unwrap();
        assert!(!permit.approval_satisfied);
        assert!(!permit.allowed);
        assert!(
            permit
                .reasons
                .iter()
                .any(|reason| reason.contains("approval"))
        );
    }
}
