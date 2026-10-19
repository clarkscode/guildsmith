//! T056: MCP protocol tests for `spec_plan` and `spec_apply`, including progress
//! notifications (FR-014).

use std::sync::{Arc, Mutex};

use guildsmith_app::App;
use guildsmith_app::fake::{FakeDiscord, fixtures};
use guildsmith_app::policy::Policy;
use guildsmith_mcp::GuildsmithServer;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ClientConfig, NumberOrString, ProgressNotificationParam, ProgressToken,
    RequestMetaObject, Tool,
};
use rmcp::service::{NotificationContext, RunningService};
use rmcp::{ClientHandler, RoleClient, ServiceExt};
use serde_json::{Value, json};

#[derive(Clone, Default)]
struct Client {
    progress: Arc<Mutex<Vec<ProgressNotificationParam>>>,
}

impl ClientHandler for Client {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::default()
    }

    async fn on_progress(&self, params: ProgressNotificationParam, _: NotificationContext<RoleClient>) {
        self.progress.lock().unwrap().push(params);
    }
}

const SPEC: &str = "[[roles]]\nname = \"Staff\"\n[[roles]]\nname = \"Moderator\"\ncolor = \"#010203\"\n";

async fn connect(app: Arc<App<FakeDiscord>>, client: Client) -> RunningService<RoleClient, Client> {
    let (server_io, client_io) = tokio::io::duplex(1 << 20);
    let server = GuildsmithServer::new(app);
    tokio::spawn(async move {
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    client.serve(client_io).await.expect("client connects")
}

fn lab() -> Arc<App<FakeDiscord>> {
    Arc::new(App::new(fixtures::lab_fake(), Policy::default()))
}

async fn call(c: &RunningService<RoleClient, Client>, name: &'static str, args: Value) -> CallToolResult {
    c.call_tool(CallToolRequestParams::new(name).with_arguments(args.as_object().unwrap().clone())).await.expect("call")
}

fn tool<'a>(tools: &'a [Tool], name: &str) -> &'a Tool {
    tools.iter().find(|t| t.name == name).unwrap_or_else(|| panic!("tool {name} not listed"))
}

fn assert_schema(t: &Tool, res: &CallToolResult) {
    let schema = Value::Object(t.output_schema.as_ref().expect("outputSchema").as_ref().clone());
    let value = res.structured_content.as_ref().expect("structuredContent");
    if let Err(e) = jsonschema::validate(&schema, value) {
        panic!("{} output doesn't match its outputSchema: {e}\n{value:#}", t.name);
    }
}

#[tokio::test]
async fn spec_plan_is_read_only_and_returns_the_plan() {
    let c = connect(lab(), Client::default()).await;
    let tools = c.list_all_tools().await.unwrap();
    let a = tool(&tools, "spec_plan").annotations.clone().unwrap();
    assert_eq!((a.read_only_hint, a.destructive_hint), (Some(true), Some(false)));
    let a = tool(&tools, "spec_apply").annotations.clone().unwrap();
    assert_eq!((a.read_only_hint, a.destructive_hint), (Some(false), Some(true)));

    let res = call(&c, "spec_plan", json!({"guild": "1000", "toml": SPEC})).await;
    assert_eq!(res.is_error, Some(false), "{:?}", res.content);
    assert_schema(tool(&tools, "spec_plan"), &res);
    let sc = res.structured_content.unwrap();
    for key in ["plan_hash", "risk", "ops", "unmanaged", "warnings", "estimated_requests"] {
        assert!(sc.get(key).is_some(), "missing {key}: {sc:#}");
    }
    assert_eq!(sc["ops"][0], json!({"summary": "create role @Staff", "risk": "safe"}));
}

#[tokio::test]
async fn spec_apply_refuses_a_made_up_hash() {
    let app = lab();
    let c = connect(app.clone(), Client::default()).await;
    let res = call(&c, "spec_apply", json!({"guild": "1000", "toml": SPEC, "plan_hash": "gsp_0123"})).await;
    assert_eq!(res.is_error, Some(true));
    assert_eq!(res.structured_content.unwrap()["refusal"]["kind"], "plan_mismatch");
    assert_eq!(app.port.writes(), []);
}

#[tokio::test]
async fn spec_apply_runs_the_reviewed_plan_with_progress() {
    let app = lab();
    let client = Client::default();
    let c = connect(app.clone(), client.clone()).await;
    let tools = c.list_all_tools().await.unwrap();
    let plan = call(&c, "spec_plan", json!({"guild": "1000", "toml": SPEC})).await.structured_content.unwrap();
    let n = plan["ops"].as_array().unwrap().len();

    let mut meta = RequestMetaObject::new();
    meta.set_progress_token(ProgressToken(NumberOrString::Number(7)));
    let mut params = CallToolRequestParams::new("spec_apply").with_arguments(
        json!({"guild": "1000", "toml": SPEC, "plan_hash": plan["plan_hash"]}).as_object().unwrap().clone(),
    );
    params.meta = Some(meta);
    let res = c.call_tool(params).await.unwrap();
    assert_eq!(res.is_error, Some(false), "{:?}", res.content);
    assert_schema(tool(&tools, "spec_apply"), &res);
    let sc = res.structured_content.unwrap();
    assert_eq!(sc["applied"].as_array().unwrap().len(), n);
    assert_eq!(sc["skipped"], json!([]));
    assert!(sc.get("failed").is_none());

    let progress = client.progress.lock().unwrap().clone();
    assert_eq!(progress.len(), n, "one progress notification per op");
    assert_eq!(progress[0].total, Some(f64::from(u32::try_from(n).unwrap())));
    assert_eq!(progress[0].message.as_deref(), Some("create role @Staff"));

    let again = call(&c, "spec_plan", json!({"guild": "1000", "toml": SPEC})).await.structured_content.unwrap();
    assert_eq!(again["ops"], json!([]), "converged");
}

#[tokio::test]
async fn a_failed_apply_reports_applied_failed_and_skipped() {
    let app = lab();
    let c = connect(app.clone(), Client::default()).await;
    let plan = call(&c, "spec_plan", json!({"guild": "1000", "toml": SPEC})).await.structured_content.unwrap();
    app.port.fail_write_at(2);
    let res = call(&c, "spec_apply", json!({"guild": "1000", "toml": SPEC, "plan_hash": plan["plan_hash"]})).await;
    let sc = res.structured_content.unwrap();
    assert_eq!(sc["applied"].as_array().unwrap().len(), 1);
    assert!(sc["failed"]["error"].as_str().unwrap().contains("injected failure"));
    assert_ne!(sc["skipped"], json!([]));
}
