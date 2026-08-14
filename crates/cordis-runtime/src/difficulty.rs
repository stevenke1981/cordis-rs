use crate::{RuntimeError, RuntimeResult};
use cordis_contracts::{
    ControlMode, DIFFICULTY_PROFILE_SCHEMA, DifficultyComponent, DifficultyComponents,
    DifficultyInputs, DifficultyProfile, Stakes, TaskContract,
};

const WEIGHTS: [f64; 6] = [0.20, 0.15, 0.25, 0.20, 0.10, 0.10];

pub fn assess_difficulty(
    task: &TaskContract,
    inputs: &DifficultyInputs,
) -> RuntimeResult<DifficultyProfile> {
    task.validate()?;
    let complexity = score(inputs.complexity, "complexity")?;
    let irreversibility = score(inputs.irreversibility, "irreversibility")?;
    let novelty = score(inputs.novelty, "novelty")?;
    let material_unknowns = task.unknowns.iter().filter(|item| item.material).count();
    let uncertainty = (0.1 + 0.25 * material_unknowns as f64).min(1.0);
    let missing_fact = usize::from(task.known_facts.is_empty());
    let blocking_gaps = task.completeness.blocking_gaps.len();
    let evidence_deficit = (0.1 + 0.35 * missing_fact as f64 + 0.2 * blocking_gaps as f64).min(1.0);
    let impact = match task.stakes {
        Stakes::Low => 0.2,
        Stakes::Medium => 0.45,
        Stakes::High => 0.75,
        Stakes::Critical => 1.0,
    };
    let components = DifficultyComponents {
        complexity: component(complexity, "host-declared complexity"),
        uncertainty: component(
            uncertainty,
            &format!("{material_unknowns} material unknown(s) in the task contract"),
        ),
        impact: component(impact, &format!("stakes={:?}", task.stakes).to_lowercase()),
        irreversibility: component(
            irreversibility,
            "host-declared irreversible action exposure",
        ),
        novelty: component(
            novelty,
            inputs
                .novelty_reason
                .as_deref()
                .unwrap_or("host-declared novelty"),
        ),
        evidence_deficit: component(
            evidence_deficit,
            &format!("{missing_fact} missing-fact signal(s), {blocking_gaps} blocking gap(s)"),
        ),
    };
    let scores = [
        complexity,
        uncertainty,
        impact,
        irreversibility,
        novelty,
        evidence_deficit,
    ];
    let aggregate = scores
        .iter()
        .zip(WEIGHTS)
        .map(|(score, weight)| score * weight)
        .sum::<f64>();
    let (control_mode, override_reason, policy_reason) = if impact >= 0.9 || irreversibility >= 0.9
    {
        (
            ControlMode::Takeover,
            Some("critical impact or irreversibility overrides aggregate difficulty".to_owned()),
            "critical impact or irreversibility overrides aggregate difficulty".to_owned(),
        )
    } else if impact >= 0.7 || irreversibility >= 0.7 || evidence_deficit >= 0.75 {
        (
            ControlMode::HighIntervention,
            None,
            "high impact, irreversibility, or evidence deficit requires stronger gates".to_owned(),
        )
    } else if aggregate >= 0.35 || complexity >= 0.6 || material_unknowns > 0 {
        (
            ControlMode::Advisory,
            None,
            "task benefits from explicit planning or cognitive context".to_owned(),
        )
    } else {
        (
            ControlMode::Fast,
            None,
            "low aggregate difficulty with no high-risk override".to_owned(),
        )
    };
    let profile = DifficultyProfile {
        schema: DIFFICULTY_PROFILE_SCHEMA.to_owned(),
        components,
        control_mode,
        override_reason,
        aggregate_score: Some(round3(aggregate)),
        policy_reasons: vec![policy_reason],
    };
    profile.validate()?;
    Ok(profile)
}

fn component(score: f64, reason: &str) -> DifficultyComponent {
    DifficultyComponent {
        score: round3(score),
        reasons: vec![reason.to_owned()],
    }
}

fn score(value: f64, field: &str) -> RuntimeResult<f64> {
    if value.is_nan() || !(0.0..=1.0).contains(&value) {
        Err(RuntimeError::Validation(format!(
            "{field} must be between 0 and 1"
        )))
    } else {
        Ok(value)
    }
}

fn round3(value: f64) -> f64 {
    (value * 1_000.0).round() / 1_000.0
}
