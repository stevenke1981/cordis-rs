use crate::{
    AUTHORIZATION_SCHEMA, ContractError, ContractResult, DIFFICULTY_PROFILE_SCHEMA,
    TASK_CONTRACT_SCHEMA, validate_text, validate_texts,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum Stakes {
    Low,
    #[default]
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthorizationStatus {
    #[default]
    Pending,
    Granted,
    Denied,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum NetworkProfile {
    #[default]
    Offline,
    ReadOnly,
    AuthorizedTargetsOnly,
    Unrestricted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AuthorizationEnvelope {
    pub schema: String,
    pub status: AuthorizationStatus,
    pub basis: String,
    pub network_profile: NetworkProfile,
    pub allowed_actions: Vec<String>,
    pub denied_actions: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub denied_tools: Vec<String>,
    pub allowed_targets: Vec<String>,
    pub denied_targets: Vec<String>,
    pub grant_id: Option<String>,
    pub approved_by: Option<String>,
    pub expires_at: Option<String>,
}

impl Default for AuthorizationEnvelope {
    fn default() -> Self {
        Self {
            schema: AUTHORIZATION_SCHEMA.to_owned(),
            status: AuthorizationStatus::Pending,
            basis: String::new(),
            network_profile: NetworkProfile::Offline,
            allowed_actions: Vec::new(),
            denied_actions: Vec::new(),
            allowed_tools: Vec::new(),
            denied_tools: Vec::new(),
            allowed_targets: Vec::new(),
            denied_targets: Vec::new(),
            grant_id: None,
            approved_by: None,
            expires_at: None,
        }
    }
}

impl AuthorizationEnvelope {
    pub fn validate(&self) -> ContractResult<()> {
        if self.schema != AUTHORIZATION_SCHEMA {
            return Err(ContractError::Unsupported {
                field: "authorization.schema",
                value: self.schema.clone(),
            });
        }
        validate_texts(
            &self.allowed_actions,
            "authorization.allowed_actions",
            100,
            200,
        )?;
        validate_texts(
            &self.denied_actions,
            "authorization.denied_actions",
            100,
            200,
        )?;
        validate_texts(&self.allowed_tools, "authorization.allowed_tools", 100, 200)?;
        validate_texts(&self.denied_tools, "authorization.denied_tools", 100, 200)?;
        validate_texts(
            &self.allowed_targets,
            "authorization.allowed_targets",
            100,
            500,
        )?;
        validate_texts(
            &self.denied_targets,
            "authorization.denied_targets",
            100,
            500,
        )?;
        ensure_disjoint(
            &self.allowed_actions,
            &self.denied_actions,
            "authorization.allowed_actions",
            "authorization.denied_actions",
        )?;
        ensure_disjoint(
            &self.allowed_tools,
            &self.denied_tools,
            "authorization.allowed_tools",
            "authorization.denied_tools",
        )?;
        ensure_disjoint(
            &self.allowed_targets,
            &self.denied_targets,
            "authorization.allowed_targets",
            "authorization.denied_targets",
        )?;
        if self.basis.chars().count() > 1_000 {
            return Err(ContractError::TooLong {
                field: "authorization.basis",
                max: 1_000,
            });
        }
        if let Some(grant_id) = &self.grant_id {
            validate_text(grant_id, "authorization.grant_id", 200)?;
        }
        if let Some(approved_by) = &self.approved_by {
            validate_text(approved_by, "authorization.approved_by", 200)?;
        }
        if let Some(expires_at) = &self.expires_at {
            validate_utc_timestamp(expires_at)?;
        }
        if self.status == AuthorizationStatus::Granted && self.basis.trim().is_empty() {
            return Err(ContractError::Inconsistent {
                field: "authorization",
                reason: "granted authorization requires a basis".to_owned(),
            });
        }
        Ok(())
    }
}

fn validate_utc_timestamp(value: &str) -> ContractResult<()> {
    const FIELD: &str = "authorization.expires_at";
    validate_text(value, FIELD, 20)?;
    let bytes = value.as_bytes();
    let separators = [
        (4, b'-'),
        (7, b'-'),
        (10, b'T'),
        (13, b':'),
        (16, b':'),
        (19, b'Z'),
    ];
    if bytes.len() != 20
        || separators
            .iter()
            .any(|(index, expected)| bytes[*index] != *expected)
        || bytes.iter().enumerate().any(|(index, byte)| {
            !separators.iter().any(|(separator, _)| *separator == index) && !byte.is_ascii_digit()
        })
    {
        return Err(ContractError::Inconsistent {
            field: FIELD,
            reason: "must use canonical UTC RFC3339 form YYYY-MM-DDTHH:MM:SSZ".to_owned(),
        });
    }
    let parse =
        |start: usize, end: usize| -> u32 { value[start..end].parse::<u32>().unwrap_or_default() };
    let year = parse(0, 4);
    let month = parse(5, 7);
    let day = parse(8, 10);
    let hour = parse(11, 13);
    let minute = parse(14, 16);
    let second = parse(17, 19);
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    if year == 0
        || max_day == 0
        || day == 0
        || day > max_day
        || hour > 23
        || minute > 59
        || second > 59
    {
        return Err(ContractError::Inconsistent {
            field: FIELD,
            reason: "contains an invalid UTC calendar timestamp".to_owned(),
        });
    }
    Ok(())
}

fn ensure_disjoint(
    left: &[String],
    right: &[String],
    field: &'static str,
    other: &'static str,
) -> ContractResult<()> {
    let right: HashSet<_> = right.iter().map(|value| value.to_lowercase()).collect();
    for value in left {
        if right.contains(&value.to_lowercase()) {
            return Err(ContractError::Overlap {
                field,
                other,
                value: value.clone(),
            });
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct TaskScope {
    #[serde(rename = "in")]
    pub included: Vec<String>,
    #[serde(rename = "out")]
    pub excluded: Vec<String>,
}

impl TaskScope {
    pub fn validate(&self) -> ContractResult<()> {
        validate_texts(&self.included, "scope.in", 100, 500)?;
        validate_texts(&self.excluded, "scope.out", 100, 500)?;
        ensure_disjoint(&self.included, &self.excluded, "scope.in", "scope.out")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AcceptanceCriterion {
    pub id: String,
    #[serde(alias = "summary")]
    pub description: String,
    #[serde(default = "default_true")]
    pub required: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct KnownFact {
    pub claim: String,
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnknownQuestion {
    pub question: String,
    #[serde(default = "default_true")]
    pub material: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Completeness {
    pub score: f64,
    #[serde(default)]
    pub blocking_gaps: Vec<String>,
}

impl Default for Completeness {
    fn default() -> Self {
        Self {
            score: 1.0,
            blocking_gaps: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TaskContract {
    pub schema: String,
    pub task_id: String,
    pub goal: String,
    pub project_id: String,
    pub domain: String,
    pub stakes: Stakes,
    #[serde(default)]
    pub stakeholders: Vec<String>,
    pub motivation: String,
    pub scope: TaskScope,
    #[serde(default)]
    pub authorization: AuthorizationEnvelope,
    #[serde(default)]
    pub constraints: Vec<String>,
    pub acceptance_evidence: Vec<AcceptanceCriterion>,
    #[serde(default)]
    pub known_facts: Vec<KnownFact>,
    #[serde(default)]
    pub unknowns: Vec<UnknownQuestion>,
    #[serde(default)]
    pub completeness: Completeness,
}

impl TaskContract {
    pub fn validate(&self) -> ContractResult<()> {
        if self.schema != TASK_CONTRACT_SCHEMA {
            return Err(ContractError::Unsupported {
                field: "task.schema",
                value: self.schema.clone(),
            });
        }
        validate_text(&self.task_id, "task_id", 200)?;
        validate_text(&self.goal, "goal", 4_000)?;
        validate_text(&self.project_id, "project_id", 200)?;
        validate_text(&self.domain, "domain", 100)?;
        validate_text(&self.motivation, "motivation", 4_000)?;
        validate_texts(&self.stakeholders, "stakeholders", 100, 500)?;
        validate_texts(&self.constraints, "constraints", 100, 1_000)?;
        self.scope.validate()?;
        self.authorization.validate()?;
        if self.acceptance_evidence.is_empty() {
            return Err(ContractError::Missing {
                field: "acceptance_evidence",
            });
        }
        let mut ids = HashSet::new();
        for criterion in &self.acceptance_evidence {
            validate_text(&criterion.id, "acceptance_evidence.id", 200)?;
            validate_text(
                &criterion.description,
                "acceptance_evidence.description",
                4_000,
            )?;
            if !ids.insert(criterion.id.as_str()) {
                return Err(ContractError::Duplicate {
                    field: "acceptance_evidence.id",
                    value: criterion.id.clone(),
                });
            }
        }
        for fact in &self.known_facts {
            validate_text(&fact.claim, "known_facts.claim", 4_000)?;
            if fact.evidence_ids.is_empty() {
                return Err(ContractError::Missing {
                    field: "known_facts.evidence_ids",
                });
            }
            validate_texts(&fact.evidence_ids, "known_facts.evidence_ids", 100, 500)?;
        }
        for unknown in &self.unknowns {
            validate_text(&unknown.question, "unknowns.question", 4_000)?;
        }
        if !(0.0..=1.0).contains(&self.completeness.score) || self.completeness.score.is_nan() {
            return Err(ContractError::Inconsistent {
                field: "completeness.score",
                reason: "score must be between 0 and 1".to_owned(),
            });
        }
        validate_texts(
            &self.completeness.blocking_gaps,
            "completeness.blocking_gaps",
            100,
            1_000,
        )?;
        Ok(())
    }

    pub fn required_acceptance_ids(&self) -> HashSet<String> {
        self.acceptance_evidence
            .iter()
            .filter(|criterion| criterion.required)
            .map(|criterion| criterion.id.clone())
            .collect()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DifficultyAxis {
    Complexity,
    Uncertainty,
    Impact,
    Irreversibility,
    Novelty,
    EvidenceDeficit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DifficultyComponent {
    pub score: f64,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, Default)]
#[serde(rename_all = "snake_case")]
pub enum ControlMode {
    Fast,
    #[default]
    Advisory,
    HighIntervention,
    Takeover,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DifficultyComponents {
    pub complexity: DifficultyComponent,
    pub uncertainty: DifficultyComponent,
    pub impact: DifficultyComponent,
    pub irreversibility: DifficultyComponent,
    pub novelty: DifficultyComponent,
    pub evidence_deficit: DifficultyComponent,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DifficultyProfile {
    pub schema: String,
    pub components: DifficultyComponents,
    pub control_mode: ControlMode,
    pub override_reason: Option<String>,
    pub aggregate_score: Option<f64>,
    #[serde(default)]
    pub policy_reasons: Vec<String>,
}

impl DifficultyProfile {
    pub fn validate(&self) -> ContractResult<()> {
        if self.schema != DIFFICULTY_PROFILE_SCHEMA {
            return Err(ContractError::Unsupported {
                field: "difficulty.schema",
                value: self.schema.clone(),
            });
        }
        let components = [
            ("components.complexity", &self.components.complexity),
            ("components.uncertainty", &self.components.uncertainty),
            ("components.impact", &self.components.impact),
            (
                "components.irreversibility",
                &self.components.irreversibility,
            ),
            ("components.novelty", &self.components.novelty),
            (
                "components.evidence_deficit",
                &self.components.evidence_deficit,
            ),
        ];
        for (field, component) in components {
            if !(0.0..=1.0).contains(&component.score) || component.score.is_nan() {
                return Err(ContractError::Inconsistent {
                    field: "difficulty.components",
                    reason: format!("{field}.score must be between 0 and 1"),
                });
            }
            if component.reasons.is_empty() {
                return Err(ContractError::Inconsistent {
                    field: "difficulty.components",
                    reason: format!("{field}.reasons must not be empty"),
                });
            }
        }
        if let Some(score) = self.aggregate_score
            && (!(0.0..=1.0).contains(&score) || score.is_nan())
        {
            return Err(ContractError::Inconsistent {
                field: "difficulty.aggregate_score",
                reason: "score must be between 0 and 1".to_owned(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contract() -> TaskContract {
        TaskContract {
            schema: TASK_CONTRACT_SCHEMA.to_owned(),
            task_id: "task-1".to_owned(),
            goal: "Verify a contract".to_owned(),
            project_id: "cordis".to_owned(),
            domain: "software".to_owned(),
            stakes: Stakes::Low,
            stakeholders: vec![],
            motivation: "Test".to_owned(),
            scope: TaskScope {
                included: vec!["core".to_owned()],
                excluded: vec!["prod".to_owned()],
            },
            authorization: AuthorizationEnvelope::default(),
            constraints: vec![],
            acceptance_evidence: vec![AcceptanceCriterion {
                id: "test".to_owned(),
                description: "test passes".to_owned(),
                required: true,
            }],
            known_facts: vec![],
            unknowns: vec![],
            completeness: Completeness::default(),
        }
    }

    #[test]
    fn valid_contract_passes() {
        contract().validate().unwrap();
    }

    #[test]
    fn overlapping_authorization_is_rejected() {
        let mut value = contract();
        value
            .authorization
            .allowed_actions
            .push("delete".to_owned());
        value.authorization.denied_actions.push("DELETE".to_owned());
        assert!(value.validate().is_err());
    }

    #[test]
    fn authorization_expiry_requires_canonical_valid_utc() {
        let mut value = contract();
        value.authorization.expires_at = Some("2026-02-30T00:00:00Z".to_owned());
        assert!(value.validate().is_err());
        value.authorization.expires_at = Some("2028-02-29T23:59:59Z".to_owned());
        value.validate().unwrap();
    }
}
