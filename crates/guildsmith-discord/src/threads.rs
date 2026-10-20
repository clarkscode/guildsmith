//! `ThreadsPort` for the real Discord API, as JSON through [`crate::raw`].

use guildsmith_app::ports::{NewThread, PortError, PortResult, ThreadEdit, ThreadInfo, ThreadsPort};
use guildsmith_core::Id;
use serde::Deserialize;
use serde_json::{Map, Value, json};
use twilight_http::request::Method;

use crate::messages::{message_json, send_message};
use crate::port::TwilightDiscord;
use crate::raw;

/// A thread's `PINNED` flag.
const PINNED: u64 = 1 << 1;

#[derive(Debug, Deserialize)]
struct RawMetadata {
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    locked: bool,
    #[serde(default)]
    auto_archive_duration: u32,
}

#[derive(Debug, Deserialize)]
struct RawThread {
    id: Id,
    #[serde(rename = "type")]
    kind: u8,
    #[serde(default)]
    parent_id: Option<Id>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    flags: u64,
    #[serde(default)]
    owner_id: Option<Id>,
    #[serde(default)]
    message_count: u32,
    #[serde(default)]
    member_count: u32,
    #[serde(default)]
    applied_tags: Vec<Id>,
    #[serde(default)]
    thread_metadata: Option<RawMetadata>,
}

impl RawThread {
    fn into_info(self) -> PortResult<ThreadInfo> {
        let kind = match self.kind {
            10 => "announcement",
            11 => "public",
            12 => "private",
            other => {
                return Err(PortError::NotFound(format!("channel {} is not a thread (type {other})", self.id)));
            }
        };
        let meta =
            self.thread_metadata.unwrap_or(RawMetadata { archived: false, locked: false, auto_archive_duration: 4320 });
        Ok(ThreadInfo {
            id: self.id,
            parent_id: self.parent_id.unwrap_or(Id(0)),
            name: self.name.unwrap_or_default(),
            kind: kind.to_owned(),
            archived: meta.archived,
            locked: meta.locked,
            auto_archive: meta.auto_archive_duration,
            tags: self.applied_tags,
            pinned: self.flags & PINNED != 0,
            owner_id: self.owner_id,
            message_count: self.message_count,
            member_count: self.member_count,
        })
    }
}

#[derive(Debug, Deserialize)]
struct RawThreadList {
    threads: Vec<RawThread>,
    #[serde(default)]
    has_more: bool,
}

#[derive(Debug, Deserialize)]
struct RawThreadMember {
    #[serde(default)]
    user_id: Option<Id>,
}

#[derive(Debug, Deserialize)]
struct RawFlags {
    #[serde(default)]
    flags: u64,
    #[serde(rename = "type")]
    kind: u8,
}

fn threads(list: Vec<RawThread>) -> Vec<ThreadInfo> {
    list.into_iter().filter_map(|t| t.into_info().ok()).collect()
}

impl ThreadsPort for TwilightDiscord {
    async fn thread(&self, thread: Id) -> PortResult<ThreadInfo> {
        let http = self.http().await?;
        let t: RawThread =
            raw::json::<(), _>(http, Method::Get, &format!("channels/{thread}"), &[], None, None).await?;
        t.into_info()
    }

    async fn channel_flags(&self, channel: Id) -> PortResult<u64> {
        let http = self.http().await?;
        let c: RawFlags =
            raw::json::<(), _>(http, Method::Get, &format!("channels/{channel}"), &[], None, None).await?;
        Ok(c.flags)
    }

    async fn create_thread(&self, channel: Id, t: &NewThread, reason: &str) -> PortResult<ThreadInfo> {
        let http = self.http().await?;
        let mut body = Map::new();
        body.insert("name".to_owned(), json!(t.name));
        if let Some(a) = t.auto_archive {
            body.insert("auto_archive_duration".to_owned(), json!(a));
        }
        let res = if let Some(post) = &t.post {
            // A forum or media post: its first message goes with it.
            if !t.tags.is_empty() {
                body.insert("applied_tags".to_owned(), json!(t.tags));
            }
            body.insert("message".to_owned(), message_json(post));
            let files: Vec<(String, Vec<u8>)> = post.files.iter().map(|f| (f.name.clone(), f.bytes.clone())).collect();
            send_message(http, &format!("channels/{channel}/threads"), &Value::Object(body), &files, Some(reason))
                .await?
        } else if let Some(m) = t.from_message {
            let path = format!("channels/{channel}/messages/{m}/threads");
            raw::send(http, Method::Post, &path, &[], Some(&Value::Object(body)), Some(reason)).await?
        } else {
            // Threads in announcement channels are announcement threads (type 10).
            let parent: RawFlags =
                raw::json::<(), _>(http, Method::Get, &format!("channels/{channel}"), &[], None, None).await?;
            let kind = if t.private {
                12
            } else if parent.kind == 5 {
                10
            } else {
                11
            };
            body.insert("type".to_owned(), json!(kind));
            raw::send(
                http,
                Method::Post,
                &format!("channels/{channel}/threads"),
                &[],
                Some(&Value::Object(body)),
                Some(reason),
            )
            .await?
        };
        raw::parse::<RawThread>(&res, "create thread")?.into_info()
    }

    async fn update_thread(&self, thread: Id, e: &ThreadEdit, reason: &str) -> PortResult<ThreadInfo> {
        let http = self.http().await?;
        let mut body = Map::new();
        if let Some(v) = &e.name {
            body.insert("name".to_owned(), json!(v));
        }
        if let Some(v) = e.archived {
            body.insert("archived".to_owned(), json!(v));
        }
        if let Some(v) = e.locked {
            body.insert("locked".to_owned(), json!(v));
        }
        if let Some(v) = e.auto_archive {
            body.insert("auto_archive_duration".to_owned(), json!(v));
        }
        if let Some(v) = &e.tags {
            body.insert("applied_tags".to_owned(), json!(v));
        }
        if let Some(v) = e.pinned {
            body.insert("flags".to_owned(), json!(if v { PINNED } else { 0 }));
        }
        if let Some(v) = e.slowmode {
            body.insert("rate_limit_per_user".to_owned(), json!(v));
        }
        let res = raw::send(
            http,
            Method::Patch,
            &format!("channels/{thread}"),
            &[],
            Some(&Value::Object(body)),
            Some(reason),
        )
        .await?;
        raw::parse::<RawThread>(&res, "update thread")?.into_info()
    }

    async fn active_threads(&self, guild: Id) -> PortResult<Vec<ThreadInfo>> {
        let http = self.http().await?;
        let list: RawThreadList =
            raw::json::<(), _>(http, Method::Get, &format!("guilds/{guild}/threads/active"), &[], None, None).await?;
        Ok(threads(list.threads))
    }

    async fn archived_threads(
        &self,
        channel: Id,
        private: bool,
        before: Option<&str>,
        limit: u8,
    ) -> PortResult<(Vec<ThreadInfo>, bool)> {
        let http = self.http().await?;
        let which = if private { "private" } else { "public" };
        let mut q = vec![("limit".to_owned(), limit.to_string())];
        if let Some(b) = before {
            q.push(("before".to_owned(), b.to_owned()));
        }
        let path = format!("channels/{channel}/threads/archived/{which}");
        let list: RawThreadList = raw::json::<(), _>(http, Method::Get, &path, &q, None, None).await?;
        Ok((threads(list.threads), list.has_more))
    }

    async fn thread_members(&self, thread: Id) -> PortResult<Vec<Id>> {
        let http = self.http().await?;
        let list: Vec<RawThreadMember> =
            raw::json::<(), _>(http, Method::Get, &format!("channels/{thread}/thread-members"), &[], None, None)
                .await?;
        Ok(list.into_iter().filter_map(|m| m.user_id).collect())
    }

    async fn add_thread_member(&self, thread: Id, user: Id) -> PortResult<()> {
        let http = self.http().await?;
        raw::send(http, Method::Put, &format!("channels/{thread}/thread-members/{user}"), &[], None, None).await?;
        Ok(())
    }

    async fn remove_thread_member(&self, thread: Id, user: Id) -> PortResult<()> {
        let http = self.http().await?;
        raw::send(http, Method::Delete, &format!("channels/{thread}/thread-members/{user}"), &[], None, None).await?;
        Ok(())
    }
}
