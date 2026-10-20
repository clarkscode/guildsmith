//! Guild message search (`GET guilds/{g}/messages/search`), missing in twilight 0.17.1
//! (research E3/E11). A `202` with code `110000` means the server isn't indexed yet;
//! `messages` is an array of arrays (each hit with its context).

use guildsmith_app::ports::{MessageInfo, PortResult, SearchOutcome, SearchQuery};
use guildsmith_core::Id;
use serde::Deserialize;
use twilight_http::Client;
use twilight_http::request::Method;

use crate::messages::RawMessage;
use crate::raw;

#[derive(Debug, Deserialize)]
struct Found {
    #[serde(default)]
    total_results: u64,
    #[serde(default)]
    messages: Vec<Vec<RawMessage>>,
}

#[derive(Debug, Deserialize)]
struct NotIndexed {
    code: u64,
    #[serde(default)]
    retry_after: Option<f64>,
}

/// The query string for a search.
pub(crate) fn query(q: &SearchQuery) -> Vec<(String, String)> {
    let mut out = vec![("limit".to_owned(), q.limit.min(25).to_string())];
    if q.offset > 0 {
        out.push(("offset".to_owned(), q.offset.min(9975).to_string()));
    }
    if let Some(c) = &q.content {
        out.push(("content".to_owned(), c.clone()));
    }
    out.extend(q.authors.iter().map(|a| ("author_id".to_owned(), a.to_string())));
    out.extend(q.channels.iter().map(|c| ("channel_id".to_owned(), c.to_string())));
    out.extend(q.has.iter().map(|h| ("has".to_owned(), h.clone())));
    if let Some(m) = q.min_id {
        out.push(("min_id".to_owned(), m.to_string()));
    }
    if let Some(m) = q.max_id {
        out.push(("max_id".to_owned(), m.to_string()));
    }
    out
}

pub(crate) async fn search(http: &Client, guild: Id, q: &SearchQuery) -> PortResult<SearchOutcome> {
    let res = raw::send(http, Method::Get, &format!("guilds/{guild}/messages/search"), &query(q), None, None).await?;
    if res.status == 202
        && let Ok(n) = raw::parse::<NotIndexed>(&res, "message search")
        && n.code == 110_000
    {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a few seconds
        let retry_after = n.retry_after.map_or(2, |s| s.ceil().max(1.0) as u64);
        return Ok(SearchOutcome::NotIndexed { retry_after });
    }
    let found: Found = raw::parse(&res, "message search")?;
    // Each hit comes first in its group, the rest is context.
    let messages: Vec<MessageInfo> =
        found.messages.into_iter().filter_map(|group| group.into_iter().next()).map(MessageInfo::from).collect();
    Ok(SearchOutcome::Found { total: found.total_results, messages })
}
