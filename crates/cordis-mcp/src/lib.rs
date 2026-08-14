//! Newline-delimited JSON-RPC stdio MCP transport for CORDIS.

use cordis_capability::{RegisterCapability, ToolProbeSpec};
use cordis_contracts::{
    AcceptanceCriterion, ActionClass, Attribution, AuthorizationEnvelope, Completeness,
    DifficultyInputs, DifficultyProfile, EventRecord, KnownFact, MemoryKind, MemoryScope,
    MemoryTrust, Outcome, PlanIr, PreflightRequest, RememberRequest, Stakes, StepResult,
    TaskContract, UnknownQuestion,
};
use cordis_core::SeedStrategyRequest;
use cordis_planner::CordisPlanner;
use cordis_policy::ActionProposal;
use cordis_runtime::{WorkflowBeginRequest, WorkflowFinishRequest};
use cordis_sdk::{CordisEngine, TaskContractInput};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Write};
use std::path::Path;
use thiserror::Error;

pub const MODERN_PROTOCOL_VERSION: &str = "2026-07-28";
pub const LEGACY_PROTOCOL_VERSION: &str = "2025-11-25";
const LEGACY_PROTOCOLS: &[&str] = &["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25"];

#[derive(Debug, Error)]
pub enum McpError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("invalid MCP request: {0}")]
    InvalidRequest(String),
    #[error("unknown tool: {0}")]
    UnknownTool(String),
}

pub type McpResult<T> = Result<T, McpError>;

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    #[serde(default)]
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Deserialize)]
struct ToolCallParams {
    name: String,
    #[serde(default)]
    arguments: Value,
}

#[derive(Debug, Clone, Serialize)]
struct ToolDefinition {
    name: String,
    title: String,
    description: String,
    #[serde(rename = "inputSchema")]
    input_schema: Value,
    annotations: Value,
}

#[derive(Clone)]
pub struct CordisMcpServer {
    engine: CordisEngine,
}

impl CordisMcpServer {
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self, cordis_sdk::SdkError> {
        Ok(Self {
            engine: CordisEngine::open(data_dir)?,
        })
    }

    pub fn engine(&self) -> &CordisEngine {
        &self.engine
    }

    pub fn run_stdio(&self) -> McpResult<()> {
        let stdin = io::stdin();
        let mut stdout = io::BufWriter::new(io::stdout().lock());
        for line in stdin.lock().lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            let response = match serde_json::from_str::<Value>(&line) {
                Ok(value) => self.handle_value(value),
                Err(error) => Some(jsonrpc_error(
                    Value::Null,
                    -32700,
                    &format!("Parse error: {error}"),
                )),
            };
            if let Some(response) = response {
                serde_json::to_writer(&mut stdout, &response)?;
                stdout.write_all(b"\n")?;
                stdout.flush()?;
            }
        }
        Ok(())
    }

    pub fn handle_value(&self, value: Value) -> Option<Value> {
        let request = match serde_json::from_value::<JsonRpcRequest>(value) {
            Ok(request) => request,
            Err(error) => return Some(jsonrpc_error(Value::Null, -32600, &error.to_string())),
        };
        if request.jsonrpc != "2.0" {
            return request
                .id
                .map(|id| jsonrpc_error(id, -32600, "jsonrpc must be 2.0"));
        }
        let id = request.id.clone();
        let result = self.dispatch(&request);
        match (id, result) {
            (None, _) => None,
            (Some(id), Ok(result)) => Some(json!({"jsonrpc": "2.0", "id": id, "result": result})),
            (Some(id), Err(McpError::UnknownTool(name))) => {
                Some(jsonrpc_error(id, -32602, &format!("Unknown tool: {name}")))
            }
            (Some(id), Err(error)) if request.method == "tools/call" => Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": tool_result(json!({"error": error.to_string()}), true),
            })),
            (Some(id), Err(error)) => Some(jsonrpc_error(id, -32602, &error.to_string())),
        }
    }

    fn dispatch(&self, request: &JsonRpcRequest) -> McpResult<Value> {
        match request.method.as_str() {
            "initialize" => self.initialize(&request.params),
            "server/discover" => Ok(self.discover()),
            "notifications/initialized" | "notifications/cancelled" => Ok(Value::Null),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({
                "resultType": "complete",
                "tools": tool_definitions(),
            })),
            "tools/call" => {
                let call: ToolCallParams = serde_json::from_value(request.params.clone())?;
                let result = self.call_tool(&call.name, call.arguments)?;
                Ok(tool_result(result, false))
            }
            method => Err(McpError::InvalidRequest(format!(
                "method not found: {method}"
            ))),
        }
    }

    fn initialize(&self, params: &Value) -> McpResult<Value> {
        let requested = params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or(LEGACY_PROTOCOL_VERSION);
        let protocol = if LEGACY_PROTOCOLS.contains(&requested) {
            requested
        } else {
            LEGACY_PROTOCOL_VERSION
        };
        Ok(json!({
            "protocolVersion": protocol,
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": server_info(),
            "instructions": "CORDIS is a fail-closed cognitive runtime. Start substantive work with cordis_begin or cordis_workflow_begin, record observable evidence, and finish exactly once."
        }))
    }

    fn discover(&self) -> Value {
        json!({
            "supportedVersions": [MODERN_PROTOCOL_VERSION, LEGACY_PROTOCOL_VERSION, "2025-06-18", "2025-03-26", "2024-11-05"],
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": server_info(),
            "instructions": "Stateless MCP transport over a durable CORDIS application database. Task/workflow IDs are explicit state handles."
        })
    }

    fn call_tool(&self, name: &str, arguments: Value) -> McpResult<Value> {
        match name {
            "cordis_begin" => self.tool_begin(arguments),
            "cordis_query" => self.tool_query(arguments),
            "cordis_observe" => self.tool_observe(arguments),
            "cordis_check_action" => self.tool_check_action(arguments),
            "cordis_finish" => self.tool_finish(arguments),
            "cordis_status" => self.engine.status().map_err(string_error),
            "cordis_memory_remember" => self.tool_memory_remember(arguments),
            "cordis_seed_strategy" => self.tool_seed_strategy(arguments),
            "cordis_workflow_begin" => self.tool_workflow_begin(arguments),
            "cordis_workflow_set_authorization" => self.tool_workflow_authorization(arguments),
            "cordis_workflow_submit_plan" => self.tool_workflow_plan(arguments),
            "cordis_workflow_approve_step" => self.tool_workflow_approve(arguments),
            "cordis_workflow_current_permit" => self.tool_workflow_permit(arguments),
            "cordis_workflow_submit_step_result" => self.tool_workflow_result(arguments),
            "cordis_workflow_replan" => self.tool_workflow_replan(arguments),
            "cordis_workflow_finish" => self.tool_workflow_finish(arguments),
            "cordis_workflow_get" => self.tool_workflow_get(arguments),
            "cordis_goal_review" => self.tool_goal_review(arguments),
            "cordis_planner_fast_route" => self.tool_planner_fast(arguments),
            "cordis_capability_register" => self.tool_capability_register(arguments),
            "cordis_capability_detect" => self.tool_capability_detect(arguments),
            "cordis_capability_require" => self.tool_capability_require(arguments),
            "cordis_capability_status" => self.engine.capability().status().map_err(string_error),
            other => Err(McpError::UnknownTool(other.to_owned())),
        }
    }

    fn tool_begin(&self, arguments: Value) -> McpResult<Value> {
        let args: BeginArgs = serde_json::from_value(arguments)?;
        let acceptance = normalize_acceptance(args.acceptance_evidence);
        let mut result = self
            .engine
            .host()
            .begin(PreflightRequest {
                task: cordis_contracts::CoreTask {
                    id: args.task_id,
                    goal: args.goal,
                    domain: args.domain,
                    project_id: args.project_id,
                    strategy_id: args.strategy_id,
                    stakes: args.stakes,
                    authorization: args.authorization,
                },
                complexity: args.complexity,
                unknowns: args.unknowns,
                constraints: args.constraints,
                current_step: args.current_step,
                acceptance_evidence: acceptance,
            })
            .map_err(string_error)?;
        if !args.scope_in.is_empty() || !args.scope_out.is_empty() {
            let focus = self
                .engine
                .host()
                .update_scope(&result.task_id, args.scope_in, args.scope_out)
                .map_err(string_error)?;
            result.focus = focus;
        }
        serde_json::to_value(result).map_err(McpError::from)
    }

    fn tool_query(&self, arguments: Value) -> McpResult<Value> {
        let args: QueryArgs = serde_json::from_value(arguments)?;
        serde_json::to_value(
            self.engine
                .host()
                .query(
                    args.task_id.as_str(),
                    args.intent,
                    args.scopes,
                    args.kinds,
                    args.limit,
                )
                .map_err(string_error)?,
        )
        .map_err(McpError::from)
    }

    fn tool_observe(&self, arguments: Value) -> McpResult<Value> {
        let args: ObserveArgs = serde_json::from_value(arguments)?;
        self.engine
            .host()
            .observe(
                &args.task_id,
                EventRecord {
                    id: args.event_id,
                    event_type: args.event_type.clone(),
                    scope: args.scope,
                    project_id: String::new(),
                    task_id: args.task_id.clone(),
                    conversation_id: args.conversation_id,
                    subject: args.subject.unwrap_or(args.event_type),
                    actual: args
                        .actual
                        .or(args.summary)
                        .unwrap_or_else(|| "event observed".to_owned()),
                    expected: args.expected,
                    error_class: args.error_class,
                    tool: args.tool,
                    model: args.model,
                    environment: args.environment,
                    uri: args.uri,
                    plan_id: args.plan_id,
                    step_id: args.step_id,
                    trust: args.trust,
                },
            )
            .map_err(string_error)
    }

    fn tool_check_action(&self, arguments: Value) -> McpResult<Value> {
        let args: CheckActionArgs = serde_json::from_value(arguments)?;
        serde_json::to_value(
            self.engine
                .host()
                .check_action(
                    &args.task_id,
                    ActionProposal {
                        action_id: args.action_id,
                        action_class: args.action_class,
                        action_name: args
                            .action_name
                            .unwrap_or_else(|| args.action_class.as_policy_name().to_owned()),
                        description: args.description,
                        purpose: args.purpose,
                        tool: args.tool,
                        target: args.target,
                        network_access: args.network_access,
                        destructive: args.destructive,
                        approval_granted: args.approval_granted,
                    },
                )
                .map_err(string_error)?,
        )
        .map_err(McpError::from)
    }

    fn tool_finish(&self, arguments: Value) -> McpResult<Value> {
        let request: cordis_contracts::FeedbackRequest = serde_json::from_value(arguments)?;
        serde_json::to_value(self.engine.host().finish(request).map_err(string_error)?)
            .map_err(McpError::from)
    }

    fn tool_memory_remember(&self, arguments: Value) -> McpResult<Value> {
        let request: RememberRequest = serde_json::from_value(arguments)?;
        serde_json::to_value(
            self.engine
                .host()
                .memory()
                .remember(request)
                .map_err(string_error)?,
        )
        .map_err(McpError::from)
    }

    fn tool_seed_strategy(&self, arguments: Value) -> McpResult<Value> {
        let request: SeedStrategyRequest = serde_json::from_value(arguments)?;
        serde_json::to_value(
            self.engine
                .host()
                .core()
                .seed_strategy(request)
                .map_err(string_error)?,
        )
        .map_err(McpError::from)
    }

    fn tool_workflow_begin(&self, arguments: Value) -> McpResult<Value> {
        let args: WorkflowBeginArgs = serde_json::from_value(arguments)?;
        let task = TaskContractInput {
            task_id: args.task_id,
            goal: args.goal,
            project_id: args.project_id,
            domain: args.domain,
            stakes: args.stakes,
            stakeholders: args.stakeholders,
            motivation: args.motivation,
            scope_in: args.scope_in,
            scope_out: args.scope_out,
            authorization: args.authorization,
            constraints: args.constraints,
            acceptance_evidence: normalize_acceptance(args.acceptance_evidence),
            known_facts: args.known_facts,
            unknowns: args.unknowns,
            completeness: args.completeness,
        }
        .build()
        .map_err(string_error)?;
        let mut difficulty = args.difficulty;
        if let Some(value) = args.complexity {
            difficulty.complexity = value;
        }
        if let Some(value) = args.irreversibility {
            difficulty.irreversibility = value;
        }
        if let Some(value) = args.novelty {
            difficulty.novelty = value;
        }
        if args.novelty_reason.is_some() {
            difficulty.novelty_reason = args.novelty_reason;
        }
        serde_json::to_value(
            self.engine
                .workflow()
                .begin(WorkflowBeginRequest { task, difficulty })
                .map_err(string_error)?,
        )
        .map_err(McpError::from)
    }

    fn tool_workflow_authorization(&self, arguments: Value) -> McpResult<Value> {
        let args: WorkflowAuthorizationArgs = serde_json::from_value(arguments)?;
        serde_json::to_value(
            self.engine
                .workflow()
                .set_authorization(&args.workflow_id, args.authorization)
                .map_err(string_error)?,
        )
        .map_err(McpError::from)
    }

    fn tool_workflow_plan(&self, arguments: Value) -> McpResult<Value> {
        let args: WorkflowPlanArgs = serde_json::from_value(arguments)?;
        serde_json::to_value(
            self.engine
                .workflow()
                .submit_plan(&args.workflow_id, args.plan)
                .map_err(string_error)?,
        )
        .map_err(McpError::from)
    }

    fn tool_workflow_approve(&self, arguments: Value) -> McpResult<Value> {
        let args: WorkflowApproveArgs = serde_json::from_value(arguments)?;
        serde_json::to_value(
            self.engine
                .workflow()
                .approve_step(
                    &args.workflow_id,
                    &args.step_id,
                    &args.approved_by,
                    args.reason,
                )
                .map_err(string_error)?,
        )
        .map_err(McpError::from)
    }

    fn tool_workflow_permit(&self, arguments: Value) -> McpResult<Value> {
        let args: WorkflowIdArgs = serde_json::from_value(arguments)?;
        serde_json::to_value(
            self.engine
                .workflow()
                .current_step_permit(&args.workflow_id)
                .map_err(string_error)?,
        )
        .map_err(McpError::from)
    }

    fn tool_workflow_result(&self, arguments: Value) -> McpResult<Value> {
        let args: WorkflowResultArgs = serde_json::from_value(arguments)?;
        serde_json::to_value(
            self.engine
                .workflow()
                .submit_step_result(&args.workflow_id, args.step_result)
                .map_err(string_error)?,
        )
        .map_err(McpError::from)
    }

    fn tool_workflow_replan(&self, arguments: Value) -> McpResult<Value> {
        let args: WorkflowPlanArgs = serde_json::from_value(arguments)?;
        serde_json::to_value(
            self.engine
                .workflow()
                .replan(&args.workflow_id, args.plan)
                .map_err(string_error)?,
        )
        .map_err(McpError::from)
    }

    fn tool_workflow_finish(&self, arguments: Value) -> McpResult<Value> {
        let args: WorkflowFinishArgs = serde_json::from_value(arguments)?;
        serde_json::to_value(
            self.engine
                .workflow()
                .finish(
                    &args.workflow_id,
                    WorkflowFinishRequest {
                        outcome: args.outcome,
                        lesson: args.lesson,
                        attribution: args.attribution,
                    },
                )
                .map_err(string_error)?,
        )
        .map_err(McpError::from)
    }

    fn tool_workflow_get(&self, arguments: Value) -> McpResult<Value> {
        let args: WorkflowIdArgs = serde_json::from_value(arguments)?;
        serde_json::to_value(
            self.engine
                .workflow()
                .get(&args.workflow_id)
                .map_err(string_error)?,
        )
        .map_err(McpError::from)
    }

    fn tool_goal_review(&self, arguments: Value) -> McpResult<Value> {
        let args: GoalReviewArgs = serde_json::from_value(arguments)?;
        serde_json::to_value(
            self.engine
                .goal_mode()
                .begin(
                    &args.task,
                    &args.difficulty,
                    args.cognitive_ir,
                    args.planner_enabled,
                )
                .map_err(string_error)?,
        )
        .map_err(McpError::from)
    }

    fn tool_planner_fast(&self, arguments: Value) -> McpResult<Value> {
        let args: PlannerFastArgs = serde_json::from_value(arguments)?;
        serde_json::to_value(
            CordisPlanner::fast_route(&args.task, &args.difficulty, args.boundary_review.as_ref())
                .map_err(string_error)?,
        )
        .map_err(McpError::from)
    }

    fn tool_capability_register(&self, arguments: Value) -> McpResult<Value> {
        let request: RegisterCapability = serde_json::from_value(arguments)?;
        serde_json::to_value(
            self.engine
                .capability()
                .register(request)
                .map_err(string_error)?,
        )
        .map_err(McpError::from)
    }

    fn tool_capability_detect(&self, arguments: Value) -> McpResult<Value> {
        let args: CapabilityDetectArgs = serde_json::from_value(arguments)?;
        serde_json::to_value(
            self.engine
                .capability()
                .detect(args.candidates)
                .map_err(string_error)?,
        )
        .map_err(McpError::from)
    }

    fn tool_capability_require(&self, arguments: Value) -> McpResult<Value> {
        let args: CapabilityRequireArgs = serde_json::from_value(arguments)?;
        serde_json::to_value(
            self.engine
                .capability()
                .require(&args.name)
                .map_err(string_error)?,
        )
        .map_err(McpError::from)
    }
}

fn server_info() -> Value {
    json!({
        "name": "cordis-rs",
        "title": "CORDIS Rust Cognitive Runtime",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "Evidence-bound agent cognition, authorization and workflow control."
    })
}

fn tool_result(value: Value, is_error: bool) -> Value {
    let text = serde_json::to_string(&value).unwrap_or_else(|_| "null".to_owned());
    json!({
        "resultType": "complete",
        "content": [{"type": "text", "text": text}],
        "structuredContent": value,
        "isError": is_error,
    })
}

fn jsonrpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn string_error(error: impl std::fmt::Display) -> McpError {
    McpError::InvalidRequest(error.to_string())
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum AcceptanceInput {
    Text(String),
    Criterion(AcceptanceCriterion),
}

fn normalize_acceptance(values: Vec<AcceptanceInput>) -> Vec<AcceptanceCriterion> {
    if values.is_empty() {
        return vec![AcceptanceCriterion {
            id: "criterion-1".to_owned(),
            description: "Task result is verified with observable evidence.".to_owned(),
            required: true,
        }];
    }
    values
        .into_iter()
        .enumerate()
        .map(|(index, item)| match item {
            AcceptanceInput::Text(description) => AcceptanceCriterion {
                id: format!("criterion-{}", index + 1),
                description,
                required: true,
            },
            AcceptanceInput::Criterion(criterion) => criterion,
        })
        .collect()
}

fn default_project() -> String {
    "global".to_owned()
}
fn default_domain() -> String {
    "general".to_owned()
}
fn default_strategy() -> String {
    "default".to_owned()
}
fn default_complexity() -> f64 {
    0.5
}
fn default_limit() -> usize {
    3
}
fn default_motivation() -> String {
    "Complete the requested task with observable evidence.".to_owned()
}
fn default_action_class() -> ActionClass {
    ActionClass::Read
}

#[derive(Debug, Deserialize)]
struct BeginArgs {
    goal: String,
    #[serde(default = "default_project")]
    project_id: String,
    #[serde(default = "default_domain")]
    domain: String,
    #[serde(default = "default_strategy")]
    strategy_id: String,
    #[serde(default)]
    stakes: Stakes,
    #[serde(default = "default_complexity")]
    complexity: f64,
    #[serde(default)]
    task_id: Option<String>,
    #[serde(default)]
    current_step: Option<String>,
    #[serde(default)]
    constraints: Vec<String>,
    #[serde(default)]
    acceptance_evidence: Vec<AcceptanceInput>,
    #[serde(default)]
    unknowns: Vec<String>,
    #[serde(default)]
    authorization: AuthorizationEnvelope,
    #[serde(default)]
    scope_in: Vec<String>,
    #[serde(default)]
    scope_out: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct QueryArgs {
    task_id: String,
    intent: String,
    #[serde(default = "default_scopes")]
    scopes: Vec<MemoryScope>,
    #[serde(default = "default_kinds")]
    kinds: Vec<MemoryKind>,
    #[serde(default = "default_limit")]
    limit: usize,
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

#[derive(Debug, Deserialize)]
struct ObserveArgs {
    task_id: String,
    event_type: String,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    actual: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    expected: Option<String>,
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    error_class: Option<String>,
    #[serde(default)]
    event_id: Option<String>,
    #[serde(default)]
    scope: MemoryScope,
    #[serde(default)]
    conversation_id: Option<String>,
    #[serde(default)]
    plan_id: Option<String>,
    #[serde(default)]
    step_id: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    environment: Option<String>,
    #[serde(default)]
    uri: Option<String>,
    #[serde(default)]
    trust: MemoryTrust,
}

#[derive(Debug, Deserialize)]
struct CheckActionArgs {
    task_id: String,
    description: String,
    purpose: String,
    #[serde(default = "default_action_class")]
    action_class: ActionClass,
    #[serde(default)]
    action_id: Option<String>,
    #[serde(default)]
    action_name: Option<String>,
    #[serde(default)]
    tool: Option<String>,
    #[serde(default)]
    target: Option<String>,
    #[serde(default)]
    network_access: bool,
    #[serde(default)]
    destructive: bool,
    #[serde(default)]
    approval_granted: bool,
}

#[derive(Debug, Deserialize)]
struct WorkflowBeginArgs {
    task_id: String,
    goal: String,
    #[serde(default = "default_project")]
    project_id: String,
    #[serde(default = "default_domain")]
    domain: String,
    #[serde(default)]
    stakes: Stakes,
    #[serde(default)]
    stakeholders: Vec<String>,
    #[serde(default = "default_motivation")]
    motivation: String,
    #[serde(default)]
    scope_in: Vec<String>,
    #[serde(default)]
    scope_out: Vec<String>,
    #[serde(default)]
    authorization: AuthorizationEnvelope,
    #[serde(default)]
    constraints: Vec<String>,
    #[serde(default)]
    acceptance_evidence: Vec<AcceptanceInput>,
    #[serde(default)]
    known_facts: Vec<KnownFact>,
    #[serde(default)]
    unknowns: Vec<UnknownQuestion>,
    #[serde(default)]
    completeness: Completeness,
    #[serde(default)]
    difficulty: DifficultyInputs,
    // Flat fields preserve the Python v0.5 MCP shape. When supplied they
    // override the corresponding nested `difficulty` fields.
    #[serde(default)]
    complexity: Option<f64>,
    #[serde(default)]
    irreversibility: Option<f64>,
    #[serde(default)]
    novelty: Option<f64>,
    #[serde(default)]
    novelty_reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WorkflowAuthorizationArgs {
    workflow_id: String,
    authorization: AuthorizationEnvelope,
}
#[derive(Debug, Deserialize)]
struct WorkflowPlanArgs {
    workflow_id: String,
    plan: PlanIr,
}
#[derive(Debug, Deserialize)]
struct WorkflowApproveArgs {
    workflow_id: String,
    step_id: String,
    approved_by: String,
    #[serde(default)]
    reason: Option<String>,
}
#[derive(Debug, Deserialize)]
struct WorkflowIdArgs {
    workflow_id: String,
}
#[derive(Debug, Deserialize)]
struct WorkflowResultArgs {
    workflow_id: String,
    step_result: StepResult,
}
#[derive(Debug, Deserialize)]
struct WorkflowFinishArgs {
    workflow_id: String,
    #[serde(default)]
    outcome: Option<Outcome>,
    #[serde(default)]
    lesson: Option<String>,
    #[serde(default)]
    attribution: Option<Attribution>,
}
#[derive(Debug, Deserialize)]
struct GoalReviewArgs {
    task: TaskContract,
    difficulty: DifficultyProfile,
    #[serde(default)]
    cognitive_ir: Value,
    #[serde(default)]
    planner_enabled: bool,
}
#[derive(Debug, Deserialize)]
struct PlannerFastArgs {
    task: TaskContract,
    difficulty: DifficultyProfile,
    #[serde(default)]
    boundary_review: Option<cordis_contracts::BoundaryReview>,
}
#[derive(Debug, Deserialize)]
struct CapabilityDetectArgs {
    candidates: BTreeMap<String, ToolProbeSpec>,
}
#[derive(Debug, Deserialize)]
struct CapabilityRequireArgs {
    name: String,
}

fn tool_definitions() -> Vec<ToolDefinition> {
    let mut tools = vec![
        tool(
            "cordis_begin",
            "Start task",
            "Start a task with authorization, acceptance evidence and compact cognitive context.",
            object_schema(
                json!({
                    "goal": string(), "project_id": string(), "domain": string(), "strategy_id": string(),
                    "stakes": stakes_schema(), "complexity": number01(), "task_id": string(), "current_step": string(),
                    "constraints": string_array(), "acceptance_evidence": array_any(), "unknowns": string_array(),
                    "authorization": object(), "scope_in": string_array(), "scope_out": string_array()
                }),
                &["goal"],
            ),
            false,
        ),
        tool(
            "cordis_query",
            "Query cognition",
            "Retrieve novel project-safe reviewed or observed cognition for an active task.",
            object_schema(
                json!({"task_id": string(), "intent": string(), "scopes": string_array(), "kinds": string_array(), "limit": integer()}),
                &["task_id", "intent"],
            ),
            true,
        ),
        tool(
            "cordis_observe",
            "Record observation",
            "Record a plan, tool, test, artifact or error event for an active task.",
            object_schema(
                json!({"task_id": string(), "event_type": string(), "subject": string(), "actual": string(), "summary": string(), "expected": string(), "tool": string(), "error_class": string(), "event_id": string(), "scope": string(), "conversation_id": string(), "plan_id": string(), "step_id": string(), "model": string(), "environment": string(), "uri": string(), "trust": string()}),
                &["task_id", "event_type"],
            ),
            false,
        ),
        tool(
            "cordis_check_action",
            "Check action",
            "Return a fail-closed execution permit plus drift and destructive-action signals.",
            object_schema(
                json!({"task_id": string(), "description": string(), "purpose": string(), "action_class": string(), "action_id": string(), "action_name": string(), "tool": string(), "target": string(), "network_access": boolean(), "destructive": boolean(), "approval_granted": boolean()}),
                &["task_id", "description", "purpose"],
            ),
            true,
        ),
        tool(
            "cordis_finish",
            "Finish task",
            "Finalize exactly one task using evidence-bound outcome validation.",
            object_schema(
                json!({"task_id": string(), "outcome": enum_schema(&["success", "partial", "failure"]), "evidence": array_any(), "attribution": string(), "lesson": string(), "outcome_score": number01()}),
                &["task_id", "outcome", "evidence"],
            ),
            false,
        ),
        tool(
            "cordis_status",
            "CORDIS status",
            "Return runtime, workflow, memory, capability and store status.",
            empty_schema(),
            true,
        ),
        tool(
            "cordis_memory_remember",
            "Remember cognition",
            "Store explicitly scoped cognition with provenance, trust and instruction-safety metadata.",
            object_schema(
                json!({"kind": string(), "subject": string(), "content": string(), "scope": string(), "project_id": string(), "conversation_id": string(), "task_id": string(), "source_id": string(), "evidence": object(), "confidence": number01(), "metadata": object(), "status": string(), "trust": string(), "instruction_safe": boolean()}),
                &["kind", "subject", "content", "scope"],
            ),
            false,
        ),
        tool(
            "cordis_seed_strategy",
            "Seed strategy",
            "Register a provenance-bound strategy seed; promotion still requires repeated success.",
            object_schema(
                json!({"strategy_id": string(), "domain": string(), "project_id": string(), "prefer": string_array(), "avoid": string_array(), "source_ref": string(), "evidence_ids": string_array(), "applicability": string()}),
                &["strategy_id"],
            ),
            false,
        ),
        tool(
            "cordis_workflow_begin",
            "Begin workflow",
            "Create a durable TaskContract, difficulty profile and fail-closed workflow state.",
            object_schema(
                json!({"task_id": string(), "goal": string(), "project_id": string(), "domain": string(), "stakes": stakes_schema(), "stakeholders": string_array(), "motivation": string(), "scope_in": string_array(), "scope_out": string_array(), "authorization": object(), "constraints": string_array(), "acceptance_evidence": array_any(), "known_facts": array_any(), "unknowns": array_any(), "completeness": object(), "difficulty": object(), "complexity": number01(), "irreversibility": number01(), "novelty": number01(), "novelty_reason": string()}),
                &["task_id", "goal"],
            ),
            false,
        ),
        tool(
            "cordis_workflow_set_authorization",
            "Set workflow authorization",
            "Set a complete authorization envelope before plan activation.",
            object_schema(
                json!({"workflow_id": string(), "authorization": object()}),
                &["workflow_id", "authorization"],
            ),
            false,
        ),
        tool(
            "cordis_workflow_submit_plan",
            "Submit plan",
            "Validate and activate a provider-neutral cordis.plan.v1 document.",
            object_schema(
                json!({"workflow_id": string(), "plan": object()}),
                &["workflow_id", "plan"],
            ),
            false,
        ),
        tool(
            "cordis_workflow_approve_step",
            "Approve step",
            "Record explicit human approval for the current gated step.",
            object_schema(
                json!({"workflow_id": string(), "step_id": string(), "approved_by": string(), "reason": string()}),
                &["workflow_id", "step_id", "approved_by"],
            ),
            false,
        ),
        tool(
            "cordis_workflow_current_permit",
            "Current step permit",
            "Return the machine-readable authorization/scope/tool permit for the current step.",
            object_schema(json!({"workflow_id": string()}), &["workflow_id"]),
            true,
        ),
        tool(
            "cordis_workflow_submit_step_result",
            "Submit step result",
            "Validate evidence and advance, retry, replan or terminate the current workflow step.",
            object_schema(
                json!({"workflow_id": string(), "step_result": object()}),
                &["workflow_id", "step_result"],
            ),
            false,
        ),
        tool(
            "cordis_workflow_replan",
            "Replace plan",
            "Replace a plan only after an explicit awaiting_replan transition.",
            object_schema(
                json!({"workflow_id": string(), "plan": object()}),
                &["workflow_id", "plan"],
            ),
            false,
        ),
        tool(
            "cordis_workflow_finish",
            "Finish workflow",
            "Close a terminal workflow and feed accumulated evidence into Core learning.",
            object_schema(
                json!({"workflow_id": string(), "outcome": string(), "lesson": string(), "attribution": string()}),
                &["workflow_id"],
            ),
            false,
        ),
        tool(
            "cordis_workflow_get",
            "Get workflow",
            "Return the durable workflow snapshot and active step.",
            object_schema(json!({"workflow_id": string()}), &["workflow_id"]),
            true,
        ),
        tool(
            "cordis_goal_review",
            "Review goal boundaries",
            "Run rule-only Goal Mode boundary review with deterministic hard gates.",
            object_schema(
                json!({"task": object(), "difficulty": object(), "cognitive_ir": object(), "planner_enabled": boolean()}),
                &["task", "difficulty"],
            ),
            true,
        ),
        tool(
            "cordis_planner_fast_route",
            "Fast route",
            "Choose direct, plan, approval or authorization_required without a model call.",
            object_schema(
                json!({"task": object(), "difficulty": object(), "boundary_review": object()}),
                &["task", "difficulty"],
            ),
            true,
        ),
        tool(
            "cordis_capability_register",
            "Register capability",
            "Register a local tool declaration without installing or changing global configuration.",
            object_schema(
                json!({"name": string(), "path": string(), "version": string(), "verify_args": string_array(), "capabilities": string_array(), "scope": enum_schema(&["project", "global"])}),
                &["name", "path"],
            ),
            false,
        ),
        tool(
            "cordis_capability_detect",
            "Detect capabilities",
            "Probe a bounded set of local executables and persist observed availability.",
            object_schema(json!({"candidates": object()}), &["candidates"]),
            false,
        ),
        tool(
            "cordis_capability_require",
            "Require capability",
            "Fail closed unless a registered local tool is currently available.",
            object_schema(json!({"name": string()}), &["name"]),
            true,
        ),
        tool(
            "cordis_capability_status",
            "Capability status",
            "Return the local capability registry.",
            empty_schema(),
            true,
        ),
    ];
    tools.sort_by(|left, right| left.name.cmp(&right.name));
    tools
}

fn tool(
    name: &str,
    title: &str,
    description: &str,
    input_schema: Value,
    read_only: bool,
) -> ToolDefinition {
    ToolDefinition {
        name: name.to_owned(),
        title: title.to_owned(),
        description: description.to_owned(),
        input_schema,
        annotations: json!({
            "readOnlyHint": read_only,
            "destructiveHint": false,
            "idempotentHint": read_only,
            "openWorldHint": false
        }),
    }
}

fn object_schema(properties: Value, required: &[&str]) -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}
fn empty_schema() -> Value {
    object_schema(json!({}), &[])
}
fn string() -> Value {
    json!({"type": "string"})
}
fn boolean() -> Value {
    json!({"type": "boolean"})
}
fn integer() -> Value {
    json!({"type": "integer", "minimum": 1})
}
fn number01() -> Value {
    json!({"type": "number", "minimum": 0, "maximum": 1})
}
fn object() -> Value {
    json!({"type": "object"})
}
fn string_array() -> Value {
    json!({"type": "array", "items": {"type": "string"}})
}
fn array_any() -> Value {
    json!({"type": "array"})
}
fn enum_schema(values: &[&str]) -> Value {
    json!({"type": "string", "enum": values})
}
fn stakes_schema() -> Value {
    enum_schema(&["low", "medium", "high", "critical"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn supports_legacy_initialize_and_modern_discovery() {
        let directory = tempdir().unwrap();
        let server = CordisMcpServer::open(directory.path()).unwrap();
        let initialized = server.handle_value(json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "test", "version": "1"}}
        })).unwrap();
        assert_eq!(initialized["result"]["protocolVersion"], "2025-11-25");
        let discovered = server
            .handle_value(json!({
                "jsonrpc": "2.0", "id": 2, "method": "server/discover", "params": {
                    "_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28"}
                }
            }))
            .unwrap();
        assert!(
            discovered["result"]["supportedVersions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| item == "2026-07-28")
        );
    }

    #[test]
    fn tools_are_deterministic_and_flat_begin_is_callable() {
        let directory = tempdir().unwrap();
        let server = CordisMcpServer::open(directory.path()).unwrap();
        let listed = server
            .handle_value(json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}}))
            .unwrap();
        let names: Vec<_> = listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["name"].as_str().unwrap())
            .collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        assert_eq!(names, sorted);
        let called = server.handle_value(json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "cordis_begin", "arguments": {
                "goal": "Verify MCP", "project_id": "mcp", "domain": "software",
                "stakes": "low", "complexity": 0.1,
                "acceptance_evidence": [{"id": "verified", "description": "MCP works", "required": true}]
            }}
        })).unwrap();
        assert_eq!(called["result"]["isError"], false);
        assert_eq!(
            called["result"]["structuredContent"]["schema"],
            "cordis.runtime.v1"
        );
    }
}
