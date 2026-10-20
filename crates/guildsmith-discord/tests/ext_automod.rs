//! T055: automod bodies against a mock Discord: unset fields are left out (twilight 0.17.1's
//! update sends `null`s), and `mention_raid_protection_enabled`, the member-profile trigger and
//! the block-member-interaction action serialize correctly.

use guildsmith_app::ports::AutomodPort;
use guildsmith_app::secret::Secret;
use guildsmith_core::Id;
use guildsmith_core::spec::automod::{Action, AutomodRule, Keywords, Trigger};
use guildsmith_discord::TwilightDiscord;
use serde_json::{Value, json};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn server() -> MockServer {
    let s = MockServer::start().await;
    let me = std::fs::read_to_string(format!("{}/tests/fixtures/users_me.json", env!("CARGO_MANIFEST_DIR"))).unwrap();
    Mock::given(method("GET"))
        .and(path("/api/v10/users/@me"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(me, "application/json"))
        .mount(&s)
        .await;
    s
}

fn client(s: &MockServer) -> TwilightDiscord {
    TwilightDiscord::with_proxy(Some(Secret::new("test-token")), s.address().to_string())
}

fn echo(body: &Value) -> Value {
    let mut v = body.clone();
    v["id"] = json!("800");
    v["guild_id"] = json!("1000");
    v["creator_id"] = json!("2");
    if v.get("trigger_type").is_none() {
        v["trigger_type"] = json!(5);
    }
    v
}

async fn sent(s: &MockServer, verb: &str) -> Value {
    let r = s
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.method.as_str() == verb && r.url.path().contains("auto-moderation"));
    serde_json::from_slice(&r.expect("sent").body).unwrap()
}

#[tokio::test]
async fn mention_spam_with_raid_protection_and_timeout() {
    let s = server().await;
    let rule = AutomodRule {
        id: None,
        name: "No mass pings".to_owned(),
        enabled: true,
        trigger: Trigger::MentionSpam { limit: 10, raid_protection: true },
        actions: vec![Action::Timeout { duration: 3600 }, Action::Alert { channel: Id(102) }],
        exempt_roles: vec![Id(10)],
        exempt_channels: Vec::new(),
    };
    let expected = json!({
        "name": "No mass pings", "event_type": 1, "trigger_type": 5,
        "trigger_metadata": {"mention_total_limit": 10, "mention_raid_protection_enabled": true},
        "actions": [{"type": 3, "metadata": {"duration_seconds": 3600}}, {"type": 2, "metadata": {"channel_id": "102"}}],
        "enabled": true, "exempt_roles": ["10"], "exempt_channels": []
    });
    Mock::given(method("POST"))
        .and(path("/api/v10/guilds/1000/auto-moderation/rules"))
        .respond_with(ResponseTemplate::new(200).set_body_json(echo(&expected)))
        .mount(&s)
        .await;
    let created = client(&s).create_automod(Id(1000), &rule, "r").await.unwrap();
    assert_eq!(sent(&s, "POST").await, expected);
    assert_eq!(created.id, Some(Id(800)));
    assert_eq!(created.trigger, rule.trigger);
    assert_eq!(created.actions, rule.actions);
}

#[tokio::test]
async fn member_profile_with_block_interaction_and_updates_leave_out_trigger_type() {
    let s = server().await;
    let rule = AutomodRule {
        id: Some(Id(801)),
        name: "Clean names".to_owned(),
        enabled: false,
        trigger: Trigger::MemberProfile(Keywords {
            keywords: vec!["badword".to_owned()],
            regex: Vec::new(),
            allow: Vec::new(),
        }),
        actions: vec![Action::BlockInteraction],
        exempt_roles: Vec::new(),
        exempt_channels: Vec::new(),
    };
    let mut reply = json!({"name": "Clean names", "event_type": 2, "trigger_type": 6,
        "trigger_metadata": {"keyword_filter": ["badword"]}, "actions": [{"type": 4}], "enabled": false,
        "exempt_roles": [], "exempt_channels": []});
    reply = echo(&reply);
    Mock::given(method("PATCH"))
        .and(path("/api/v10/guilds/1000/auto-moderation/rules/801"))
        .respond_with(ResponseTemplate::new(200).set_body_json(reply))
        .mount(&s)
        .await;
    let updated = client(&s).update_automod(Id(1000), Id(801), &rule, "r").await.unwrap();
    let body = sent(&s, "PATCH").await;
    assert!(body.get("trigger_type").is_none(), "the trigger type can't change on update");
    assert_eq!(body["event_type"], 2);
    assert_eq!(body["trigger_metadata"], json!({"keyword_filter": ["badword"]}), "no nulls or empty lists");
    assert_eq!(body["actions"], json!([{"type": 4}]));
    assert!(!body.to_string().contains("null"), "{body}");
    assert_eq!(updated.actions, [Action::BlockInteraction]);
}

#[tokio::test]
async fn spam_rules_have_no_metadata_and_block_messages_carry_custom_text() {
    let s = server().await;
    let rule = AutomodRule {
        id: None,
        name: "Spam".to_owned(),
        enabled: true,
        trigger: Trigger::Spam,
        actions: vec![Action::Block { message: Some("No spam, please.".to_owned()) }],
        exempt_roles: Vec::new(),
        exempt_channels: Vec::new(),
    };
    let reply = echo(&json!({"name": "Spam", "event_type": 1, "trigger_type": 3, "actions": [{"type": 1,
        "metadata": {"custom_message": "No spam, please."}}], "enabled": true, "exempt_roles": [], "exempt_channels": []}));
    Mock::given(method("POST"))
        .and(path("/api/v10/guilds/1000/auto-moderation/rules"))
        .respond_with(ResponseTemplate::new(200).set_body_json(reply))
        .mount(&s)
        .await;
    client(&s).create_automod(Id(1000), &rule, "r").await.unwrap();
    let body = sent(&s, "POST").await;
    assert!(body.get("trigger_metadata").is_none());
    assert_eq!(body["actions"][0]["metadata"]["custom_message"], "No spam, please.");
}
