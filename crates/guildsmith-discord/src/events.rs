//! `EventsPort` for the real Discord API, as JSON through [`crate::raw`] (with `ext::events` for
//! the bodies twilight 0.17.1 can't send).

use guildsmith_app::ports::{EventFields, EventInfo, EventsPort, PortResult, UserInfo};
use guildsmith_core::Id;
use serde_json::Value;
use twilight_http::request::Method;

use crate::ext::events::{RawEventUser, body, event};
use crate::port::TwilightDiscord;
use crate::raw;

impl EventsPort for TwilightDiscord {
    async fn events(&self, guild: Id) -> PortResult<Vec<EventInfo>> {
        let http = self.http().await?;
        let q = [("with_user_count".to_owned(), "true".to_owned())];
        let list: Vec<Value> =
            raw::json::<(), _>(http, Method::Get, &format!("guilds/{guild}/scheduled-events"), &q, None, None).await?;
        list.into_iter().map(event).collect()
    }

    async fn create_event(&self, guild: Id, f: &EventFields, reason: &str) -> PortResult<EventInfo> {
        let http = self.http().await?;
        let path = format!("guilds/{guild}/scheduled-events");
        let res = raw::send(http, Method::Post, &path, &[], Some(&body(f, true)), Some(reason)).await?;
        event(res.json()?)
    }

    async fn update_event(&self, guild: Id, id: Id, f: &EventFields, reason: &str) -> PortResult<EventInfo> {
        let http = self.http().await?;
        let path = format!("guilds/{guild}/scheduled-events/{id}");
        let res = raw::send(http, Method::Patch, &path, &[], Some(&body(f, false)), Some(reason)).await?;
        event(res.json()?)
    }

    async fn delete_event(&self, guild: Id, id: Id, reason: &str) -> PortResult<()> {
        let http = self.http().await?;
        raw::send(http, Method::Delete, &format!("guilds/{guild}/scheduled-events/{id}"), &[], None, Some(reason))
            .await?;
        Ok(())
    }

    async fn event_users(&self, guild: Id, id: Id, after: Option<Id>, limit: u8) -> PortResult<Vec<UserInfo>> {
        let http = self.http().await?;
        let mut q = vec![("limit".to_owned(), limit.to_string())];
        if let Some(a) = after {
            q.push(("after".to_owned(), a.to_string()));
        }
        let path = format!("guilds/{guild}/scheduled-events/{id}/users");
        let users: Vec<RawEventUser> = raw::json::<(), _>(http, Method::Get, &path, &q, None, None).await?;
        Ok(users.into_iter().map(|u| UserInfo::from(u.user)).collect())
    }
}
