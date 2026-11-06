//! On-demand mini web server for the `sch_sourcing` toolset's `open_sourcing_ui`
//! tool. Not a toolset itself (no `tools()`, not registered in
//! `router/registry.rs`) — pure support code, started as a background task
//! inside the already-running MCP server's Tokio runtime rather than as a
//! second OS process (unlike `open_schematic_viewer`, which spawns a separate
//! Tauri binary).
//!
//! Serves a single embedded static page plus a couple of thin JSON endpoints
//! that call straight into `sch_sourcing`'s handler functions in-process — no
//! second MCP round-trip. Bound to `127.0.0.1` only, never `0.0.0.0`: this is
//! a single-purpose local server for one browser tab, not a general-purpose
//! endpoint like the MCP HTTP transport in `konnect::transport::http`.

use crate::mcp::protocol::ToolContent;
use crate::tools::sch_sourcing;
use crate::tools::ToolContext;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::Arc;

const INDEX_HTML: &str = include_str!("../../assets/sourcing_ui/index.html");

#[derive(Clone)]
struct AppState {
    ctx: Arc<ToolContext>,
    root_schematic: PathBuf,
}

/// Bind a fresh `127.0.0.1` port, start serving in the background, and
/// return the assigned port immediately (the server task is detached — this
/// function does not await it).
pub(crate) async fn spawn(ctx: Arc<ToolContext>, root_schematic: PathBuf) -> anyhow::Result<u16> {
    let state = AppState {
        ctx,
        root_schematic,
    };

    let app = Router::new()
        .route("/", get(index))
        .route("/api/groups", get(get_groups).post(post_groups))
        .route("/api/refresh_all", post(post_refresh_all))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let port = listener.local_addr()?.port();

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::warn!("sourcing UI server stopped: {}", e);
        }
    });

    Ok(port)
}

async fn index() -> impl IntoResponse {
    Html(INDEX_HTML)
}

async fn get_groups(State(state): State<AppState>) -> impl IntoResponse {
    let args = json!({ "schematic": state.root_schematic.display().to_string() });
    to_response(sch_sourcing::handle_list_sourcing_groups(&args, state.ctx.as_ref()).await)
}

async fn post_groups(
    State(state): State<AppState>,
    Json(mut body): Json<Value>,
) -> impl IntoResponse {
    body["schematic"] = json!(state.root_schematic.display().to_string());
    to_response(sch_sourcing::handle_set_sourcing_fields(&body, state.ctx.as_ref()).await)
}

async fn post_refresh_all(State(state): State<AppState>) -> impl IntoResponse {
    let args = json!({ "schematic": state.root_schematic.display().to_string() });
    to_response(sch_sourcing::handle_refresh_sourcing_stock_price(&args, state.ctx.as_ref()).await)
}

/// Convert a tool handler's `CallToolResult` (a pretty-printed JSON string in
/// a text content block, per the MCP protocol shape) back into a JSON HTTP
/// response for the frontend.
fn to_response(
    result: anyhow::Result<crate::mcp::protocol::CallToolResult>,
) -> axum::response::Response {
    match result {
        Ok(r) => {
            let text = match r.content.first() {
                Some(ToolContent::Text { text }) => text.as_str(),
                _ => "",
            };
            if r.is_error {
                (StatusCode::BAD_REQUEST, text.to_string()).into_response()
            } else {
                match serde_json::from_str::<Value>(text) {
                    Ok(v) => (StatusCode::OK, Json(v)).into_response(),
                    Err(_) => (StatusCode::OK, text.to_string()).into_response(),
                }
            }
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{ServerConfig, ToolContext};
    use konnect_schematic_editor as cse;
    use tempfile::TempDir;

    fn test_ctx() -> Arc<ToolContext> {
        let config = ServerConfig {
            kicad_cli: "kicad-cli".into(),
            kicad_binary: "kicad".into(),
            ipc_address: String::new(),
            project_dir: None,
            jlcpcb_db_path: None,
            auto_load_toolsets: false,
            eager_toolsets: false,
        };
        Arc::new(ToolContext::new(
            config,
            Arc::new(crate::router::ToolRouter::new()),
        ))
    }

    fn fixture_schematic(dir: &std::path::Path) -> PathBuf {
        let path = dir.join("root.kicad_sch");
        let template = crate::tools::blank_schematic_template();
        konnect_sexp::writer::write_atomic(&path, &template).unwrap();

        let mut sch = cse::Schematic::load(&path).unwrap();
        for (reference, value, footprint) in [
            ("R1", "10k", "R_0603_1608Metric"),
            ("R2", "10k", "R_0603_1608Metric"),
        ] {
            let mut sym = cse::Symbol::new("Device:R", 10.0, 10.0);
            sym.set_reference(reference);
            sym.set_value_str(value);
            sym.set_footprint(footprint);
            sch.add_symbol(sym);
        }
        sch.overwrite().unwrap();
        path
    }

    /// End-to-end smoke test over real HTTP: spawn the mini-server, fetch the
    /// embedded page, list groups, edit one via POST, and confirm the write
    /// actually landed on disk — the one part of this feature the handler
    /// unit tests (in `sch_sourcing::tests`) don't exercise, since they call
    /// handlers directly rather than going through Axum/TCP.
    #[tokio::test]
    async fn serves_page_and_round_trips_a_sourcing_edit_over_http() {
        let tmp = TempDir::new().unwrap();
        let root = fixture_schematic(tmp.path());
        let ctx = test_ctx();

        let port = spawn(ctx, root.clone()).await.unwrap();
        let base = format!("http://127.0.0.1:{}", port);
        let client = reqwest::Client::new();

        // GET / serves the embedded static page.
        let page = client.get(&base).send().await.unwrap();
        assert!(page.status().is_success());
        let page_text = page.text().await.unwrap();
        assert!(page_text.contains("LCSC Sourcing"));

        // GET /api/groups lists the one (value, footprint) group with qty 2.
        let groups_resp = client
            .get(format!("{base}/api/groups"))
            .send()
            .await
            .unwrap();
        assert!(groups_resp.status().is_success());
        let groups_body: Value = groups_resp.json().await.unwrap();
        assert_eq!(groups_body["count"], 1);
        assert_eq!(groups_body["groups"][0]["quantity"], 2);

        // POST /api/groups edits LCSC/LCSC2/LCSCX without the client needing
        // to know the schematic path (the server fills it in).
        let post_resp = client
            .post(format!("{base}/api/groups"))
            .json(&json!({
                "value": "10k",
                "footprint": "R_0603_1608Metric",
                "lcsc": "C111111",
                "lcsc2": "C222222",
                "lcscx": "C333333,C444444"
            }))
            .send()
            .await
            .unwrap();
        assert!(post_resp.status().is_success());
        let post_body: Value = post_resp.json().await.unwrap();
        assert_eq!(post_body["symbols_updated"], 2);

        // The edit actually landed on disk.
        let reloaded = cse::Schematic::load(&root).unwrap();
        assert_eq!(
            reloaded
                .symbols
                .by_reference("R1")
                .unwrap()
                .property("LCSC"),
            Some("C111111")
        );
        assert_eq!(
            reloaded
                .symbols
                .by_reference("R2")
                .unwrap()
                .property("LCSC2"),
            Some("C222222")
        );

        // POST /api/refresh_all surfaces the handler's own "no local JLCPCB
        // db" error as an HTTP 400 rather than panicking the server — same
        // condition covered by
        // sch_sourcing::tests::refresh_sourcing_stock_price_errors_when_db_missing.
        let refresh_resp = client
            .post(format!("{base}/api/refresh_all"))
            .send()
            .await
            .unwrap();
        assert_eq!(refresh_resp.status(), reqwest::StatusCode::BAD_REQUEST);
    }
}
