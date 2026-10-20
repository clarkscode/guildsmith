//! T104: the raw path classifier against every row of research E2.

use guildsmith_core::Id;
use guildsmith_core::raw::{RawMethod, RawScope, body_problem, classify, looks_like_bot_token, normalize, redact};
use serde_json::json;

use RawMethod::{Delete, Get, Patch, Post, Put};

fn refused(path: &str, m: RawMethod) -> String {
    match classify(path, m) {
        RawScope::Refused(why) => why,
        other => panic!("{m} {path} should be refused, got {other:?}"),
    }
}

#[test]
fn guild_channel_webhook_invite_and_self_paths() {
    assert_eq!(classify("guilds/1000/audit-logs", Get), RawScope::Guild(Id(1000)));
    assert_eq!(classify("guilds/1000", Patch), RawScope::Guild(Id(1000)));
    assert_eq!(classify("channels/101/messages", Post), RawScope::Channel(Id(101)));
    assert_eq!(classify("stage-instances/105", Delete), RawScope::Channel(Id(105)));
    assert_eq!(classify("webhooks/700", Get), RawScope::Webhook(Id(700)));
    assert_eq!(classify("invites/abc123", Get), RawScope::Invite("abc123".to_owned()));
    assert_eq!(classify("invites/abc123/target-users", Put), RawScope::Invite("abc123".to_owned()));
    assert_eq!(classify("users/@me", Get), RawScope::SelfUser);
    assert_eq!(classify("users/@me/guilds", Get), RawScope::SelfUser);
}

#[test]
fn leaving_a_server_is_a_destructive_guild_write() {
    assert_eq!(classify("users/@me/guilds/1000", Delete), RawScope::Guild(Id(1000)));
    assert!(RawMethod::Delete.is_write());
}

#[test]
fn refused_routes() {
    assert!(refused("webhooks/700/abcdef-token", Post).contains("token"));
    assert!(refused("webhooks/700/abcdef-token/messages/1", Patch).contains("token"));
    assert!(refused("applications/123/commands", Put).contains("commands"));
    assert!(refused("applications/123/guilds/1000/commands", Post).contains("commands"));
    assert!(refused("oauth2/applications/@me", Get).contains("OAuth2"));
    assert!(refused("gateway/bot", Get).contains("gateway"));
    assert!(refused("guilds", Post).contains("can't create servers"));
    assert!(refused("users/@me", Patch).contains("account"));
    assert!(refused("voice/regions", Get).contains("can't tie"));
    assert!(refused("guilds/templates/abc", Get).contains("can't tie"));
    assert!(refused("", Get).contains("empty"));
}

#[test]
fn paths_are_normalized() {
    assert_eq!(
        normalize("/api/v10/guilds/1000/bans?limit=10&after=5"),
        (
            "guilds/1000/bans".to_owned(),
            vec![("limit".to_owned(), "10".to_owned()), ("after".to_owned(), "5".to_owned())]
        )
    );
    assert_eq!(normalize("https://discord.com/api/v9/channels/1/"), ("channels/1".to_owned(), vec![]));
    assert_eq!(normalize("guilds/1"), ("guilds/1".to_owned(), vec![]));
}

#[test]
fn redaction_and_body_checks() {
    let mut v = json!({"token": "x", "nested": [{"url": "https://discord.com/api/webhooks/1/secret"}], "keep": "ok"});
    redact(&mut v);
    assert_eq!(v, json!({"nested": [{"url": "<redacted>"}], "keep": "ok"}));
    assert!(body_problem(&json!({"headers": {"Authorization": "Bot x"}})).is_some());
    assert!(body_problem(&json!({"note": "https://discord.com/api/webhooks/1/abc"})).is_some());
    let token = format!("{}.{}.{}", "M".repeat(24), "GabcDe", "x".repeat(27));
    assert!(looks_like_bot_token(&token));
    assert!(body_problem(&json!({"value": token})).is_some());
    assert!(body_problem(&json!({"description": "a lab"})).is_none());
}
