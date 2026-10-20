//! `PollsPort`: poll voters and ending polls (US2). Polls are sent as messages
//! (`OutgoingMessage::poll`) and read back as `MessageInfo::poll`.

use std::future::Future;

use guildsmith_core::Id;

use super::PortResult;
use super::messages::{MessageInfo, UserInfo};

pub trait PollsPort: Send + Sync {
    /// Users who voted for one answer.
    fn answer_voters(
        &self,
        channel: Id,
        message: Id,
        answer: u32,
        after: Option<Id>,
        limit: u8,
    ) -> impl Future<Output = PortResult<Vec<UserInfo>>> + Send;
    /// Ends the bot's own poll now.
    fn end_poll(&self, channel: Id, message: Id) -> impl Future<Output = PortResult<MessageInfo>> + Send;
}
