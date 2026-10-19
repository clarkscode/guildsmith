//! The application context both interfaces share: the Discord port and the policy.

use crate::policy::Policy;
use crate::ports::DiscordPort;

/// What every service needs. Built once per process by the binary.
pub struct App<P> {
    pub port: P,
    pub policy: Policy,
}

impl<P: DiscordPort> App<P> {
    pub fn new(port: P, policy: Policy) -> Self {
        App { port, policy }
    }
}
