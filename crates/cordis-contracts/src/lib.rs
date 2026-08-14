//! Versioned, provider-neutral contracts shared by every CORDIS Rust crate.

mod boundary;
mod cognition;
mod error;
mod ids;
mod memory;
mod task;
mod workflow;

pub use boundary::*;
pub use cognition::*;
pub use error::*;
pub use ids::*;
pub use memory::*;
pub use task::*;
pub use workflow::*;

pub const TASK_CONTRACT_SCHEMA: &str = "cordis.task.v1";
pub const AUTHORIZATION_SCHEMA: &str = "cordis.authorization.v1";
pub const DIFFICULTY_PROFILE_SCHEMA: &str = "cordis.difficulty.v1";
pub const PLAN_SCHEMA: &str = "cordis.plan.v1";
pub const STEP_RESULT_SCHEMA: &str = "cordis.step-result.v1";
pub const COGNITIVE_IR_SCHEMA: &str = "cordis.cognitive.v1";
pub const FEEDBACK_RESULT_SCHEMA: &str = "cordis.feedback-result.v1";
pub const BOUNDARY_REVIEW_SCHEMA: &str = "cordis.boundary-review.v1";
pub const GOAL_MODE_SCHEMA: &str = "cordis.goal-mode.v1";
pub const PLAN_MODE_SCHEMA: &str = "cordis.plan-mode.v1";
pub const MEMORY_SCHEMA: &str = "cordis.memory.v1";
pub const RUNTIME_SCHEMA: &str = "cordis.runtime.v1";
pub const WORKFLOW_RUNTIME_SCHEMA: &str = "cordis.workflow-runtime.v1";
pub const CAPABILITY_INDEX_SCHEMA: &str = "cordis.capability-index.v1";
