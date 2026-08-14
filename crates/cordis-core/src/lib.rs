//! Evidence-bound prediction, attribution, learning, strategy promotion and world patterns.

use cordis_contracts::{
    AcceptanceCriterion, Attribution, AuthorizationStatus, CognitiveIr, CognitiveState, CoreTask,
    DomainState, Escalation, Evidence, FeedbackEvent, FeedbackRequest, FeedbackResult, Outcome,
    Prediction, PreflightRequest, RelevantEpisode, RelevantWorldPattern, Stakes, StrategyAdvice,
    StrategyEvidence, StrategyPromotionStatus, StrategySeed, StrategyState, StrategyStatus,
    VerificationContract, new_id, now_rfc3339,
};
use cordis_store::{CordisStore, JsonTable, StoreError};
use rusqlite::params;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use thiserror::Error;

const MAX_EPISODES: usize = 500;
const WORLD_PATTERN_MIN_SOURCES: usize = 2;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Contract(#[from] cordis_contracts::ContractError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("invalid core request: {0}")]
    Validation(String),
    #[error("feedback conflicts with observable evidence: {0}")]
    EvidenceConflict(String),
}

pub type CoreResult<T> = Result<T, CoreError>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SeedStrategyRequest {
    pub strategy_id: String,
    #[serde(default = "default_domain")]
    pub domain: String,
    #[serde(default = "default_project")]
    pub project_id: String,
    #[serde(default)]
    pub prefer: Vec<String>,
    #[serde(default)]
    pub avoid: Vec<String>,
    #[serde(default)]
    pub source_ref: String,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    #[serde(default)]
    pub applicability: String,
}

fn default_domain() -> String {
    "general".to_owned()
}
fn default_project() -> String {
    "global".to_owned()
}

#[derive(Clone)]
pub struct CordisCore {
    store: CordisStore,
}

impl CordisCore {
    pub fn new(store: CordisStore) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &CordisStore {
        &self.store
    }

    pub fn initialize(&self) -> CoreResult<()> {
        self.store.initialize()?;
        Ok(())
    }

    pub fn seed_strategy(&self, request: SeedStrategyRequest) -> CoreResult<StrategyState> {
        validate_non_empty(&request.strategy_id, "strategy_id")?;
        validate_non_empty(&request.domain, "domain")?;
        validate_non_empty(&request.project_id, "project_id")?;
        let key = strategy_key(&request.project_id, &request.domain, &request.strategy_id);
        let mut state = self
            .store
            .load_json::<StrategyState>(JsonTable::Strategy, &key)?
            .unwrap_or_else(|| {
                StrategyState::new(
                    request.project_id.clone(),
                    request.domain.clone(),
                    request.strategy_id.clone(),
                )
            });
        state.source = "seed".to_owned();
        state.promotion_status = StrategyPromotionStatus::Seed;
        state.prefer = dedupe(request.prefer);
        state.avoid = dedupe(request.avoid);
        state.seed = Some(StrategySeed {
            source_ref: request.source_ref,
            evidence_ids: dedupe(request.evidence_ids),
            applicability: request.applicability,
        });
        self.store.upsert_strategy(
            &key,
            &state.project_id,
            &state.domain,
            &state.strategy_id,
            &state,
        )?;
        self.store.audit("strategy_seeded", None, &state)?;
        Ok(state)
    }

    pub fn preflight(&self, mut request: PreflightRequest) -> CoreResult<CognitiveIr> {
        validate_preflight(&request)?;
        request.complexity = clamp(request.complexity);
        request.task.domain = request.task.domain.trim().to_lowercase();
        request.task.project_id = request.task.project_id.trim().to_owned();
        request.task.strategy_id = request.task.strategy_id.trim().to_owned();
        request.task.goal = request.task.goal.trim().to_owned();
        request.task.authorization.validate()?;

        let task_id = request.task.id.clone().unwrap_or_else(|| new_id("cordis"));
        request.task.id = Some(task_id.clone());
        let domain_key = domain_key(&request.task.project_id, &request.task.domain);
        let strategy_key = strategy_key(
            &request.task.project_id,
            &request.task.domain,
            &request.task.strategy_id,
        );
        let domain = self
            .store
            .load_json::<DomainState>(JsonTable::Domain, &domain_key)?
            .unwrap_or_else(|| {
                DomainState::new(request.task.project_id.clone(), request.task.domain.clone())
            });
        let strategy = self
            .store
            .load_json::<StrategyState>(JsonTable::Strategy, &strategy_key)?
            .unwrap_or_else(|| {
                StrategyState::new(
                    request.task.project_id.clone(),
                    request.task.domain.clone(),
                    request.task.strategy_id.clone(),
                )
            });

        let expected_success = expected_success(&domain, &strategy);
        let entropy = self.strategy_entropy(&request.task.project_id, &request.task.domain)?;
        let risk = risk_score(
            request.complexity,
            request.task.stakes,
            request.unknowns.len(),
            &domain,
        );
        let repeat_failed = strategy.failures > strategy.successes && strategy.failures > 0;
        let exploration_required = entropy.is_some_and(|value| value < 0.35) && strategy.uses >= 2;
        let authorization_required = request.task.authorization.status
            != AuthorizationStatus::Granted
            && (matches!(request.task.stakes, Stakes::High | Stakes::Critical) || risk >= 0.45);
        let advisor_required = request.task.stakes == Stakes::Critical
            || risk >= 0.62
            || domain.review_pressure >= 0.45
            || repeat_failed
            || authorization_required;

        let mut avoid = vec![
            "blind_retry".to_owned(),
            "claim_without_evidence".to_owned(),
        ];
        let mut prefer = vec![
            "read_before_write".to_owned(),
            "smallest_reversible_change".to_owned(),
            "prove_with_observable_evidence".to_owned(),
        ];
        prefer.extend(strategy.prefer.clone());
        avoid.extend(strategy.avoid.clone());
        if authorization_required {
            avoid.push("act_without_authorization".to_owned());
            prefer.push("request_authorization_before_act".to_owned());
        }
        if repeat_failed {
            avoid.push(format!(
                "repeat_failed_strategy:{}",
                request.task.strategy_id
            ));
            prefer.push("compare_alternative_strategy".to_owned());
        }
        if exploration_required {
            prefer.push("seek_strategy_diversity".to_owned());
        }

        let relevant_memory = self.relevant_episodes(&request.task, 3)?;
        let relevant_world_patterns = self.relevant_patterns(&request.task, 3)?;
        let prediction = Prediction {
            expected_success_probability: round3(expected_success),
            risk_score: round3(risk),
            strategy_entropy: entropy.map(round3),
            strategy_evidence: StrategyEvidence {
                uses: strategy.uses,
                successes: strategy.successes,
                failures: strategy.failures,
                partials: strategy.partials,
                calibration_error: round3(strategy.calibration_error),
            },
        };
        let ir = CognitiveIr {
            schema: CognitiveIr::new_schema(),
            task: request.task.clone(),
            state: CognitiveState {
                relevant_memory,
                relevant_world_patterns,
                capability_uncertainty: round3(domain.capability_uncertainty),
            },
            prediction: prediction.clone(),
            strategy: StrategyAdvice {
                id: request.task.strategy_id.clone(),
                status: if repeat_failed {
                    StrategyStatus::AvoidUntilRevalidated
                } else {
                    StrategyStatus::Available
                },
                source: strategy.source.clone(),
                promotion_status: strategy.promotion_status,
                prefer: dedupe(prefer),
                avoid: dedupe(avoid),
                exploration_required,
            },
            verification: VerificationContract {
                acceptance_evidence: request.acceptance_evidence.clone(),
                unknowns: request.unknowns.clone(),
            },
            escalation: Escalation {
                advisor_required,
                authorization_required,
                authorization: request.task.authorization.clone(),
            },
        };
        self.store.insert_task(
            &task_id,
            &request.task.project_id,
            &request.task.domain,
            &request.task.strategy_id,
            &request,
            &ir,
            &prediction,
        )?;
        self.store.audit("task_preflight", Some(&task_id), &ir)?;
        Ok(ir)
    }

    pub fn feedback(&self, request: FeedbackRequest) -> CoreResult<FeedbackResult> {
        validate_non_empty(&request.task_id, "task_id")?;
        if request.evidence.is_empty() {
            return Err(CoreError::Validation(
                "evidence must not be empty".to_owned(),
            ));
        }
        for evidence in &request.evidence {
            evidence.validate()?;
        }
        let stored = self
            .store
            .get_task(&request.task_id)?
            .ok_or_else(|| StoreError::NotFound(request.task_id.clone()))?;
        if stored.status != "open" {
            return Err(StoreError::AlreadyFinalized(request.task_id.clone()).into());
        }
        let preflight: PreflightRequest = serde_json::from_value(stored.payload.clone())?;
        let cognitive_ir: CognitiveIr = serde_json::from_value(stored.cognitive_ir.clone())?;
        let score = request
            .outcome_score
            .unwrap_or_else(|| request.outcome.score());
        validate_feedback(
            request.outcome,
            score,
            &request.evidence,
            &cognitive_ir.verification.acceptance_evidence,
        )?;
        let (attribution, attribution_source) =
            attribute(request.outcome, &request.evidence, request.attribution)?;

        let domain_key = domain_key(&stored.project_id, &stored.domain);
        let strategy_key = strategy_key(&stored.project_id, &stored.domain, &stored.strategy_id);
        let mut domain = self
            .store
            .load_json::<DomainState>(JsonTable::Domain, &domain_key)?
            .unwrap_or_else(|| DomainState::new(stored.project_id.clone(), stored.domain.clone()));
        let mut strategy = self
            .store
            .load_json::<StrategyState>(JsonTable::Strategy, &strategy_key)?
            .unwrap_or_else(|| {
                StrategyState::new(
                    stored.project_id.clone(),
                    stored.domain.clone(),
                    stored.strategy_id.clone(),
                )
            });
        let predicted = cognitive_ir.prediction.expected_success_probability;
        let difference = (predicted - score).abs();
        let rate = 0.20;
        domain.outcomes += 1;
        domain.calibration_error = ema(domain.calibration_error, difference, rate);
        strategy.uses += 1;
        match request.outcome {
            Outcome::Success => strategy.successes += 1,
            Outcome::Partial => strategy.partials += 1,
            Outcome::Failure => strategy.failures += 1,
        }
        strategy.calibration_error = ema(strategy.calibration_error, difference, rate);
        strategy.promotion_status = promotion_status(&strategy);
        let mut updates = vec![
            "calibration_error".to_owned(),
            "strategy_outcome".to_owned(),
        ];
        match attribution {
            Attribution::Strategy => {
                domain.review_pressure = ema(domain.review_pressure, difference, rate);
                updates.push("review_pressure".to_owned());
            }
            Attribution::Capability => {
                domain.capability_uncertainty =
                    ema(domain.capability_uncertainty, difference, rate);
                updates.push("capability_uncertainty".to_owned());
            }
            Attribution::World => {
                domain.world_uncertainty = ema(domain.world_uncertainty, difference, rate);
                updates.push("world_uncertainty".to_owned());
            }
            Attribution::Evidence | Attribution::Unknown => {}
        }

        let event = FeedbackEvent {
            id: new_id("fb"),
            at: now_rfc3339(),
            task_id: request.task_id.clone(),
            outcome: request.outcome,
            outcome_score: round3(score),
            expected_success_probability: round3(predicted),
            difference: round3(difference),
            attribution,
            attribution_source,
            evidence: request.evidence.clone(),
            lesson: request.lesson.clone(),
        };
        let episode = cordis_contracts::EpisodeRecord {
            id: new_id("ep"),
            at: event.at.clone(),
            goal: preflight.task.goal.clone(),
            domain: stored.domain.clone(),
            project_id: stored.project_id.clone(),
            strategy_id: stored.strategy_id.clone(),
            outcome: request.outcome,
            attribution,
            lesson: request.lesson.clone(),
            evidence_count: request.evidence.len(),
        };

        // The terminal task transition, feedback, calibrated states and episode are committed together.
        let domain_json = serde_json::to_string(&domain)?;
        let strategy_json = serde_json::to_string(&strategy)?;
        let event_json = serde_json::to_string(&event)?;
        let episode_json = serde_json::to_string(&episode)?;
        let now = now_rfc3339();
        self.store.with_transaction(|tx| {
            let changed = tx.execute(
                "UPDATE task_records SET status='finalized', finalized_at=?2 WHERE task_id=?1 AND status='open'",
                params![request.task_id, now],
            )?;
            if changed == 0 {
                return Err(StoreError::AlreadyFinalized(request.task_id.clone()));
            }
            tx.execute(
                "INSERT INTO feedback_events(id, task_id, payload_json, created_at) VALUES(?1, ?2, ?3, ?4)",
                params![event.id, request.task_id, event_json, now],
            )?;
            tx.execute(
                "INSERT INTO domain_states(state_key, payload_json, updated_at) VALUES(?1, ?2, ?3) ON CONFLICT(state_key) DO UPDATE SET payload_json=excluded.payload_json, updated_at=excluded.updated_at",
                params![domain_key, domain_json, now],
            )?;
            tx.execute(
                "INSERT INTO strategy_states(state_key, project_id, domain, strategy_id, payload_json, updated_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(state_key) DO UPDATE SET payload_json=excluded.payload_json, updated_at=excluded.updated_at",
                params![strategy_key, stored.project_id, stored.domain, stored.strategy_id, strategy_json, now],
            )?;
            tx.execute(
                "INSERT INTO episodes(id, project_id, domain, goal, payload_json, created_at) VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
                params![episode.id, stored.project_id, stored.domain, episode.goal, episode_json, now],
            )?;
            tx.execute(
                "DELETE FROM episodes WHERE id IN (SELECT id FROM episodes ORDER BY created_at DESC LIMIT -1 OFFSET ?1)",
                [MAX_EPISODES as i64],
            )?;
            Ok(())
        })?;

        if attribution == Attribution::World
            && let Some(lesson) = request.lesson.as_deref()
        {
            self.observe_world_pattern(
                &stored.project_id,
                &stored.domain,
                lesson,
                &request.evidence,
                &request.task_id,
            )?;
            updates.push("world_pattern_candidate".to_owned());
        }
        self.store
            .audit("task_feedback", Some(&request.task_id), &event)?;
        let entropy = self.strategy_entropy(&stored.project_id, &stored.domain)?;
        let next = BTreeMap::from([
            (
                "strategy_entropy".to_owned(),
                serde_json::to_value(entropy.map(round3))?,
            ),
            (
                "review_pressure".to_owned(),
                Value::from(round3(domain.review_pressure)),
            ),
            (
                "world_uncertainty".to_owned(),
                Value::from(round3(domain.world_uncertainty)),
            ),
            (
                "capability_uncertainty".to_owned(),
                Value::from(round3(domain.capability_uncertainty)),
            ),
        ]);
        Ok(FeedbackResult {
            schema: FeedbackResult::schema(),
            event,
            state_updates: updates,
            next_preflight_effect: next,
        })
    }

    pub fn status(&self) -> CoreResult<Value> {
        let counts = self.store.counts()?;
        let recent_failures = self.store.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT payload_json FROM feedback_events ORDER BY created_at DESC LIMIT 100",
            )?;
            let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
            let mut failures = Vec::new();
            for row in rows {
                let event: FeedbackEvent = serde_json::from_str(&row?)?;
                if event.outcome == Outcome::Failure {
                    failures.push(json!({
                        "id": event.id,
                        "task_id": event.task_id,
                        "at": event.at,
                        "attribution": event.attribution,
                        "difference": event.difference,
                        "lesson": event.lesson,
                    }));
                }
                if failures.len() == 10 {
                    break;
                }
            }
            Ok(failures)
        })?;
        Ok(json!({
            "schema": "cordis.state.v1",
            "updated_at": now_rfc3339(),
            "counts": counts,
            "recent_failures": recent_failures,
        }))
    }

    fn relevant_episodes(&self, task: &CoreTask, limit: usize) -> CoreResult<Vec<RelevantEpisode>> {
        let target = tokens(&task.goal);
        let mut ranked = Vec::new();
        for payload in self
            .store
            .episode_payloads(&task.project_id, &task.domain, 100)?
        {
            let episode: cordis_contracts::EpisodeRecord = serde_json::from_value(payload)?;
            let episode_tokens = tokens(&format!(
                "{} {}",
                episode.goal,
                episode.lesson.as_deref().unwrap_or_default()
            ));
            let overlap = overlap_score(&target, &episode_tokens);
            if overlap > 0.0 || episode.project_id == task.project_id {
                ranked.push((
                    overlap
                        + if episode.project_id == task.project_id {
                            0.05
                        } else {
                            0.0
                        },
                    RelevantEpisode {
                        id: episode.id,
                        outcome: episode.outcome,
                        attribution: episode.attribution,
                        lesson: episode.lesson,
                        evidence_count: episode.evidence_count,
                    },
                ));
            }
        }
        ranked.sort_by(|left, right| right.0.partial_cmp(&left.0).unwrap_or(Ordering::Equal));
        Ok(ranked
            .into_iter()
            .take(limit)
            .map(|(_, item)| item)
            .collect())
    }

    fn relevant_patterns(
        &self,
        task: &CoreTask,
        limit: usize,
    ) -> CoreResult<Vec<RelevantWorldPattern>> {
        let target = tokens(&task.goal);
        let mut ranked = Vec::new();
        for payload in self
            .store
            .world_pattern_payloads(&task.project_id, &task.domain)?
        {
            let pattern: cordis_contracts::WorldPatternRecord = serde_json::from_value(payload)?;
            if pattern.evidence_count < WORLD_PATTERN_MIN_SOURCES {
                continue;
            }
            let score = overlap_score(&target, &tokens(&pattern.statement));
            ranked.push((
                score + pattern.confidence * 0.1,
                RelevantWorldPattern {
                    id: pattern.id,
                    statement: pattern.statement,
                    confidence: pattern.confidence,
                    evidence_count: pattern.evidence_count,
                },
            ));
        }
        ranked.sort_by(|left, right| right.0.partial_cmp(&left.0).unwrap_or(Ordering::Equal));
        Ok(ranked
            .into_iter()
            .take(limit)
            .map(|(_, item)| item)
            .collect())
    }

    fn strategy_entropy(&self, project_id: &str, domain: &str) -> CoreResult<Option<f64>> {
        let states = self.store.strategy_payloads(project_id, domain)?;
        let mut counts = Vec::new();
        for payload in states {
            let state: StrategyState = serde_json::from_value(payload)?;
            if state.uses > 0 {
                counts.push(state.uses as f64);
            }
        }
        let total: f64 = counts.iter().sum();
        if total < 2.0 {
            return Ok(None);
        }
        if counts.len() == 1 {
            return Ok(Some(0.0));
        }
        let entropy = -counts
            .iter()
            .map(|count| {
                let probability = count / total;
                probability * probability.log2()
            })
            .sum::<f64>();
        Ok(Some(entropy / (counts.len() as f64).log2()))
    }

    fn observe_world_pattern(
        &self,
        project_id: &str,
        domain: &str,
        statement: &str,
        evidence: &[Evidence],
        task_id: &str,
    ) -> CoreResult<()> {
        let existing = self
            .store
            .find_world_pattern(project_id, domain, statement)?;
        let (id, mut sources) = existing
            .as_ref()
            .map(|(id, sources, _)| (id.clone(), sources.clone()))
            .unwrap_or_else(|| (new_id("wp"), Vec::new()));
        let mut source_set: BTreeSet<String> = sources.drain(..).collect();
        for item in evidence {
            source_set.insert(
                item.source_id
                    .clone()
                    .unwrap_or_else(|| format!("task:{task_id}")),
            );
        }
        let sources: Vec<_> = source_set.into_iter().collect();
        let record = cordis_contracts::WorldPatternRecord {
            id: id.clone(),
            project_id: project_id.to_owned(),
            domain: domain.to_owned(),
            statement: statement.trim().chars().take(500).collect(),
            evidence_count: sources.len(),
            confidence: round3(clamp(0.5 + sources.len() as f64 * 0.1)),
        };
        self.store
            .upsert_world_pattern(&id, project_id, domain, statement, &sources, &record)?;
        Ok(())
    }
}

fn validate_preflight(request: &PreflightRequest) -> CoreResult<()> {
    validate_non_empty(&request.task.goal, "task.goal")?;
    validate_non_empty(&request.task.domain, "task.domain")?;
    validate_non_empty(&request.task.project_id, "task.project_id")?;
    validate_non_empty(&request.task.strategy_id, "task.strategy_id")?;
    if request.complexity.is_nan() {
        return Err(CoreError::Validation(
            "complexity must be numeric".to_owned(),
        ));
    }
    let mut ids = HashSet::new();
    for criterion in &request.acceptance_evidence {
        validate_non_empty(&criterion.id, "acceptance_evidence.id")?;
        validate_non_empty(&criterion.description, "acceptance_evidence.description")?;
        if !ids.insert(criterion.id.as_str()) {
            return Err(CoreError::Validation(format!(
                "duplicate acceptance criterion: {}",
                criterion.id
            )));
        }
    }
    Ok(())
}

fn validate_feedback(
    outcome: Outcome,
    score: f64,
    evidence: &[Evidence],
    acceptance: &[AcceptanceCriterion],
) -> CoreResult<()> {
    if (score - outcome.score()).abs() > f64::EPSILON {
        return Err(CoreError::EvidenceConflict(
            "outcome_score must match the declared outcome".to_owned(),
        ));
    }
    let passed = evidence.iter().filter(|item| item.passed).count();
    let failed = evidence.len() - passed;
    if outcome == Outcome::Success && failed > 0 {
        return Err(CoreError::EvidenceConflict(
            "success cannot include failed evidence".to_owned(),
        ));
    }
    if outcome == Outcome::Failure && failed == 0 {
        return Err(CoreError::EvidenceConflict(
            "failure requires failed evidence".to_owned(),
        ));
    }
    let criteria: BTreeMap<_, _> = acceptance.iter().map(|item| (&item.id, item)).collect();
    for evidence in evidence {
        if let Some(acceptance_id) = &evidence.acceptance_id
            && !criteria.contains_key(acceptance_id)
        {
            return Err(CoreError::EvidenceConflict(format!(
                "unknown acceptance criterion: {acceptance_id}"
            )));
        }
    }
    let required: BTreeSet<_> = acceptance
        .iter()
        .filter(|item| item.required)
        .map(|item| item.id.as_str())
        .collect();
    let passed_ids: BTreeSet<_> = evidence
        .iter()
        .filter(|item| item.passed)
        .filter_map(|item| item.acceptance_id.as_deref())
        .collect();
    if outcome == Outcome::Success && !required.is_subset(&passed_ids) {
        return Err(CoreError::EvidenceConflict(
            "success must prove every required acceptance criterion".to_owned(),
        ));
    }
    if outcome == Outcome::Partial
        && !((passed > 0 && failed > 0) || !required.is_subset(&passed_ids))
    {
        return Err(CoreError::EvidenceConflict(
            "partial requires mixed evidence or incomplete acceptance".to_owned(),
        ));
    }
    Ok(())
}

fn attribute(
    outcome: Outcome,
    evidence: &[Evidence],
    hint: Option<Attribution>,
) -> CoreResult<(Attribution, String)> {
    let automatic = evidence_axis(outcome, evidence);
    if let Some(hint) = hint {
        if outcome == Outcome::Success && hint != Attribution::Unknown {
            return Err(CoreError::EvidenceConflict(
                "successful feedback cannot attribute a failure axis".to_owned(),
            ));
        }
        if automatic.0 != Attribution::Unknown && automatic.0 != hint {
            return Err(CoreError::EvidenceConflict(
                "attribution conflicts with evidence kind".to_owned(),
            ));
        }
        return Ok((hint, "workflow_hint".to_owned()));
    }
    Ok(automatic)
}

fn evidence_axis(outcome: Outcome, evidence: &[Evidence]) -> (Attribution, String) {
    if outcome == Outcome::Success {
        return (Attribution::Unknown, "no_failure_to_attribute".to_owned());
    }
    let kinds: BTreeSet<_> = evidence
        .iter()
        .map(|item| item.kind.to_lowercase())
        .collect();
    if contains_any(&kinds, &["network", "provider", "environment", "external"]) {
        return (Attribution::World, "evidence_kind".to_owned());
    }
    if contains_any(&kinds, &["model", "tool", "executor", "capability"]) {
        return (Attribution::Capability, "evidence_kind".to_owned());
    }
    if contains_any(&kinds, &["plan", "strategy", "approach"]) {
        return (Attribution::Strategy, "evidence_kind".to_owned());
    }
    if contains_any(&kinds, &["validation", "evidence", "verification"]) {
        return (Attribution::Evidence, "evidence_kind".to_owned());
    }
    (Attribution::Unknown, "insufficient_evidence".to_owned())
}

fn contains_any(values: &BTreeSet<String>, candidates: &[&str]) -> bool {
    candidates
        .iter()
        .any(|candidate| values.contains(*candidate))
}

fn expected_success(domain: &DomainState, strategy: &StrategyState) -> f64 {
    let base = (strategy.successes as f64 + 1.0) / (strategy.uses as f64 + 2.0);
    clamp(base - strategy.calibration_error * 0.35 - domain.calibration_error * 0.5)
}

fn risk_score(complexity: f64, stakes: Stakes, unknowns: usize, domain: &DomainState) -> f64 {
    let stake_weight = match stakes {
        Stakes::Low => 0.15,
        Stakes::Medium => 0.4,
        Stakes::High => 0.7,
        Stakes::Critical => 1.0,
    };
    clamp(
        0.35 * complexity
            + 0.25 * stake_weight
            + 0.20 * (unknowns as f64 / 6.0).min(1.0)
            + 0.08 * domain.calibration_error
            + 0.06 * domain.review_pressure
            + 0.03 * domain.world_uncertainty
            + 0.03 * domain.capability_uncertainty,
    )
}

fn promotion_status(strategy: &StrategyState) -> StrategyPromotionStatus {
    if strategy.failures >= 2 {
        StrategyPromotionStatus::Quarantined
    } else if strategy.source == "seed" {
        if strategy.uses >= 3 && strategy.successes >= 2 && strategy.failures == 0 {
            StrategyPromotionStatus::Active
        } else {
            StrategyPromotionStatus::Seed
        }
    } else {
        StrategyPromotionStatus::Active
    }
}

fn ema(before: f64, sample: f64, rate: f64) -> f64 {
    clamp(before * (1.0 - rate) + sample * rate)
}

fn clamp(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

fn round3(value: f64) -> f64 {
    (value * 1_000.0).round() / 1_000.0
}

fn domain_key(project_id: &str, domain: &str) -> String {
    format!("{project_id}:{domain}")
}

fn strategy_key(project_id: &str, domain: &str, strategy_id: &str) -> String {
    format!("{project_id}:{domain}:{strategy_id}")
}

fn validate_non_empty(value: &str, field: &str) -> CoreResult<()> {
    if value.trim().is_empty() {
        Err(CoreError::Validation(format!("{field} must be non-empty")))
    } else {
        Ok(())
    }
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn tokens(text: &str) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    let mut buffer = String::new();
    let flush = |buffer: &mut String, result: &mut BTreeSet<String>| {
        if buffer.chars().count() >= 2 {
            result.insert(buffer.to_lowercase());
        }
        buffer.clear();
    };
    let mut cjk_run = Vec::new();
    for character in text.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            if !cjk_run.is_empty() {
                add_cjk_tokens(&cjk_run, &mut result);
                cjk_run.clear();
            }
            buffer.push(character);
        } else if is_cjk(character) {
            flush(&mut buffer, &mut result);
            cjk_run.push(character);
        } else {
            flush(&mut buffer, &mut result);
            if !cjk_run.is_empty() {
                add_cjk_tokens(&cjk_run, &mut result);
                cjk_run.clear();
            }
        }
    }
    flush(&mut buffer, &mut result);
    if !cjk_run.is_empty() {
        add_cjk_tokens(&cjk_run, &mut result);
    }
    result
}

fn is_cjk(character: char) -> bool {
    matches!(character as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF)
}

fn add_cjk_tokens(run: &[char], result: &mut BTreeSet<String>) {
    if run.len() == 1 {
        result.insert(run.iter().collect());
        return;
    }
    for window in run.windows(2) {
        result.insert(window.iter().collect());
    }
    if run.len() <= 8 {
        result.insert(run.iter().collect());
    }
}

fn overlap_score(left: &BTreeSet<String>, right: &BTreeSet<String>) -> f64 {
    if left.is_empty() {
        return 0.0;
    }
    left.intersection(right).count() as f64 / left.len() as f64
}

#[cfg(test)]
mod tests {
    use super::*;
    use cordis_contracts::{AuthorizationEnvelope, EvidenceTrust, StrategyPromotionStatus};
    use tempfile::tempdir;

    fn runtime() -> CordisCore {
        let directory = tempdir().unwrap();
        let path = directory.keep().join("cordis.db");
        CordisCore::new(CordisStore::open(path).unwrap())
    }

    fn request(strategy: &str) -> PreflightRequest {
        PreflightRequest {
            task: CoreTask {
                id: None,
                goal: "Start an HTTP adapter in a clean environment".to_owned(),
                domain: "software".to_owned(),
                project_id: "cordis".to_owned(),
                strategy_id: strategy.to_owned(),
                stakes: Stakes::Medium,
                authorization: AuthorizationEnvelope::default(),
            },
            complexity: 0.5,
            unknowns: vec![],
            constraints: vec![],
            current_step: None,
            acceptance_evidence: vec![AcceptanceCriterion {
                id: "criterion-1".to_owned(),
                description: "clean-environment test passes".to_owned(),
                required: true,
            }],
        }
    }

    fn failed() -> Vec<Evidence> {
        vec![Evidence {
            id: None,
            kind: "strategy".to_owned(),
            summary: "clean import failed".to_owned(),
            passed: false,
            uri: None,
            acceptance_id: None,
            source_id: None,
            trust: EvidenceTrust::Observed,
        }]
    }

    fn passed() -> Vec<Evidence> {
        vec![Evidence {
            id: None,
            kind: "test".to_owned(),
            summary: "clean import passed".to_owned(),
            passed: true,
            uri: None,
            acceptance_id: Some("criterion-1".to_owned()),
            source_id: None,
            trust: EvidenceTrust::Observed,
        }]
    }

    #[test]
    fn failure_changes_next_preflight() {
        let core = runtime();
        let first = core.preflight(request("inspect")).unwrap();
        core.feedback(FeedbackRequest {
            task_id: first.task.id.clone().unwrap(),
            outcome: Outcome::Failure,
            attribution: Some(Attribution::Strategy),
            lesson: Some("Inspect runtime configuration first.".to_owned()),
            evidence: failed(),
            outcome_score: None,
        })
        .unwrap();
        let second = core.preflight(request("inspect")).unwrap();
        assert_eq!(
            second.strategy.status,
            StrategyStatus::AvoidUntilRevalidated
        );
        assert!(
            second
                .strategy
                .avoid
                .iter()
                .any(|item| item.contains("repeat_failed_strategy"))
        );
    }

    #[test]
    fn seed_requires_repeated_success() {
        let core = runtime();
        core.seed_strategy(SeedStrategyRequest {
            strategy_id: "verify_first".to_owned(),
            domain: "software".to_owned(),
            project_id: "cordis".to_owned(),
            prefer: vec![],
            avoid: vec![],
            source_ref: "manual".to_owned(),
            evidence_ids: vec!["seed".to_owned()],
            applicability: String::new(),
        })
        .unwrap();
        for index in 0..3 {
            let first = core.preflight(request("verify_first")).unwrap();
            core.feedback(FeedbackRequest {
                task_id: first.task.id.clone().unwrap(),
                outcome: Outcome::Success,
                attribution: None,
                lesson: Some(format!("success {index}")),
                evidence: passed(),
                outcome_score: None,
            })
            .unwrap();
        }
        let key = strategy_key("cordis", "software", "verify_first");
        let state = core
            .store
            .load_json::<StrategyState>(JsonTable::Strategy, &key)
            .unwrap()
            .unwrap();
        assert_eq!(state.promotion_status, StrategyPromotionStatus::Active);
    }

    #[test]
    fn success_without_acceptance_binding_is_rejected() {
        let core = runtime();
        let first = core.preflight(request("inspect")).unwrap();
        let mut evidence = passed();
        evidence[0].acceptance_id = None;
        assert!(
            core.feedback(FeedbackRequest {
                task_id: first.task.id.clone().unwrap(),
                outcome: Outcome::Success,
                attribution: None,
                lesson: None,
                evidence,
                outcome_score: None,
            })
            .is_err()
        );
    }
}
