use crate::{CordisHostRuntime, RuntimeError, RuntimeResult};
use cordis_contracts::{
    Attribution, EventRecord, Evidence, EvidenceTrust, FeedbackRequest, FeedbackResult,
    MemoryScope, MemoryTrust, Outcome, PreflightRequest,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const MANAGED_SESSION_SCHEMA: &str = "cordis.managed-session.v1";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ManagedSessionStatus {
    #[default]
    New,
    Active,
    Closed,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HostEventType {
    ToolSucceeded,
    ToolFailed,
    TestPassed,
    TestFailed,
    VerificationPassed,
    VerificationFailed,
    Note,
}

impl HostEventType {
    fn observation_name(self) -> &'static str {
        match self {
            Self::ToolSucceeded => "tool_succeeded",
            Self::ToolFailed => "tool_failed",
            Self::TestPassed => "test_passed",
            Self::TestFailed => "test_failed",
            Self::VerificationPassed => "verification_passed",
            Self::VerificationFailed => "verification_failed",
            Self::Note => "note",
        }
    }

    fn evidence(self) -> Option<(&'static str, bool)> {
        match self {
            Self::ToolSucceeded => Some(("tool", true)),
            Self::ToolFailed => Some(("tool", false)),
            Self::TestPassed => Some(("test", true)),
            Self::TestFailed => Some(("test", false)),
            Self::VerificationPassed => Some(("verification", true)),
            Self::VerificationFailed => Some(("verification", false)),
            Self::Note => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HostEvent {
    #[serde(rename = "type")]
    pub event_type: HostEventType,
    pub summary: String,
    #[serde(default)]
    pub subject: Option<String>,
    #[serde(default)]
    pub acceptance_id: Option<String>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub environment: Option<String>,
    #[serde(default)]
    pub expected: Option<String>,
    #[serde(default)]
    pub error_class: Option<String>,
    #[serde(default)]
    pub uri: Option<String>,
    #[serde(default)]
    pub conversation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManagedSessionSnapshot {
    pub schema: String,
    pub status: ManagedSessionStatus,
    pub task_id: Option<String>,
    pub required_acceptance_ids: Vec<String>,
    pub evidence_count: usize,
    #[serde(default)]
    pub derived_outcome: Option<Outcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManagedRecordResult {
    pub schema: String,
    pub observation: serde_json::Value,
    pub evidence: Option<Evidence>,
    pub evidence_count: usize,
}

/// A typed host-owned task lifecycle. Models never construct low-level closure payloads.
pub struct CordisManagedSession {
    runtime: CordisHostRuntime,
    task_id: Option<String>,
    project_id: Option<String>,
    required_acceptance_ids: BTreeSet<String>,
    known_acceptance_ids: BTreeSet<String>,
    evidence: Vec<Evidence>,
    status: ManagedSessionStatus,
}

impl CordisManagedSession {
    pub fn new(runtime: CordisHostRuntime) -> Self {
        Self {
            runtime,
            task_id: None,
            project_id: None,
            required_acceptance_ids: BTreeSet::new(),
            known_acceptance_ids: BTreeSet::new(),
            evidence: Vec::new(),
            status: ManagedSessionStatus::New,
        }
    }

    pub fn status(&self) -> ManagedSessionStatus {
        self.status
    }

    pub fn task_id(&self) -> Option<&str> {
        self.task_id.as_deref()
    }

    pub fn snapshot(&self) -> ManagedSessionSnapshot {
        ManagedSessionSnapshot {
            schema: MANAGED_SESSION_SCHEMA.to_owned(),
            status: self.status,
            task_id: self.task_id.clone(),
            required_acceptance_ids: self.required_acceptance_ids.iter().cloned().collect(),
            evidence_count: self.evidence.len(),
            derived_outcome: if self.status == ManagedSessionStatus::New {
                None
            } else {
                Some(self.derive_outcome())
            },
        }
    }

    pub fn start(&mut self, request: PreflightRequest) -> RuntimeResult<crate::RuntimeBeginResult> {
        if self.status != ManagedSessionStatus::New {
            return Err(RuntimeError::Transition(
                "managed session can only be started once".to_owned(),
            ));
        }
        let result = self.runtime.begin(request)?;
        self.task_id = Some(result.task_id.clone());
        self.project_id = Some(result.focus.project_id.clone());
        self.known_acceptance_ids = result
            .cognitive_ir
            .verification
            .acceptance_evidence
            .iter()
            .map(|criterion| criterion.id.clone())
            .collect();
        self.required_acceptance_ids = result
            .cognitive_ir
            .verification
            .acceptance_evidence
            .iter()
            .filter(|criterion| criterion.required)
            .map(|criterion| criterion.id.clone())
            .collect();
        self.status = ManagedSessionStatus::Active;
        Ok(result)
    }

    pub fn record(&mut self, event: HostEvent) -> RuntimeResult<ManagedRecordResult> {
        self.require_active()?;
        if event.summary.trim().is_empty() {
            return Err(RuntimeError::Validation(
                "host event summary must be non-empty".to_owned(),
            ));
        }
        if let Some(acceptance_id) = &event.acceptance_id
            && !self.known_acceptance_ids.contains(acceptance_id)
        {
            return Err(RuntimeError::Validation(format!(
                "host event references unknown acceptance criterion: {acceptance_id}"
            )));
        }
        if event.event_type == HostEventType::Note && event.acceptance_id.is_some() {
            return Err(RuntimeError::Validation(
                "note events cannot satisfy an acceptance criterion".to_owned(),
            ));
        }

        let task_id = self.task_id.clone().ok_or_else(|| {
            RuntimeError::Transition("managed session has no active task id".to_owned())
        })?;
        let project_id = self.project_id.clone().ok_or_else(|| {
            RuntimeError::Transition("managed session has no project id".to_owned())
        })?;
        let observation = self.runtime.observe(
            &task_id,
            EventRecord {
                id: event.id.clone(),
                event_type: event.event_type.observation_name().to_owned(),
                scope: MemoryScope::Workflow,
                project_id,
                task_id: task_id.clone(),
                conversation_id: event.conversation_id.clone(),
                subject: event
                    .subject
                    .clone()
                    .unwrap_or_else(|| event.event_type.observation_name().to_owned()),
                actual: event.summary.trim().to_owned(),
                expected: event.expected.clone(),
                error_class: event.error_class.clone(),
                tool: event.tool.clone(),
                model: event.model.clone(),
                environment: event.environment.clone(),
                uri: event.uri.clone(),
                plan_id: None,
                step_id: None,
                trust: MemoryTrust::Observed,
            },
        )?;

        let evidence_id = event.id.clone();
        let evidence = event.event_type.evidence().map(|(kind, passed)| Evidence {
            id: evidence_id.clone(),
            kind: kind.to_owned(),
            summary: event.summary.trim().to_owned(),
            passed,
            uri: event.uri,
            acceptance_id: event.acceptance_id,
            source_id: evidence_id,
            trust: EvidenceTrust::Observed,
        });
        if let Some(item) = &evidence {
            item.validate()?;
            self.evidence.push(item.clone());
        }
        Ok(ManagedRecordResult {
            schema: MANAGED_SESSION_SCHEMA.to_owned(),
            observation,
            evidence,
            evidence_count: self.evidence.len(),
        })
    }

    pub fn complete(
        &mut self,
        lesson: Option<String>,
        attribution: Option<Attribution>,
        asserted_outcome: Option<Outcome>,
    ) -> RuntimeResult<FeedbackResult> {
        self.require_active()?;
        if self.evidence.is_empty() {
            return Err(RuntimeError::Validation(
                "managed session cannot complete without observable evidence".to_owned(),
            ));
        }
        if lesson.as_ref().is_some_and(|value| value.trim().is_empty()) {
            return Err(RuntimeError::Validation(
                "lesson must be non-empty when provided".to_owned(),
            ));
        }
        let derived = self.derive_outcome();
        if asserted_outcome.is_some_and(|outcome| outcome != derived) {
            return Err(RuntimeError::Validation(format!(
                "asserted outcome conflicts with evidence; expected {derived:?}"
            )));
        }
        let task_id = self.task_id.clone().ok_or_else(|| {
            RuntimeError::Transition("managed session has no active task id".to_owned())
        })?;
        let result = self.runtime.finish(FeedbackRequest {
            task_id,
            outcome: derived,
            attribution,
            lesson: lesson.map(|value| value.trim().to_owned()),
            evidence: self.evidence.clone(),
            outcome_score: None,
        })?;
        self.status = ManagedSessionStatus::Closed;
        Ok(result)
    }

    fn derive_outcome(&self) -> Outcome {
        let passed: Vec<_> = self.evidence.iter().filter(|item| item.passed).collect();
        let has_failed = self.evidence.iter().any(|item| !item.passed);
        let passed_acceptance: BTreeSet<_> = passed
            .iter()
            .filter_map(|item| item.acceptance_id.clone())
            .collect();
        let all_required = self.required_acceptance_ids.is_subset(&passed_acceptance);
        if all_required && !passed.is_empty() && !has_failed {
            Outcome::Success
        } else if !passed.is_empty() {
            Outcome::Partial
        } else {
            Outcome::Failure
        }
    }

    fn require_active(&self) -> RuntimeResult<()> {
        if self.status == ManagedSessionStatus::Active && self.task_id.is_some() {
            Ok(())
        } else {
            Err(RuntimeError::Transition(
                "managed session is not active; call start first".to_owned(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cordis_contracts::{AcceptanceCriterion, AuthorizationEnvelope, CoreTask, Stakes};
    use cordis_store::CordisStore;
    use tempfile::tempdir;

    fn runtime() -> CordisHostRuntime {
        let path = tempdir().unwrap().keep().join("managed.db");
        CordisHostRuntime::new(CordisStore::open(path).unwrap())
    }

    fn request() -> PreflightRequest {
        PreflightRequest {
            task: CoreTask {
                id: None,
                goal: "Verify a managed lifecycle".to_owned(),
                domain: "software".to_owned(),
                project_id: "managed".to_owned(),
                strategy_id: "verify".to_owned(),
                stakes: Stakes::Low,
                authorization: AuthorizationEnvelope::default(),
            },
            complexity: 0.1,
            unknowns: vec![],
            constraints: vec![],
            current_step: None,
            acceptance_evidence: vec![AcceptanceCriterion {
                id: "tests".to_owned(),
                description: "tests pass".to_owned(),
                required: true,
            }],
        }
    }

    #[test]
    fn derives_success_only_from_bound_evidence() {
        let mut session = CordisManagedSession::new(runtime());
        session.start(request()).unwrap();
        session
            .record(HostEvent {
                event_type: HostEventType::TestPassed,
                summary: "cargo test passed".to_owned(),
                subject: None,
                acceptance_id: Some("tests".to_owned()),
                id: Some("run-1".to_owned()),
                tool: Some("cargo".to_owned()),
                model: None,
                environment: None,
                expected: None,
                error_class: None,
                uri: None,
                conversation_id: None,
            })
            .unwrap();
        let result = session
            .complete(Some("Tests passed.".to_owned()), None, None)
            .unwrap();
        assert_eq!(result.event.outcome, Outcome::Success);
        assert_eq!(session.status(), ManagedSessionStatus::Closed);
    }

    #[test]
    fn incomplete_acceptance_is_partial() {
        let mut request = request();
        request.acceptance_evidence.push(AcceptanceCriterion {
            id: "artifact".to_owned(),
            description: "artifact exists".to_owned(),
            required: true,
        });
        let mut session = CordisManagedSession::new(runtime());
        session.start(request).unwrap();
        session
            .record(HostEvent {
                event_type: HostEventType::TestPassed,
                summary: "tests passed".to_owned(),
                subject: None,
                acceptance_id: Some("tests".to_owned()),
                id: None,
                tool: None,
                model: None,
                environment: None,
                expected: None,
                error_class: None,
                uri: None,
                conversation_id: None,
            })
            .unwrap();
        let result = session.complete(None, None, None).unwrap();
        assert_eq!(result.event.outcome, Outcome::Partial);
    }
}
