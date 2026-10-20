//! `PollsPort` for the real Discord API: voters per answer, and ending a poll early.

use guildsmith_app::ports::{MessageInfo, PollsPort, PortResult, UserInfo};
use guildsmith_core::Id;
use serde::Deserialize;
use twilight_http::request::Method;

use crate::messages::{RawMessage, RawUser};
use crate::port::TwilightDiscord;
use crate::raw;

#[derive(Debug, Deserialize)]
struct RawVoters {
    users: Vec<RawUser>,
}

impl PollsPort for TwilightDiscord {
    async fn answer_voters(
        &self,
        channel: Id,
        message: Id,
        answer: u32,
        after: Option<Id>,
        limit: u8,
    ) -> PortResult<Vec<UserInfo>> {
        let http = self.http().await?;
        let mut q = vec![("limit".to_owned(), limit.to_string())];
        if let Some(a) = after {
            q.push(("after".to_owned(), a.to_string()));
        }
        let path = format!("channels/{channel}/polls/{message}/answers/{answer}");
        let v: RawVoters = raw::json::<(), _>(http, Method::Get, &path, &q, None, None).await?;
        Ok(v.users.into_iter().map(UserInfo::from).collect())
    }

    async fn end_poll(&self, channel: Id, message: Id) -> PortResult<MessageInfo> {
        let http = self.http().await?;
        let path = format!("channels/{channel}/polls/{message}/expire");
        let res = raw::send(http, Method::Post, &path, &[], None, None).await?;
        Ok(raw::parse::<RawMessage>(&res, "end poll")?.into())
    }
}
