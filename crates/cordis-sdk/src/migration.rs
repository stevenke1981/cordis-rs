use crate::{CordisEngine, SdkError, SdkResult};
use cordis_store::StoreError;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LegacyMigrationReport {
    pub schema: String,
    pub source_dir: String,
    pub tasks_imported: usize,
    pub tasks_skipped: usize,
    pub domains_imported: usize,
    pub strategies_imported: usize,
    pub episodes_imported: usize,
    pub patterns_imported: usize,
    pub workflows_imported: usize,
    pub active_focus_skipped: usize,
    pub cognition: Option<Value>,
    pub warnings: Vec<String>,
}

pub fn migrate_python_v05(
    engine: &CordisEngine,
    source_dir: impl AsRef<Path>,
) -> SdkResult<LegacyMigrationReport> {
    let source_dir = source_dir.as_ref();
    if !source_dir.is_dir() {
        return Err(SdkError::Validation(format!(
            "legacy source directory does not exist: {}",
            source_dir.display()
        )));
    }
    let mut report = LegacyMigrationReport {
        schema: "cordis.legacy-migration.v1".to_owned(),
        source_dir: source_dir.display().to_string(),
        tasks_imported: 0,
        tasks_skipped: 0,
        domains_imported: 0,
        strategies_imported: 0,
        episodes_imported: 0,
        patterns_imported: 0,
        workflows_imported: 0,
        active_focus_skipped: 0,
        cognition: None,
        warnings: Vec::new(),
    };

    let state_path = source_dir.join("state.json");
    if state_path.is_file() {
        import_state(engine, &state_path, &mut report)?;
    } else {
        report.warnings.push("state.json not found".to_owned());
    }

    let cognition_path = source_dir.join("cognition.db");
    if cognition_path.is_file() {
        report.cognition = Some(engine.store().import_legacy_cognition(&cognition_path)?);
    } else {
        report.warnings.push("cognition.db not found".to_owned());
    }

    let workflow_path = source_dir.join("workflow.json");
    if workflow_path.is_file() {
        import_workflows(engine, &workflow_path, &mut report)?;
    }

    let focus_path = source_dir.join("focus.json");
    if focus_path.is_file() {
        let value: Value = serde_json::from_str(&fs::read_to_string(&focus_path)?)?;
        report.active_focus_skipped = value
            .get("tasks")
            .and_then(Value::as_object)
            .map_or(0, serde_json::Map::len);
        if report.active_focus_skipped > 0 {
            report.warnings.push(
                "active Python focus records were not resumed; close or restart those tasks explicitly"
                    .to_owned(),
            );
        }
    }

    engine
        .store()
        .audit("legacy_python_migration", None, &report)?;
    Ok(report)
}

fn import_state(
    engine: &CordisEngine,
    path: &Path,
    report: &mut LegacyMigrationReport,
) -> SdkResult<()> {
    let state: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    if let Some(domains) = state.get("domains").and_then(Value::as_object) {
        for (key, payload) in domains {
            engine.store().upsert_domain(key, payload)?;
            report.domains_imported += 1;
        }
    }
    if let Some(strategies) = state.get("strategies").and_then(Value::as_object) {
        for (key, payload) in strategies {
            let project_id = payload
                .get("project_id")
                .and_then(Value::as_str)
                .unwrap_or("global");
            let domain = payload
                .get("domain")
                .and_then(Value::as_str)
                .unwrap_or("general");
            let strategy_id = payload
                .get("strategy_id")
                .and_then(Value::as_str)
                .unwrap_or("default");
            engine
                .store()
                .upsert_strategy(key, project_id, domain, strategy_id, payload)?;
            report.strategies_imported += 1;
        }
    }
    if let Some(tasks) = state.get("tasks").and_then(Value::as_object) {
        for (task_id, record) in tasks {
            let task = record.get("task").cloned().unwrap_or_else(|| json!({}));
            let project_id = task
                .get("project_id")
                .and_then(Value::as_str)
                .unwrap_or("global");
            let domain = task
                .get("domain")
                .and_then(Value::as_str)
                .unwrap_or("general");
            let strategy_id = task
                .get("strategy_id")
                .and_then(Value::as_str)
                .unwrap_or("default");
            let cognitive = record
                .get("cognitive_ir")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let prediction = record
                .get("prediction")
                .cloned()
                .unwrap_or_else(|| json!({}));
            match engine.store().insert_task(
                task_id,
                project_id,
                domain,
                strategy_id,
                &task,
                &cognitive,
                &prediction,
            ) {
                Ok(()) => {
                    report.tasks_imported += 1;
                    if let Some(feedback) = record
                        .get("feedback")
                        .and_then(Value::as_array)
                        .and_then(|items| items.last())
                    {
                        let feedback_id = feedback
                            .get("id")
                            .and_then(Value::as_str)
                            .map_or_else(|| format!("legacy-feedback-{task_id}"), str::to_owned);
                        engine
                            .store()
                            .finalize_task(task_id, &feedback_id, feedback)?;
                    }
                }
                Err(StoreError::AlreadyExists(_)) => report.tasks_skipped += 1,
                Err(error) => return Err(error.into()),
            }
        }
    }
    if let Some(episodes) = state.get("episodes").and_then(Value::as_array) {
        for episode in episodes {
            let Some(id) = episode.get("id").and_then(Value::as_str) else {
                continue;
            };
            let project_id = episode
                .get("project_id")
                .and_then(Value::as_str)
                .unwrap_or("global");
            let domain = episode
                .get("domain")
                .and_then(Value::as_str)
                .unwrap_or("general");
            let goal = episode
                .get("goal")
                .and_then(Value::as_str)
                .unwrap_or("legacy task");
            match engine
                .store()
                .insert_episode(id, project_id, domain, goal, episode)
            {
                Ok(()) => report.episodes_imported += 1,
                Err(StoreError::Sqlite(error))
                    if error.sqlite_error_code()
                        == Some(rusqlite::ErrorCode::ConstraintViolation) => {}
                Err(error) => return Err(error.into()),
            }
        }
    }
    if let Some(patterns) = state.get("world_patterns").and_then(Value::as_array) {
        for pattern in patterns {
            let Some(id) = pattern.get("id").and_then(Value::as_str) else {
                continue;
            };
            let project_id = pattern
                .get("project_id")
                .and_then(Value::as_str)
                .unwrap_or("global");
            let domain = pattern
                .get("domain")
                .and_then(Value::as_str)
                .unwrap_or("general");
            let Some(statement) = pattern.get("statement").and_then(Value::as_str) else {
                continue;
            };
            let evidence_count = pattern
                .get("evidence_count")
                .and_then(Value::as_u64)
                .unwrap_or(1);
            let sources: Vec<_> = (1..=evidence_count)
                .map(|index| format!("legacy-pattern-{id}-{index}"))
                .collect();
            engine
                .store()
                .upsert_world_pattern(id, project_id, domain, statement, &sources, pattern)?;
            report.patterns_imported += 1;
        }
    }
    Ok(())
}

fn import_workflows(
    engine: &CordisEngine,
    path: &Path,
    report: &mut LegacyMigrationReport,
) -> SdkResult<()> {
    let state: Value = serde_json::from_str(&fs::read_to_string(path)?)?;
    if let Some(workflows) = state.get("workflows").and_then(Value::as_object) {
        for (workflow_id, record) in workflows {
            let task_id = record
                .get("task_id")
                .and_then(Value::as_str)
                .unwrap_or_else(|| workflow_id.strip_prefix("workflow:").unwrap_or(workflow_id));
            let status = record
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("blocked");
            engine
                .store()
                .save_workflow(workflow_id, task_id, status, record)?;
            report.workflows_imported += 1;
        }
    }
    Ok(())
}
