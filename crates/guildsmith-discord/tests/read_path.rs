//! T014: the adapter's read path against recorded Discord responses (wiremock).

use guildsmith_app::ports::{DiscordPort, PortError};
use guildsmith_app::secret::Secret;
use guildsmith_core::snapshot::{Completeness, LiveChannelKind, OverwriteKind};
use guildsmith_core::spec::ChannelKind;
use guildsmith_core::{Id, Permission};
use guildsmith_discord::TwilightDiscord;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> serde_json::Value {
    let text = std::fs::read_to_string(format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))).unwrap();
    serde_json::from_str(&text).unwrap()
}

async fn server(guild_fixture: &str) -> MockServer {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v10/users/@me"))
        .and(header("authorization", "Bot test-token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("users_me.json")))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/1000"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture(guild_fixture)))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/1000/channels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("channels.json")))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/1000/members/2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("bot_member.json")))
        .mount(&s)
        .await;
    s
}

fn client(s: &MockServer) -> TwilightDiscord {
    TwilightDiscord::with_proxy(Some(Secret::new("test-token")), s.address().to_string())
}

#[tokio::test]
async fn snapshot_maps_every_kind() {
    let s = server("guild.json").await;
    let snap = client(&s).snapshot(Id(1000)).await.unwrap();

    assert_eq!(snap.guild.name, "Guildsmith Lab");
    assert_eq!(snap.guild.owner_id, Id(1));
    assert!(snap.guild.features.contains("NEWS"));

    let bot_role = snap.role(Id(9)).unwrap();
    assert!(bot_role.managed);
    assert_eq!(bot_role.tags.as_ref().unwrap().bot_id, Some(Id(2)));
    let moderator = snap.role(Id(10)).unwrap();
    assert_eq!(moderator.color, 3_909_981);
    assert_eq!(moderator.secondary_color, Some(16_711_680), "gradient colors are kept read-only");
    assert!(moderator.permissions.contains(Permission::KickMembers));

    let kind = |id: u64| snap.channel(Id(id)).unwrap().kind;
    assert_eq!(kind(100), LiveChannelKind::Category);
    assert_eq!(kind(101), LiveChannelKind::Known(ChannelKind::Text));
    assert_eq!(kind(104), LiveChannelKind::Known(ChannelKind::Announcement));
    assert_eq!(kind(103), LiveChannelKind::Known(ChannelKind::Voice));
    assert_eq!(kind(105), LiveChannelKind::Known(ChannelKind::Stage));
    assert_eq!(kind(106), LiveChannelKind::Known(ChannelKind::Forum));
    assert_eq!(kind(107), LiveChannelKind::Known(ChannelKind::Media));
    assert_eq!(kind(108), LiveChannelKind::Other(14), "unsupported kinds are kept, not dropped");

    let general = snap.channel(Id(101)).unwrap();
    assert_eq!(general.parent_id, Some(Id(100)));
    assert_eq!(general.topic.as_deref(), Some("Talk about anything"));
    assert_eq!(general.slowmode, 5);

    let mod_log = snap.channel(Id(102)).unwrap();
    let everyone = mod_log.overwrites.iter().find(|o| o.id == Id(1000)).unwrap();
    assert_eq!(everyone.kind, OverwriteKind::Role);
    assert!(everyone.deny.contains(Permission::ViewChannel));

    let lobby = snap.channel(Id(103)).unwrap();
    assert_eq!((lobby.bitrate, lobby.user_limit, lobby.rtc_region.as_deref()), (Some(64_000), Some(10), None));
    assert_eq!(snap.channel(Id(105)).unwrap().rtc_region.as_deref(), Some("us-west"));

    let help = snap.channel(Id(106)).unwrap();
    assert_eq!(help.tags.len(), 2);
    assert!(help.tags.iter().any(|t| t.name == "solved" && t.moderated));
    assert_eq!(help.default_reaction.as_deref(), Some("👍"));

    assert_eq!(snap.bot.user_id, Id(2));
    assert_eq!(snap.bot.role_ids, vec![Id(9)]);
    assert_eq!(snap.bot.top_role_position, 3);
    assert!(snap.bot.permissions.contains(Permission::Administrator));
    assert_eq!(snap.completeness, Completeness::Complete);
}

#[tokio::test]
async fn non_admin_bot_snapshot_is_maybe_incomplete() {
    let s = server("guild_not_admin.json").await;
    let snap = client(&s).snapshot(Id(1000)).await.unwrap();
    assert_eq!(snap.completeness, Completeness::MaybeIncomplete);
    assert!(!snap.bot.permissions.contains(Permission::Administrator));
    assert!(snap.bot.permissions.contains(Permission::ManageRoles));
}

#[tokio::test]
async fn reads_are_retried_on_5xx() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v10/users/@me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("users_me.json")))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/1000"))
        .respond_with(
            ResponseTemplate::new(502).set_body_json(serde_json::json!({"message": "bad gateway", "code": 0})),
        )
        .up_to_n_times(2)
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/1000"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("guild.json")))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/1000/channels"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("channels.json")))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/1000/members/2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("bot_member.json")))
        .mount(&s)
        .await;
    let snap = client(&s).snapshot(Id(1000)).await.unwrap();
    assert_eq!(snap.guild.id, Id(1000));
    let guild_gets =
        s.received_requests().await.unwrap().iter().filter(|r| r.url.path() == "/api/v10/guilds/1000").count();
    assert_eq!(guild_gets, 3, "two 502s, then success");
}

#[tokio::test]
async fn user_account_tokens_are_refused() {
    let s = MockServer::start().await;
    let mut me = fixture("users_me.json");
    me["bot"] = serde_json::Value::Bool(false);
    Mock::given(method("GET"))
        .and(path("/api/v10/users/@me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(me))
        .mount(&s)
        .await;
    let err = client(&s).snapshot(Id(1000)).await.unwrap_err();
    assert_eq!(err, PortError::NotBotToken);
}

#[tokio::test]
async fn bearer_tokens_and_missing_tokens_are_refused() {
    let s = MockServer::start().await;
    let c = TwilightDiscord::with_proxy(Some(Secret::new("Bearer abc")), s.address().to_string());
    assert_eq!(c.current_user().await.unwrap_err(), PortError::NotBotToken);
    let c = TwilightDiscord::with_proxy(None, s.address().to_string());
    assert_eq!(c.current_user().await.unwrap_err(), PortError::NoToken);
    assert!(s.received_requests().await.unwrap().is_empty(), "no request is sent without a usable token");
}

#[tokio::test]
async fn missing_guild_maps_to_not_in_guild() {
    let s = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v10/users/@me"))
        .respond_with(ResponseTemplate::new(200).set_body_json(fixture("users_me.json")))
        .mount(&s)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/1000"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(serde_json::json!({"message": "Unknown Guild", "code": 10004})),
        )
        .mount(&s)
        .await;
    assert_eq!(client(&s).snapshot(Id(1000)).await.unwrap_err(), PortError::NotInGuild(Id(1000)));
}
