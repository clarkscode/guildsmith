//! T039: the secret-leak harness (SC-006). Seeds the canary token and checks it appears in no
//! output, log or error. Each story adds its commands and tools here.

mod common;

use common::CANARY;
use guildsmith::logging::Scrubber;
use guildsmith_app::config::{Dirs, LoadOptions, MapEnv, NoStore, Os, load};
use guildsmith_app::ports::PortError;
use guildsmith_app::secret::Secret;

fn assert_clean(what: &str, text: &str) {
    assert!(!text.contains(CANARY), "{what} leaked the token:\n{text}");
}

#[test]
fn mcp_session_at_trace_never_shows_the_token() {
    let session = common::mcp_session(&[("no_such_tool", serde_json::json!({"token": "x"}))]);
    let out = common::run(&["mcp", "--log", "trace"], &[("DISCORD_TOKEN", CANARY)], &session);
    assert_clean("mcp stdout", &out.stdout);
    assert_clean("mcp stderr", &out.stderr);
}

#[test]
fn bad_config_errors_never_show_the_token() {
    let out = common::run(&["mcp"], &[("DISCORD_TOKEN", CANARY), ("GUILDSMITH_TOOLSETS", "core,bogus")], "");
    assert_eq!(out.code, Some(1));
    assert!(out.stderr.contains("unknown toolset `bogus`"), "{}", out.stderr);
    assert_clean("config error", &out.stderr);
    assert_clean("config error stdout", &out.stdout);
}

#[test]
fn config_and_errors_redact_the_token() {
    let env = MapEnv::new(&[("DISCORD_TOKEN", CANARY)]);
    let read = |_: &std::path::Path| Err(std::io::Error::from(std::io::ErrorKind::NotFound));
    let cfg = load(&LoadOptions {
        env: &env,
        config_path: None,
        dirs: Dirs { os: Os::Linux, home: None, appdata: None },
        store: &NoStore,
        read_file: &read,
    })
    .unwrap();
    assert_clean("Config Debug", &format!("{cfg:?}"));
    assert_clean("Secret Display", &Secret::new(CANARY).to_string());
    for e in [PortError::NoToken, PortError::NotBotToken, PortError::Unauthorized] {
        assert_clean("PortError", &e.to_string());
    }
}

#[test]
fn log_scrubber_removes_the_token_and_auth_headers() {
    let s = Scrubber::new(vec![CANARY.to_owned()]);
    assert_clean("scrubbed", &s.scrub(&format!("sending Authorization: Bot {CANARY}")));
    assert_clean("scrubbed", &s.scrub(&format!("{{\"token\":\"{CANARY}\"}}")));
}

#[tokio::test]
async fn us1_commands_never_show_the_token() {
    use clap::Parser;
    use guildsmith::cli::Cli;
    use guildsmith_app::App;
    use guildsmith_app::fake::{FakeDiscord, fixtures};
    use guildsmith_app::policy::Policy;

    let spec = std::env::temp_dir().join(format!("guildsmith-leak-{}.toml", std::process::id()));
    std::fs::write(&spec, format!("[guild]\nname = \"{CANARY}x\"\n[[roles]]\nnmae = \"x\"\n")).unwrap();
    let spec_path = spec.to_str().unwrap().to_owned();
    for args in [
        vec!["guilds"],
        vec!["guilds", "--json"],
        vec!["export", "-g", "1000"],
        vec!["export", "-g", "2000"],
        vec!["schema"],
    ] {
        let cli = Cli::try_parse_from(std::iter::once("guildsmith").chain(args.iter().copied())).unwrap();
        let app = std::sync::Arc::new(App::new(
            FakeDiscord::new(vec![(fixtures::rich(), fixtures::lab_members())]),
            Policy::default(),
        ));
        let (mut out, mut err) = (Vec::new(), Vec::new());
        guildsmith::dispatch(&cli, app, &mut out, &mut err).await;
        assert_clean(&format!("{args:?} stdout"), &String::from_utf8_lossy(&out));
        assert_clean(&format!("{args:?} stderr"), &String::from_utf8_lossy(&err));
    }
    let _ = (spec_path, std::fs::remove_file(&spec));
}

/// Trace logs captured in memory, with only the generic `Authorization` scrubbing (not the
/// known-token scrub), so a token logged anywhere else shows up.
#[derive(Clone, Default)]
struct Captured(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = Scrubber::default().scrub(&String::from_utf8_lossy(buf));
        self.0.lock().unwrap().extend_from_slice(text.as_bytes());
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn discord_fixture(name: &str) -> serde_json::Value {
    let path = format!("{}/../guildsmith-discord/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// A mock Discord: reads succeed, every write fails, and the token travels in each request.
async fn mock_discord() -> wiremock::MockServer {
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, ResponseTemplate};
    let s = wiremock::MockServer::start().await;
    let get = |p: &str, body: serde_json::Value| {
        Mock::given(method("GET"))
            .and(path(format!("/api/v10/{p}")))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
    };
    get("users/@me", discord_fixture("users_me.json")).mount(&s).await;
    get(
        "users/@me/guilds",
        serde_json::json!([{"id": "1000", "name": "Guildsmith Lab", "icon": null, "owner": false,
        "permissions": "8", "features": []}]),
    )
    .mount(&s)
    .await;
    get("guilds/1000", discord_fixture("guild.json")).mount(&s).await;
    get("guilds/1000/channels", discord_fixture("channels.json")).mount(&s).await;
    get("guilds/1000/members/2", discord_fixture("bot_member.json")).mount(&s).await;
    get("guilds/1000/members/search", serde_json::json!([])).mount(&s).await;
    get("channels/101/messages", serde_json::json!([])).mount(&s).await;
    Mock::given(wiremock::matchers::any())
        .respond_with(
            ResponseTemplate::new(403)
                .set_body_json(serde_json::json!({"message": "Missing Permissions", "code": 50013})),
        )
        .mount(&s)
        .await;
    s
}

/// Every CLI command and MCP tool through the real adapter, including failures: the canary
/// token reaches the mock in every request, but never any output, error or log line.
#[test]
fn every_command_and_tool_keeps_the_token_out_of_outputs_and_logs() {
    use clap::Parser;
    use guildsmith::cli::Cli;
    use guildsmith_app::App;
    use guildsmith_app::policy::{Policy, Toolset};
    use guildsmith_discord::TwilightDiscord;
    use rmcp::ServiceExt;

    let logs = Captured::default();
    let sink = logs.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .with_ansi(false)
        .with_writer(move || sink.clone())
        .finish();
    let mut outputs = String::new();
    tracing::subscriber::with_default(subscriber, || {
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let mock = mock_discord().await;
            let policy = Policy { toolsets: [Toolset::Core, Toolset::Moderation].into_iter().collect(), ..Policy::default() };
            let app = std::sync::Arc::new(App::new(
                TwilightDiscord::with_proxy(Some(Secret::new(CANARY)), mock.address().to_string()),
                policy,
            ));
            let spec = std::env::temp_dir().join(format!("guildsmith-leak-live-{}.toml", std::process::id()));
            std::fs::write(&spec, "[[roles]]\nname = \"Staff\"\n").unwrap();
            let spec = spec.to_str().unwrap().to_owned();
            for args in [
                vec!["guilds", "--json"],
                vec!["export", "-g", "1000"],
                vec!["plan", "-g", "1000", &spec],
                vec!["apply", "-g", "1000", &spec, "--yes", "--plan-hash", "gsp_00000000000000000000000000000000"],
                vec!["apply", "-g", "1000", &spec],
                vec!["doctor", "-g", "1000"],
            ] {
                let cli = Cli::try_parse_from(std::iter::once("guildsmith").chain(args.iter().copied())).unwrap();
                let (mut out, mut err) = (Vec::new(), Vec::new());
                guildsmith::dispatch_with(&cli, app.clone(), &mut out, &mut err, &mut |_| Some(true)).await;
                outputs.push_str(&String::from_utf8_lossy(&out));
                outputs.push_str(&String::from_utf8_lossy(&err));
            }

            let (server_io, client_io) = tokio::io::duplex(1 << 20);
            let server = guildsmith_mcp::GuildsmithServer::new(app.clone());
            tokio::spawn(async move {
                if let Ok(running) = server.serve(server_io).await {
                    let _ = running.waiting().await;
                }
            });
            let client = ().serve(client_io).await.unwrap();
            let g = "1000";
            for (tool, args) in [
                ("guilds_list", serde_json::json!({})),
                ("guild_overview", serde_json::json!({"guild": g})),
                ("refresh", serde_json::json!({})),
                ("spec_export", serde_json::json!({"guild": g})),
                ("spec_plan", serde_json::json!({"guild": g, "toml": "[[roles]]\nname = \"Staff\"\n"})),
                ("spec_apply", serde_json::json!({"guild": g, "toml": "[[roles]]\nname = \"Staff\"\n", "plan_hash": "gsp_x"})),
                ("roles_list", serde_json::json!({"guild": g})),
                ("role_upsert", serde_json::json!({"guild": g, "name": "Staff"})),
                ("role_delete", serde_json::json!({"guild": g, "role": "@Member", "confirm": true})),
                ("channels_tree", serde_json::json!({"guild": g})),
                ("channel_upsert", serde_json::json!({"guild": g, "name": "new"})),
                ("channel_delete", serde_json::json!({"guild": g, "channel": "#general", "confirm": true})),
                ("channels_reorder", serde_json::json!({"guild": g, "order": ["#general"]})),
                ("permissions_set", serde_json::json!({"guild": g, "channel": "#general", "target": "@everyone", "allow": ["view_channel"]})),
                ("permissions_explain", serde_json::json!({"guild": g, "channel": "#general", "who": "@everyone"})),
                ("members_find", serde_json::json!({"guild": g, "query": "a"})),
                ("member_roles", serde_json::json!({"guild": g, "member": "50", "add": ["Moderator"]})),
                ("message_send", serde_json::json!({"guild": g, "channel": "#general", "content": "hi"})),
                ("messages_read", serde_json::json!({"guild": g, "channel": "#general"})),
                ("bot_preflight", serde_json::json!({"guild": g})),
                ("member_timeout", serde_json::json!({"guild": g, "member": "50", "duration": 60})),
                ("member_kick", serde_json::json!({"guild": g, "member": "50", "confirm": true})),
                ("member_ban", serde_json::json!({"guild": g, "member": "50", "confirm": true})),
                ("ban_remove", serde_json::json!({"guild": g, "user": "50"})),
            ] {
                let params = rmcp::model::CallToolRequestParams::new(tool).with_arguments(args.as_object().unwrap().clone());
                let res = client.call_tool(params).await.unwrap();
                outputs.push_str(&serde_json::to_string(&res).unwrap());
            }
            let requests = mock.received_requests().await.unwrap();
            assert!(requests.len() > 10);
            assert!(
                requests.iter().all(|r| r.headers.get("authorization").and_then(|v| v.to_str().ok()) == Some(&format!("Bot {CANARY}"))),
                "the canary was really used"
            );
        });
    });
    assert!(
        outputs.contains("Missing Permissions") || outputs.contains("missing"),
        "failures were exercised:\n{outputs}"
    );
    assert_clean("outputs", &outputs);
    let logs = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
    assert!(logs.contains("TRACE") || logs.contains("DEBUG"), "trace logging was on");
    assert_clean("logs", &logs);
}
