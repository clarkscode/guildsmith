//! T033: guild message search against a mock Discord: `limit` ≤ 25 and `offset` ≤ 9975 in
//! the query, a `202` with code `110000` means "not indexed yet" (with `retry_after`), and
//! `messages` is an array of arrays.

use guildsmith_app::ports::{MessagesPort, SearchOutcome, SearchQuery};
use guildsmith_app::secret::Secret;
use guildsmith_core::Id;
use guildsmith_discord::TwilightDiscord;
use serde_json::json;
use wiremock::matchers::{method, path, query_param};
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

fn message(id: &str, content: &str) -> serde_json::Value {
    json!({"id": id, "channel_id": "101", "author": {"id": "51", "username": "sam"}, "content": content,
        "timestamp": "2026-10-19T12:00:00+00:00", "edited_timestamp": null, "pinned": false, "flags": 0,
        "embeds": [], "attachments": [], "mentions": [], "type": 0})
}

#[tokio::test]
async fn search_sends_the_filters_and_reads_arrays_of_arrays() {
    let s = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/1000/messages/search"))
        .and(query_param("content", "deploy"))
        .and(query_param("limit", "25"))
        .and(query_param("offset", "9975"))
        .and(query_param("author_id", "51"))
        .and(query_param("has", "file"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "total_results": 2,
            "messages": [[message("901", "deploy done"), message("900", "context")], [message("899", "deploy now")]],
        })))
        .expect(1)
        .mount(&s)
        .await;
    let q = SearchQuery {
        content: Some("deploy".to_owned()),
        authors: vec![Id(51)],
        has: vec!["file".to_owned()],
        limit: 99,
        offset: 20_000,
        ..SearchQuery::default()
    };
    let SearchOutcome::Found { total, messages } = client(&s).search(Id(1000), &q).await.unwrap() else {
        panic!("found expected")
    };
    assert_eq!(total, 2);
    assert_eq!(messages.iter().map(|m| m.id).collect::<Vec<_>>(), [Id(901), Id(899)], "the hit, not its context");
    assert_eq!(messages[0].content, "deploy done");
}

#[tokio::test]
async fn a_202_with_110000_means_not_indexed_yet() {
    let s = server().await;
    Mock::given(method("GET"))
        .and(path("/api/v10/guilds/1000/messages/search"))
        .respond_with(ResponseTemplate::new(202).set_body_json(json!({
            "message": "Index not yet available. Try again later", "code": 110_000, "documents_indexed": 0, "retry_after": 2.5
        })))
        .mount(&s)
        .await;
    let q = SearchQuery { limit: 25, ..SearchQuery::default() };
    assert_eq!(client(&s).search(Id(1000), &q).await.unwrap(), SearchOutcome::NotIndexed { retry_after: 3 });
}
