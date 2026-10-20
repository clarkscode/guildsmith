//! `AutomodPort` for the real Discord API: rules as JSON through [`crate::raw`], with
//! `ext::automod` for the bodies.

use guildsmith_app::ports::{AutomodPort, PortResult};
use guildsmith_core::Id;
use guildsmith_core::spec::automod::AutomodRule;
use twilight_http::request::Method;

use crate::ext::automod::{RawRule, body};
use crate::port::TwilightDiscord;
use crate::raw;

impl AutomodPort for TwilightDiscord {
    async fn automod_rules(&self, guild: Id) -> PortResult<Vec<AutomodRule<Id>>> {
        let http = self.http().await?;
        let path = format!("guilds/{guild}/auto-moderation/rules");
        let rules: Vec<RawRule> = raw::json::<(), _>(http, Method::Get, &path, &[], None, None).await?;
        // Rules of kinds guildsmith doesn't know are left out rather than failing the list.
        Ok(rules.into_iter().filter_map(|r| AutomodRule::try_from(r).ok()).collect())
    }

    async fn create_automod(&self, guild: Id, rule: &AutomodRule<Id>, reason: &str) -> PortResult<AutomodRule<Id>> {
        let http = self.http().await?;
        let path = format!("guilds/{guild}/auto-moderation/rules");
        let res = raw::send(http, Method::Post, &path, &[], Some(&body(rule, true)), Some(reason)).await?;
        raw::parse::<RawRule>(&res, "create automod rule")?.try_into()
    }

    async fn update_automod(
        &self,
        guild: Id,
        id: Id,
        rule: &AutomodRule<Id>,
        reason: &str,
    ) -> PortResult<AutomodRule<Id>> {
        let http = self.http().await?;
        let path = format!("guilds/{guild}/auto-moderation/rules/{id}");
        let res = raw::send(http, Method::Patch, &path, &[], Some(&body(rule, false)), Some(reason)).await?;
        raw::parse::<RawRule>(&res, "update automod rule")?.try_into()
    }

    async fn delete_automod(&self, guild: Id, id: Id, reason: &str) -> PortResult<()> {
        let http = self.http().await?;
        raw::send(http, Method::Delete, &format!("guilds/{guild}/auto-moderation/rules/{id}"), &[], None, Some(reason))
            .await?;
        Ok(())
    }
}
