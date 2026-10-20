//! The messages, threads and polls adapter against a mock Discord: request bodies (mentions off
//! by default, replies, polls, Components V2), multipart uploads, the paginated pins route, forum
//! posts with tags, and reading messages back.

use guildsmith_app::ports::{
    AllowedMentions, FileData, MessagesPort, NewPoll, NewThread, OutgoingMessage, PollsPort, ThreadsPort,
};
use guildsmith_app::secret::Secret;
use guildsmith_core::Id;
use guildsmith_discord::TwilightDiscord;
use serde_json::{Value, json};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

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

fn message(id: &str) -> Value {
    json!({"id": id, "channel_id": "101", "author": {"id": "2", "username": "guildsmith", "bot": true},
        "content": "hi", "timestamp": "2026-10-19T12:00:00+00:00", "edited_timestamp": null, "pinned": false,
        "flags": 0, "embeds": [], "attachments": [], "mentions": [], "type": 0,
        "reactions": [{"count": 2, "me": true, "emoji": {"id": null, "name": "✅"}}],
        "poll": {"question": {"text": "Q?"}, "answers": [{"answer_id": 1, "poll_media": {"text": "A"}}],
            "expiry": "2026-10-20T12:00:00+00:00", "allow_multiselect": false, "layout_type": 1,
            "results": {"is_finalized": false, "answer_counts": [{"id": 1, "count": 3, "me_voted": false}]}}})
}

async fn requests(s: &MockServer, verb: &str, p: &str) -> Vec<Request> {
    s.received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.method.as_str() == verb && r.url.path() == p)
        .collect()
}

#[tokio::test]
async fn posts_send_mentions_off_by_default_replies_and_polls() {
    let s = server().await;
    Mock::given(method("POST"))
        .and(path("/api/v10/channels/101/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(message("900")))
        .mount(&s)
        .await;
    let c = client(&s);
    let msg = OutgoingMessage {
        content: Some("@everyone hi".to_owned()),
        reply_to: Some(Id(899)),
        poll: Some(NewPoll {
            question: "Q?".to_owned(),
            answers: vec![("A".to_owned(), None), ("B".to_owned(), Some("🔥".to_owned()))],
            duration_hours: 24,
            multiselect: true,
        }),
        ..OutgoingMessage::default()
    };
    let m = c.post(Id(101), &msg).await.unwrap();
    assert_eq!(m.id, Id(900));
    assert_eq!(m.reactions[0].emoji, "✅");
    assert_eq!(m.poll.as_ref().unwrap().answers[0].votes, 3);
    let req = &requests(&s, "POST", "/api/v10/channels/101/messages").await[0];
    let body: Value = serde_json::from_slice(&req.body).unwrap();
    assert_eq!(body["allowed_mentions"], json!({"parse": [], "users": [], "roles": [], "replied_user": false}));
    assert_eq!(body["message_reference"]["message_id"], "899");
    assert_eq!(body["poll"]["duration"], 24);
    assert_eq!(body["poll"]["answers"][1]["poll_media"], json!({"text": "B", "emoji": {"name": "🔥"}}));

    let v2 = OutgoingMessage {
        components: Some(json!([{"type": 10, "content": "x"}])),
        flags: 1 << 15,
        allowed_mentions: AllowedMentions { everyone: true, ..AllowedMentions::default() },
        forward: Some((Id(102), Id(55))),
        ..OutgoingMessage::default()
    };
    c.post(Id(101), &v2).await.unwrap();
    let req = &requests(&s, "POST", "/api/v10/channels/101/messages").await[1];
    let body: Value = serde_json::from_slice(&req.body).unwrap();
    assert_eq!(body["flags"], 1 << 15);
    assert_eq!(body["allowed_mentions"]["parse"], json!(["everyone"]));
    assert_eq!(body["message_reference"], json!({"type": 1, "channel_id": "102", "message_id": "55"}));
    assert!(body.get("content").is_none());
}

#[tokio::test]
async fn files_go_as_multipart_with_payload_json() {
    let s = server().await;
    Mock::given(method("POST"))
        .and(path("/api/v10/channels/101/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(message("901")))
        .mount(&s)
        .await;
    let msg = OutgoingMessage {
        content: Some("notes".to_owned()),
        files: vec![FileData { name: "notes.txt".to_owned(), bytes: b"hello file".to_vec() }],
        ..OutgoingMessage::default()
    };
    client(&s).post(Id(101), &msg).await.unwrap();
    let req = &requests(&s, "POST", "/api/v10/channels/101/messages").await[0];
    let ct = req.headers.get("content-type").unwrap().to_str().unwrap().to_owned();
    assert!(ct.starts_with("multipart/form-data"), "{ct}");
    let body = String::from_utf8_lossy(&req.body);
    assert!(body.contains("name=\"payload_json\""), "{body}");
    assert!(
        body.contains("\"attachments\":[{\"filename\":\"notes.txt\",\"id\":0}]")
            || body.contains("\"attachments\":[{\"id\":0,\"filename\":\"notes.txt\"}]"),
        "{body}"
    );
    assert!(body.contains("name=\"files[0]\"; filename=\"notes.txt\""), "{body}");
    assert!(body.contains("hello file"));
}

#[tokio::test]
async fn pins_use_the_paginated_route_and_reactions_encode_emoji() {
    let s = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v10/channels/101/messages/pins"))
        .and(query_param("limit", "50"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "items": [{"pinned_at": "2026-10-19T12:00:00+00:00", "message": message("902")}], "has_more": true
        })))
        .mount(&s)
        .await;
    Mock::given(method("PUT"))
        .and(path("/api/v10/channels/101/messages/pins/902"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&s)
        .await;
    Mock::given(method("PUT")).respond_with(ResponseTemplate::new(204)).mount(&s).await;
    let c = client(&s);
    let (pins, more) = c.pins(Id(101), None, 50).await.unwrap();
    assert!(more);
    assert_eq!(pins[0].1, "2026-10-19T12:00:00+00:00");
    c.set_pinned(Id(101), Id(902), true, "guildsmith: pin").await.unwrap();
    c.react(Id(101), Id(902), "✅").await.unwrap();
    c.react(Id(101), Id(902), "party:777").await.unwrap();
    let paths: Vec<String> = s
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .filter(|r| r.url.path().contains("/reactions/"))
        .map(|r| r.url.path().to_owned())
        .collect();
    assert_eq!(
        paths,
        [
            "/api/v10/channels/101/messages/902/reactions/%E2%9C%85/@me",
            "/api/v10/channels/101/messages/902/reactions/party%3A777/@me"
        ]
    );
}

#[tokio::test]
async fn forum_posts_carry_tags_and_their_first_message() {
    let s = server().await;
    Mock::given(method("POST"))
        .and(path("/api/v10/channels/106/threads"))
        .respond_with(ResponseTemplate::new(201).set_body_json(json!({
            "id": "950", "type": 11, "parent_id": "106", "name": "Login fails", "flags": 0, "owner_id": "2",
            "message_count": 1, "member_count": 1, "applied_tags": ["201"],
            "thread_metadata": {"archived": false, "locked": false, "auto_archive_duration": 1440,
                "archive_timestamp": "2026-10-19T12:00:00+00:00"}
        })))
        .mount(&s)
        .await;
    let t = NewThread {
        name: "Login fails".to_owned(),
        auto_archive: Some(1440),
        tags: vec![Id(201)],
        post: Some(OutgoingMessage { content: Some("help".to_owned()), ..OutgoingMessage::default() }),
        ..NewThread::default()
    };
    let info = client(&s).create_thread(Id(106), &t, "guildsmith: create post").await.unwrap();
    assert_eq!((info.id, info.kind.as_str(), info.tags.clone()), (Id(950), "public", vec![Id(201)]));
    let req = &requests(&s, "POST", "/api/v10/channels/106/threads").await[0];
    let body: Value = serde_json::from_slice(&req.body).unwrap();
    assert_eq!(body["applied_tags"], json!(["201"]));
    assert_eq!(body["message"]["content"], "help");
    assert_eq!(body["auto_archive_duration"], 1440);
    assert!(req.headers.get("x-audit-log-reason").is_some());
}

#[tokio::test]
async fn poll_voters_and_ending() {
    let s = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v10/channels/101/polls/900/answers/1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"users": [{"id": "51", "username": "sam"}]})))
        .mount(&s)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/v10/channels/101/polls/900/expire"))
        .respond_with(ResponseTemplate::new(200).set_body_json(message("900")))
        .expect(1)
        .mount(&s)
        .await;
    let c = client(&s);
    let voters = c.answer_voters(Id(101), Id(900), 1, None, 50).await.unwrap();
    assert_eq!((voters[0].id, voters[0].name.as_str()), (Id(51), "sam"));
    assert_eq!(c.end_poll(Id(101), Id(900)).await.unwrap().id, Id(900));
}
