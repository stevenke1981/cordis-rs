mod setup;

use clap::{Parser, Subcommand, ValueEnum};
use cordis_contracts::{PlanIr, StepResult, TaskContract};
use cordis_mcp::CordisMcpServer;
use serde_json::{Value, json};
use std::fs;
use std::io::{self, Read};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "cordis", version, about = "CORDIS Rust JSON CLI")]
struct Args {
    #[arg(long, default_value = ".cordis", global = true)]
    data_dir: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Init,
    Status,
    Begin {
        #[arg(default_value = "-")]
        input: String,
    },
    Query {
        #[arg(default_value = "-")]
        input: String,
    },
    Observe {
        #[arg(default_value = "-")]
        input: String,
    },
    CheckAction {
        #[arg(default_value = "-")]
        input: String,
    },
    Finish {
        #[arg(default_value = "-")]
        input: String,
    },
    MemoryRemember {
        #[arg(default_value = "-")]
        input: String,
    },
    SeedStrategy {
        #[arg(default_value = "-")]
        input: String,
    },
    WorkflowBegin {
        #[arg(default_value = "-")]
        input: String,
    },
    WorkflowAuthorize {
        #[arg(default_value = "-")]
        input: String,
    },
    WorkflowSubmitPlan {
        #[arg(default_value = "-")]
        input: String,
    },
    WorkflowApproveStep {
        #[arg(default_value = "-")]
        input: String,
    },
    WorkflowPermit {
        #[arg(default_value = "-")]
        input: String,
    },
    WorkflowSubmitResult {
        #[arg(default_value = "-")]
        input: String,
    },
    WorkflowReplan {
        #[arg(default_value = "-")]
        input: String,
    },
    WorkflowFinish {
        #[arg(default_value = "-")]
        input: String,
    },
    WorkflowGet {
        #[arg(default_value = "-")]
        input: String,
    },
    GoalReview {
        #[arg(default_value = "-")]
        input: String,
    },
    PlannerFastRoute {
        #[arg(default_value = "-")]
        input: String,
    },
    CapabilityRegister {
        #[arg(default_value = "-")]
        input: String,
    },
    CapabilityDetect {
        #[arg(default_value = "-")]
        input: String,
    },
    CapabilityRequire {
        #[arg(default_value = "-")]
        input: String,
    },
    CapabilityStatus,
    Call {
        tool: String,
        #[arg(default_value = "-")]
        input: String,
    },
    Validate {
        kind: ContractKind,
        #[arg(default_value = "-")]
        input: String,
    },
    Setup {
        host: SetupHost,
        #[arg(long)]
        state_dir: Option<PathBuf>,
    },
    MigratePython {
        source_dir: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ContractKind {
    Task,
    Plan,
    StepResult,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum SetupHost {
    Codex,
    ClaudeCode,
    Opencode,
    Hermes,
    All,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("cordis: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let Args { data_dir, command } = Args::parse();
    let output = match command {
        Command::Setup { host, state_dir } => match host {
            SetupHost::All => setup::setup_all(),
            SetupHost::Codex => setup::setup(setup::HostKind::Codex, state_dir)?,
            SetupHost::ClaudeCode => setup::setup(setup::HostKind::ClaudeCode, state_dir)?,
            SetupHost::Opencode => setup::setup(setup::HostKind::OpenCode, state_dir)?,
            SetupHost::Hermes => setup::setup(setup::HostKind::Hermes, state_dir)?,
        },
        Command::Validate { kind, input } => validate(kind, read_json(&input)?)?,
        Command::MigratePython { source_dir } => {
            let engine = cordis_sdk::CordisEngine::open(&data_dir)?;
            serde_json::to_value(cordis_sdk::migrate_python_v05(&engine, source_dir)?)?
        }
        command => {
            let server = CordisMcpServer::open(&data_dir)?;
            let (tool, input) = command_tool(command)?;
            call(&server, &tool, input.as_deref())?
        }
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn command_tool(command: Command) -> Result<(String, Option<String>), Box<dyn std::error::Error>> {
    let (tool, input) = match command {
        Command::Init | Command::Status => ("cordis_status".to_owned(), None),
        Command::Begin { input } => ("cordis_begin".to_owned(), Some(input)),
        Command::Query { input } => ("cordis_query".to_owned(), Some(input)),
        Command::Observe { input } => ("cordis_observe".to_owned(), Some(input)),
        Command::CheckAction { input } => ("cordis_check_action".to_owned(), Some(input)),
        Command::Finish { input } => ("cordis_finish".to_owned(), Some(input)),
        Command::MemoryRemember { input } => ("cordis_memory_remember".to_owned(), Some(input)),
        Command::SeedStrategy { input } => ("cordis_seed_strategy".to_owned(), Some(input)),
        Command::WorkflowBegin { input } => ("cordis_workflow_begin".to_owned(), Some(input)),
        Command::WorkflowAuthorize { input } => {
            ("cordis_workflow_set_authorization".to_owned(), Some(input))
        }
        Command::WorkflowSubmitPlan { input } => {
            ("cordis_workflow_submit_plan".to_owned(), Some(input))
        }
        Command::WorkflowApproveStep { input } => {
            ("cordis_workflow_approve_step".to_owned(), Some(input))
        }
        Command::WorkflowPermit { input } => {
            ("cordis_workflow_current_permit".to_owned(), Some(input))
        }
        Command::WorkflowSubmitResult { input } => {
            ("cordis_workflow_submit_step_result".to_owned(), Some(input))
        }
        Command::WorkflowReplan { input } => ("cordis_workflow_replan".to_owned(), Some(input)),
        Command::WorkflowFinish { input } => ("cordis_workflow_finish".to_owned(), Some(input)),
        Command::WorkflowGet { input } => ("cordis_workflow_get".to_owned(), Some(input)),
        Command::GoalReview { input } => ("cordis_goal_review".to_owned(), Some(input)),
        Command::PlannerFastRoute { input } => {
            ("cordis_planner_fast_route".to_owned(), Some(input))
        }
        Command::CapabilityRegister { input } => {
            ("cordis_capability_register".to_owned(), Some(input))
        }
        Command::CapabilityDetect { input } => ("cordis_capability_detect".to_owned(), Some(input)),
        Command::CapabilityRequire { input } => {
            ("cordis_capability_require".to_owned(), Some(input))
        }
        Command::CapabilityStatus => ("cordis_capability_status".to_owned(), None),
        Command::Call { tool, input } => (tool, Some(input)),
        Command::Setup { .. } | Command::Validate { .. } | Command::MigratePython { .. } => {
            unreachable!()
        }
    };
    Ok((tool, input))
}

fn call(
    server: &CordisMcpServer,
    tool: &str,
    input: Option<&str>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let arguments = match input {
        Some(path) => read_json(path)?,
        None => json!({}),
    };
    let response = server
        .handle_value(json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": tool, "arguments": arguments}
        }))
        .ok_or("MCP server returned no response")?;
    if let Some(error) = response.get("error") {
        return Err(format!("MCP protocol error: {error}").into());
    }
    let result = &response["result"];
    if result["isError"].as_bool().unwrap_or(false) {
        return Err(format!("tool execution failed: {}", result["structuredContent"]).into());
    }
    Ok(result["structuredContent"].clone())
}

fn read_json(path: &str) -> Result<Value, Box<dyn std::error::Error>> {
    let text = if path == "-" {
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        input
    } else {
        fs::read_to_string(path)?
    };
    if text.trim().is_empty() {
        Ok(json!({}))
    } else {
        Ok(serde_json::from_str(&text)?)
    }
}

fn validate(kind: ContractKind, value: Value) -> Result<Value, Box<dyn std::error::Error>> {
    match kind {
        ContractKind::Task => {
            let contract: TaskContract = serde_json::from_value(value)?;
            contract.validate()?;
            Ok(json!({"valid": true, "schema": contract.schema, "task_id": contract.task_id}))
        }
        ContractKind::Plan => {
            let contract: PlanIr = serde_json::from_value(value)?;
            contract.validate()?;
            Ok(json!({"valid": true, "schema": contract.schema, "plan_id": contract.plan_id}))
        }
        ContractKind::StepResult => {
            let contract: StepResult = serde_json::from_value(value)?;
            contract.validate()?;
            Ok(json!({"valid": true, "schema": contract.schema, "step_id": contract.step_id}))
        }
    }
}
