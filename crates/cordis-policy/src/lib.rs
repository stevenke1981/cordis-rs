//! Central fail-closed policy engine. No other crate may invent execution permission.

use cordis_contracts::{
    ActionClass, AuthorizationEnvelope, AuthorizationStatus, ControlMode, NetworkProfile, Stakes,
    new_id, now_rfc3339,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum PolicyError {
    #[error("invalid policy context: {0}")]
    InvalidContext(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionProposal {
    #[serde(default)]
    pub action_id: Option<String>,
    pub action_class: ActionClass,
    pub action_name: String,
    pub description: String,
    pub purpose: String,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub target: Option<String>,
    #[serde(default)]
    pub network_access: bool,
    #[serde(default)]
    pub destructive: bool,
    #[serde(default)]
    pub approval_granted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PolicyContext {
    pub stakes: Stakes,
    pub risk_score: f64,
    pub control_mode: ControlMode,
    pub authorization: AuthorizationEnvelope,
    #[serde(default)]
    pub approval_required: bool,
    #[serde(default)]
    pub scope_in: Vec<String>,
    #[serde(default)]
    pub scope_out: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExecutionPermit {
    pub permit_id: String,
    pub issued_at: String,
    pub action_id: String,
    pub allowed: bool,
    pub authorization_satisfied: bool,
    pub approval_satisfied: bool,
    pub action_satisfied: bool,
    pub tool_satisfied: bool,
    pub target_satisfied: bool,
    pub network_satisfied: bool,
    pub scope_satisfied: bool,
    pub reasons: Vec<String>,
}

impl ExecutionPermit {
    pub fn denied(action_id: String, reasons: Vec<String>) -> Self {
        Self {
            permit_id: new_id("permit"),
            issued_at: now_rfc3339(),
            action_id,
            allowed: false,
            authorization_satisfied: false,
            approval_satisfied: false,
            action_satisfied: false,
            tool_satisfied: false,
            target_satisfied: false,
            network_satisfied: false,
            scope_satisfied: false,
            reasons,
        }
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct PolicyEngine;

impl PolicyEngine {
    pub fn evaluate(
        &self,
        context: &PolicyContext,
        action: &ActionProposal,
    ) -> Result<ExecutionPermit, PolicyError> {
        if !(0.0..=1.0).contains(&context.risk_score) || context.risk_score.is_nan() {
            return Err(PolicyError::InvalidContext(
                "risk_score must be between 0 and 1".to_owned(),
            ));
        }
        if action.action_name.trim().is_empty()
            || action.description.trim().is_empty()
            || action.purpose.trim().is_empty()
        {
            return Err(PolicyError::InvalidContext(
                "action name, description and purpose must be non-empty".to_owned(),
            ));
        }

        let action_id = action.action_id.clone().unwrap_or_else(|| new_id("action"));
        let mut reasons = BTreeSet::new();
        let authorization_required = requires_authorization(context, action);
        let authorization_satisfied =
            authorization_ok(context, authorization_required, &mut reasons);
        let approval_satisfied = approval_ok(context, action, &mut reasons);
        let action_satisfied = list_policy_ok(
            action.action_class.as_policy_name(),
            &action.action_name,
            &context.authorization.allowed_actions,
            &context.authorization.denied_actions,
            "action",
            &mut reasons,
        );
        let tool_satisfied = option_policy_ok(
            action.tool.as_deref(),
            &context.authorization.allowed_tools,
            &context.authorization.denied_tools,
            "tool",
            &mut reasons,
        );
        let target_satisfied = option_policy_ok(
            action.target.as_deref(),
            &context.authorization.allowed_targets,
            &context.authorization.denied_targets,
            "target",
            &mut reasons,
        );
        let network_satisfied = network_ok(context, action, &mut reasons);
        let scope_satisfied = scope_ok(context, action, &mut reasons);

        if context.control_mode == ControlMode::Takeover
            && action.action_class == ActionClass::Change
        {
            reasons.insert("takeover mode forbids change actions".to_owned());
        }

        let allowed = authorization_satisfied
            && approval_satisfied
            && action_satisfied
            && tool_satisfied
            && target_satisfied
            && network_satisfied
            && scope_satisfied
            && !(context.control_mode == ControlMode::Takeover
                && action.action_class == ActionClass::Change);

        Ok(ExecutionPermit {
            permit_id: new_id("permit"),
            issued_at: now_rfc3339(),
            action_id,
            allowed,
            authorization_satisfied,
            approval_satisfied,
            action_satisfied,
            tool_satisfied,
            target_satisfied,
            network_satisfied,
            scope_satisfied,
            reasons: reasons.into_iter().collect(),
        })
    }

    pub fn task_start_permit(&self, context: &PolicyContext) -> ExecutionPermit {
        // Task start is a local context-construction operation, not a tool or target action.
        // Preserve authorization status, expiry and risk gates while avoiding false denials
        // caused by allowed tool/target lists that apply only to later executable steps.
        let mut start_context = context.clone();
        start_context.authorization.allowed_actions.clear();
        start_context.authorization.denied_actions.clear();
        start_context.authorization.allowed_tools.clear();
        start_context.authorization.denied_tools.clear();
        start_context.authorization.allowed_targets.clear();
        start_context.authorization.denied_targets.clear();
        start_context.scope_in.clear();
        start_context.scope_out.clear();
        start_context.approval_required = false;

        let action = ActionProposal {
            action_id: Some(new_id("start")),
            action_class: ActionClass::Read,
            action_name: "task_start".to_owned(),
            description: "Start the declared CORDIS task".to_owned(),
            purpose: "Construct context without executing external changes".to_owned(),
            tool: None,
            target: None,
            network_access: false,
            destructive: false,
            approval_granted: false,
        };
        self.evaluate(&start_context, &action)
            .unwrap_or_else(|error| {
                ExecutionPermit::denied(
                    action.action_id.clone().unwrap_or_else(|| new_id("start")),
                    vec![error.to_string()],
                )
            })
    }
}

fn requires_authorization(context: &PolicyContext, action: &ActionProposal) -> bool {
    context.stakes == Stakes::High
        || context.stakes == Stakes::Critical
        || context.risk_score >= 0.45
        || action.action_class == ActionClass::Change
        || action.network_access
        || action.destructive
}

fn authorization_ok(
    context: &PolicyContext,
    required: bool,
    reasons: &mut BTreeSet<String>,
) -> bool {
    let auth = &context.authorization;
    if auth.status == AuthorizationStatus::Denied {
        reasons.insert("authorization was explicitly denied".to_owned());
        return false;
    }
    let now = now_rfc3339();
    if let Some(expiry) = &auth.expires_at
        && expiry.as_str() < now.as_str()
    {
        reasons.insert("authorization grant is expired".to_owned());
        return false;
    }
    if required && auth.status != AuthorizationStatus::Granted {
        reasons.insert("authorization is required but not granted".to_owned());
        return false;
    }
    true
}

fn approval_ok(
    context: &PolicyContext,
    action: &ActionProposal,
    reasons: &mut BTreeSet<String>,
) -> bool {
    let required = context.approval_required
        || action.destructive
        || context.stakes == Stakes::Critical
        || context.control_mode == ControlMode::Takeover;
    if required && !action.approval_granted {
        reasons.insert("explicit approval is required for this action".to_owned());
        return false;
    }
    true
}

fn list_policy_ok(
    class: &str,
    name: &str,
    allowed: &[String],
    denied: &[String],
    label: &str,
    reasons: &mut BTreeSet<String>,
) -> bool {
    let class = normalize(class);
    let name = normalize(name);
    let denied: BTreeSet<_> = denied.iter().map(|item| normalize(item)).collect();
    if denied.contains(&class) || denied.contains(&name) {
        reasons.insert(format!("{label} is explicitly denied"));
        return false;
    }
    if allowed.is_empty() {
        return true;
    }
    let allowed: BTreeSet<_> = allowed.iter().map(|item| normalize(item)).collect();
    if allowed.contains(&class) || allowed.contains(&name) {
        true
    } else {
        reasons.insert(format!("{label} is outside the allowed set"));
        false
    }
}

fn option_policy_ok(
    value: Option<&str>,
    allowed: &[String],
    denied: &[String],
    label: &str,
    reasons: &mut BTreeSet<String>,
) -> bool {
    let Some(value) = value else {
        if allowed.is_empty() {
            return true;
        }
        reasons.insert(format!(
            "{label} is required by the allowed set but was not declared"
        ));
        return false;
    };
    list_policy_ok(value, value, allowed, denied, label, reasons)
}

fn network_ok(
    context: &PolicyContext,
    action: &ActionProposal,
    reasons: &mut BTreeSet<String>,
) -> bool {
    if !action.network_access {
        return true;
    }
    match context.authorization.network_profile {
        NetworkProfile::Offline => {
            reasons.insert("network profile is offline".to_owned());
            false
        }
        NetworkProfile::ReadOnly => {
            if action.action_class == ActionClass::Change {
                reasons.insert("read-only network profile forbids remote changes".to_owned());
                false
            } else {
                true
            }
        }
        NetworkProfile::AuthorizedTargetsOnly => {
            if action.target.is_none() {
                reasons.insert(
                    "authorized-target network access requires an explicit target".to_owned(),
                );
                false
            } else {
                true
            }
        }
        NetworkProfile::Unrestricted => true,
    }
}

fn scope_ok(
    context: &PolicyContext,
    action: &ActionProposal,
    reasons: &mut BTreeSet<String>,
) -> bool {
    let Some(target) = action.target.as_deref() else {
        return true;
    };
    let normalized_target = normalize(target);
    if context
        .scope_out
        .iter()
        .map(|item| normalize(item))
        .any(|item| target_matches(&normalized_target, &item))
    {
        reasons.insert("target overlaps task scope.out".to_owned());
        return false;
    }
    if context.scope_in.is_empty() {
        return true;
    }
    if context
        .scope_in
        .iter()
        .map(|item| normalize(item))
        .any(|item| target_matches(&normalized_target, &item))
    {
        true
    } else {
        reasons.insert("target is outside task scope.in".to_owned());
        false
    }
}

fn target_matches(target: &str, policy: &str) -> bool {
    target == policy
        || target.starts_with(&format!("{policy}/"))
        || target.starts_with(&format!("{policy}\\"))
}

fn normalize(value: &str) -> String {
    value.trim().to_lowercase().replace(' ', "_")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context() -> PolicyContext {
        PolicyContext {
            stakes: Stakes::High,
            risk_score: 0.6,
            control_mode: ControlMode::HighIntervention,
            authorization: AuthorizationEnvelope::default(),
            approval_required: false,
            scope_in: vec!["src".to_owned()],
            scope_out: vec!["production".to_owned()],
        }
    }

    fn action() -> ActionProposal {
        ActionProposal {
            action_id: Some("a1".to_owned()),
            action_class: ActionClass::Change,
            action_name: "change".to_owned(),
            description: "Update source".to_owned(),
            purpose: "Fix a bug".to_owned(),
            tool: Some("editor".to_owned()),
            target: Some("src/lib.rs".to_owned()),
            network_access: false,
            destructive: false,
            approval_granted: false,
        }
    }

    #[test]
    fn pending_high_risk_task_is_fail_closed() {
        let permit = PolicyEngine.evaluate(&context(), &action()).unwrap();
        assert!(!permit.allowed);
        assert!(!permit.authorization_satisfied);
    }

    #[test]
    fn granted_authorization_allows_bounded_action() {
        let mut context = context();
        context.authorization.status = AuthorizationStatus::Granted;
        context.authorization.basis = "owner approval".to_owned();
        context.authorization.allowed_actions = vec!["change".to_owned()];
        context.authorization.allowed_tools = vec!["editor".to_owned()];
        context.authorization.allowed_targets = vec!["src/lib.rs".to_owned()];
        let permit = PolicyEngine.evaluate(&context, &action()).unwrap();
        assert!(permit.allowed, "{:?}", permit.reasons);
    }

    #[test]
    fn denied_tool_wins_over_allowed_action() {
        let mut context = context();
        context.authorization.status = AuthorizationStatus::Granted;
        context.authorization.basis = "owner approval".to_owned();
        context.authorization.denied_tools = vec!["editor".to_owned()];
        let permit = PolicyEngine.evaluate(&context, &action()).unwrap();
        assert!(!permit.allowed);
        assert!(!permit.tool_satisfied);
    }
}
