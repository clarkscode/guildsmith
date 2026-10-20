//! guildsmith-core: the pure domain model, spec format, diff and planner. No I/O.

pub mod asset;
pub mod diff;
pub mod explain;
pub mod export;
pub mod hash;
pub mod id;
pub mod live;
pub mod matching;
pub mod permissions;
pub mod planner;
pub mod raw;
pub mod snapshot;
pub mod spec;
pub mod suggest;
pub mod template;
pub mod time;
pub mod validate;

pub use diff::{diff, diff_with};
pub use id::Id;
pub use matching::PlanError;
pub use permissions::{Permission, PermissionSet};
pub use planner::Plan;
