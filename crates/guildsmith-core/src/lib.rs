//! guildsmith-core: the pure domain model, spec format, diff and planner. No I/O.

pub mod diff;
pub mod explain;
pub mod export;
pub mod hash;
pub mod id;
pub mod matching;
pub mod permissions;
pub mod planner;
pub mod snapshot;
pub mod spec;
pub mod suggest;
pub mod validate;

pub use diff::diff;
pub use id::Id;
pub use matching::PlanError;
pub use permissions::{Permission, PermissionSet};
pub use planner::Plan;
