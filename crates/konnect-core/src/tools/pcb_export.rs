//! `pcb_export` toolset — Gerber, PDF, SVG, 3D, BOM, netlist, position file, DRC, zone refill.
//!
//! All operations delegate to `kicad-cli` via the `cli` module, except `refill_zones`
//! which uses the KiCAD IPC API.

use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::{get_path, ToolContext, ToolDef};
use serde_json::json;
use tokio::task;

use super::cli;

// ─── IPC helpers (mirrors pcb_board / pcb_components) ───────────────────────

async fn with_ipc<T, F>(addr: String, f: F) -> anyhow::Result<Result<T, String>>
where
    T: Send + 'static,
    F: FnOnce(&konnect_ipc::client::KiCadIpcClient) -> anyhow::Result<T> + Send + 'static,
{
    let result = task::spawn_blocking(move || {
        let client = konnect_ipc::client::KiCadIpcClient::new(&addr);
        f(&client).map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| anyhow::anyhow!("spawn_blocking panicked: {e}"))?;
    Ok(result)
}

// ─── Severity filter helpers ──────────────────────────────────────────────────

fn severity_rank(s: &str) -> u8 {
    match s {
        "error" => 2,
        "warning" => 1,
        _ => 0,
    }
}

// ─── Tool definitions ─────────────────────────────────────────────────────────

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "export_gerber",
            "Export Gerber production files for all copper and mask layers using kicad-cli.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output_dir": { "type": "string", "description": "Directory to write Gerber files into" },
                    "layers": {
                        "type": "array",
                        "description": "Layer names to export (empty = all fabrication layers)",
                        "items": { "type": "string" }
                    },
                    "drill_file": { "type": "boolean", "description": "Also generate Excellon drill file", "default": true }
                },
                "required": ["board", "output_dir"]
            }),
            |args, ctx| async move { handle_export_gerber(args, ctx).await }
        ),
        tool!(
            "export_pdf",
            "Export the PCB layout to a PDF file using kicad-cli.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output": { "type": "string", "description": "Output PDF file path" },
                    "layers": {
                        "type": "array",
                        "description": "Layer names to include (empty = all visible layers)",
                        "items": { "type": "string" }
                    },
                    "black_and_white": { "type": "boolean", "description": "Render in black and white", "default": false }
                },
                "required": ["board", "output"]
            }),
            |args, ctx| async move { handle_export_pdf(args, ctx).await }
        ),
        tool!(
            "export_svg",
            "Export the PCB layout to an SVG file using kicad-cli.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output": { "type": "string", "description": "Output SVG file path" },
                    "layers": {
                        "type": "array",
                        "description": "Layer names to include (empty = all visible layers)",
                        "items": { "type": "string" }
                    },
                    "black_and_white": { "type": "boolean", "description": "Render in black and white", "default": false }
                },
                "required": ["board", "output"]
            }),
            |args, ctx| async move { handle_export_svg(args, ctx).await }
        ),
        tool!(
            "export_3d",
            "Export the PCB as a 3D model (STEP or VRML) using kicad-cli.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output": { "type": "string", "description": "Output file path (.step or .wrl)" },
                    "format": {
                        "type": "string",
                        "description": "Export format: 'step' (default) or 'vrml'",
                        "default": "step"
                    },
                    "include_unspecified": {
                        "type": "boolean",
                        "description": "Include footprints with unspecified 3D models",
                        "default": false
                    }
                },
                "required": ["board", "output"]
            }),
            |args, ctx| async move { handle_export_3d(args, ctx).await }
        ),
        tool!(
            "export_bom",
            "Generate a Bill of Materials (BOM) CSV from the schematic's component data.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file (BOM uses schematic data)" },
                    "output": { "type": "string", "description": "Output CSV file path" },
                    "format": {
                        "type": "string",
                        "description": "BOM format passed to kicad-cli: 'csv' (default)",
                        "default": "csv"
                    },
                    "exclude_dnp": {
                        "type": "boolean",
                        "description": "Exclude 'Do Not Place' components",
                        "default": true
                    }
                },
                "required": ["schematic", "output"]
            }),
            |args, ctx| async move { handle_export_bom(args, ctx).await }
        ),
        tool!(
            "export_netlist",
            "Export the PCB netlist to a file in KiCAD or IPC-D-356 format.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file (or .kicad_sch for schematic netlist)" },
                    "output": { "type": "string", "description": "Output netlist file path" },
                    "format": {
                        "type": "string",
                        "description": "Netlist format: 'kicad' or 'ipc' (IPC-D-356)",
                        "default": "kicad"
                    }
                },
                "required": ["board", "output"]
            }),
            |args, ctx| async move { handle_export_netlist(args, ctx).await }
        ),
        tool!(
            "export_position_file",
            "Generate a component placement (pick-and-place) position file for SMT assembly.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output": { "type": "string", "description": "Output position file path" },
                    "format": {
                        "type": "string",
                        "description": "File format: 'csv' (default) or 'gerber'",
                        "default": "csv"
                    },
                    "side": {
                        "type": "string",
                        "description": "Board side: 'front', 'back', or 'both'",
                        "default": "both"
                    },
                    "units": {
                        "type": "string",
                        "description": "Coordinate units: 'mm' (default) or 'in'",
                        "default": "mm"
                    }
                },
                "required": ["board", "output"]
            }),
            |args, ctx| async move { handle_export_position_file(args, ctx).await }
        ),
        tool!(
            "refill_zones",
            "Refill all copper pour zones on the board. Requires a running KiCAD instance with IPC enabled; returns an error with instructions if KiCAD is not open.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "zones": {
                        "type": "array",
                        "description": "Net names of specific zones to refill (empty = all zones, currently not filtered)",
                        "items": { "type": "string" }
                    }
                },
                "required": ["board"]
            }),
            |args, ctx| async move { handle_refill_zones(args, ctx).await }
        ),
        tool!(
            "get_drc_violations",
            "Run the Design Rule Check (DRC) on the PCB and return a list of violations. \
             Provided in `pcb_export` because the output is handy to bundle alongside \
             Gerbers when preparing a build package. For interactive / iterative DRC \
             work, prefer `run_drc` (verification toolset) — same kicad-cli check, \
             cleaner summary with error/warning counts.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to .kicad_pcb file" },
                    "output": { "type": "string", "description": "Optional path to write DRC report JSON" },
                    "severity": {
                        "type": "string",
                        "description": "Minimum severity to include: 'error', 'warning' (default), 'info'",
                        "default": "warning"
                    }
                },
                "required": ["board"]
            }),
            |args, ctx| async move { handle_get_drc_violations(args, ctx).await }
        ),
    ]
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

async fn handle_export_gerber(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let output_dir = get_path(args, "output_dir")?;
    let drill = args["drill_file"].as_bool().unwrap_or(true);

    // Ensure output dir exists
    tokio::fs::create_dir_all(&output_dir).await?;

    let cli = &ctx.config.kicad_cli;
    cli::export_gerber(cli, &board, &output_dir).await?;

    if drill {
        // kicad-cli also has a dedicated drill export
        let drill_path = output_dir.join("drill.drl");
        let _ = cli::export_drill(cli, &board, &drill_path).await; // best-effort
    }

    // List produced files
    let mut files = Vec::new();
    if let Ok(mut rd) = tokio::fs::read_dir(&output_dir).await {
        while let Ok(Some(entry)) = rd.next_entry().await {
            files.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    files.sort();

    Ok(CallToolResult::text(
        serde_json::to_string_pretty(&json!({
            "success": true,
            "output_dir": output_dir.to_str().unwrap_or(""),
            "files": files
        }))
        .unwrap(),
    ))
}

async fn handle_export_pdf(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let output = get_path(args, "output")?;

    // Collect optional layer list
    let layers: Vec<String> = args["layers"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let layer_refs: Vec<&str> = layers.iter().map(|s| s.as_str()).collect();

    let cli = &ctx.config.kicad_cli;
    cli::export_pdf(cli, &board, &output, &layer_refs).await?;

    Ok(CallToolResult::text(
        serde_json::to_string_pretty(&json!({
            "success": true,
            "output": output.to_str().unwrap_or("")
        }))
        .unwrap(),
    ))
}

async fn handle_export_svg(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let output = get_path(args, "output")?;

    let layers: Vec<String> = args["layers"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let layer_refs: Vec<&str> = layers.iter().map(|s| s.as_str()).collect();

    let cli = &ctx.config.kicad_cli;
    cli::export_svg_pcb(cli, &board, &output, &layer_refs).await?;

    Ok(CallToolResult::text(
        serde_json::to_string_pretty(&json!({
            "success": true,
            "output": output.to_str().unwrap_or("")
        }))
        .unwrap(),
    ))
}

async fn handle_export_3d(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let output = get_path(args, "output")?;
    let format = args["format"].as_str().unwrap_or("step");

    let cli = &ctx.config.kicad_cli;
    cli::export_3d(cli, &board, &output, format).await?;

    Ok(CallToolResult::text(
        serde_json::to_string_pretty(&json!({
            "success": true,
            "format": format,
            "output": output.to_str().unwrap_or("")
        }))
        .unwrap(),
    ))
}

async fn handle_export_bom(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let schematic = get_path(args, "schematic")?;
    let output = get_path(args, "output")?;
    let format = args["format"].as_str().unwrap_or("csv");

    let cli = &ctx.config.kicad_cli;
    cli::export_bom(cli, &schematic, &output, format).await?;

    Ok(CallToolResult::text(
        serde_json::to_string_pretty(&json!({
            "success": true,
            "output": output.to_str().unwrap_or("")
        }))
        .unwrap(),
    ))
}

async fn handle_export_netlist(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let output = get_path(args, "output")?;
    let format = args["format"].as_str().unwrap_or("kicad");

    let cli = &ctx.config.kicad_cli;
    // kicad-cli `sch export netlist` works on both .kicad_sch and .kicad_pcb paths.
    // For PCB-specific netlist formats (IPC-D-356), delegate same way.
    cli::export_netlist(cli, &board, &output, format).await?;

    Ok(CallToolResult::text(
        serde_json::to_string_pretty(&json!({
            "success": true,
            "format": format,
            "output": output.to_str().unwrap_or("")
        }))
        .unwrap(),
    ))
}

async fn handle_export_position_file(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let output = get_path(args, "output")?;
    let format = args["format"].as_str().unwrap_or("csv");
    let side = args["side"].as_str().unwrap_or("both");
    let units = args["units"].as_str().unwrap_or("mm");

    let cli = &ctx.config.kicad_cli;
    cli::export_position_file(cli, &board, &output, format).await?;

    Ok(CallToolResult::text(
        serde_json::to_string_pretty(&json!({
            "success": true,
            "format": format,
            "side": side,
            "units": units,
            "output": output.to_str().unwrap_or("")
        }))
        .unwrap(),
    ))
}

async fn handle_refill_zones(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let _cli = &ctx.config.kicad_cli;

    // kicad-cli pcb export gerber triggers zone fills as a side-effect,
    // but the proper command is kicad-cli pcb --refill-zones (not in all versions).
    // Use IPC refill_zones when available, otherwise fall back to file-level
    // zone fill marker update.
    let addr = ctx.config.ipc_address.clone();
    let result = with_ipc(addr, move |client| {
        client.refill_zones()?;
        Ok(())
    })
    .await;

    match result {
        Ok(Ok(())) => Ok(CallToolResult::text(
            serde_json::to_string_pretty(&json!({
                "success": true,
                "method": "ipc",
                "board": board.to_str().unwrap_or("")
            }))
            .unwrap(),
        )),
        _ => {
            // Fallback: run kicad-cli with zone-fill option if supported
            // kicad-cli pcb export gerber fills zones as a side effect
            // For now report the limitation
            Ok(CallToolResult::text(
                serde_json::to_string_pretty(&json!({
                    "success": false,
                    "note": "Zone refill requires a running KiCAD instance with IPC enabled, or manual zone fill in KiCAD GUI",
                    "board": board.to_str().unwrap_or("")
                }))
                .unwrap(),
            ))
        }
    }
}

async fn handle_get_drc_violations(
    args: &serde_json::Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let board = get_path(args, "board")?;
    let severity_filter = args["severity"].as_str().unwrap_or("warning");
    let min_rank = severity_rank(severity_filter);

    let cli = &ctx.config.kicad_cli;
    let refill = args["refill_zones"].as_bool().unwrap_or(false);
    let violations = cli::run_drc(cli, &board, refill).await?;

    // Optionally write report
    if let Some(out_path) = args["output"].as_str() {
        let report = serde_json::to_string_pretty(&violations)?;
        tokio::fs::write(out_path, report).await?;
    }

    let filtered: Vec<_> = violations
        .iter()
        .filter(|v| severity_rank(&v.severity) >= min_rank)
        .collect();

    let summary = json!({
        "total": violations.len(),
        "filtered_count": filtered.len(),
        "severity_filter": severity_filter,
        "violations": filtered.iter().map(|v| json!({
            "severity": v.severity,
            "description": v.description,
            "pos": v.pos.as_ref().map(|p| json!({ "x": p.x, "y": p.y }))
        })).collect::<Vec<_>>()
    });

    Ok(CallToolResult::text(
        serde_json::to_string_pretty(&summary).unwrap(),
    ))
}
