//! guildsmith-core: the pure domain model, spec format, diff and planner. No I/O.

pub mod explain;
pub mod export;
pub mod id;
pub mod permissions;
pub mod snapshot;
pub mod spec;
pub mod suggest;
pub mod validate;

pub use id::Id;
pub use permissions::{Permission, PermissionSet};
