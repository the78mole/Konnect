//! `sch_sourcing` toolset — JLCPCB/LCSC sourcing management: group identical
//! components (same Value + Footprint) across the whole project hierarchy,
//! bulk-edit their `LCSC`/`LCSC2`/`LCSCX` sourcing fields, refresh
//! `LCSCstock`/`LCSCprice` from the local JLCPCB parts cache, and launch a
//! small on-demand web UI for doing all of the above by hand.
//!
//! Field names deliberately reuse the existing `LCSC` property (already read
//! by the manufacturing BOM export for the "LCSC Part #" fab column) rather
//! than introducing a separate "JLC" property: `LCSC` is the preferred/
//! primary source, `LCSC2` a second source, `LCSCX` a comma-separated list of
//! further alternatives, `LCSCstock`/`LCSCprice` the last-known stock/price
//! for the `LCSC` part.

use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::integration::{default_jlcpcb_db_path, fetch_jlcpcb_part};
use crate::tools::sch_hierarchy::walk_schematic_files;
use crate::tools::{get_path, opt_str, require_str, ToolContext, ToolDef};
use konnect_schematic_editor as cse;
use konnect_schematic_editor::sexp::{atom, tagged};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "list_sourcing_groups",
            "List every component in the project, grouped by identical (Value, Footprint) \
             pair — one row per distinct component across the whole schematic hierarchy, not \
             one per instance. Each group reports its references, quantity, and current \
             LCSC/LCSC2/LCSCX sourcing fields plus the last-known LCSCstock/LCSCprice for the \
             LCSC (primary) part. Missing fields read as empty strings, not errors.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Root schematic to start from" }
                },
                "required": ["schematic"]
            }),
            |args, ctx| async move { handle_list_sourcing_groups(args, ctx).await }
        ),
        tool!(
            "set_sourcing_fields",
            "Bulk-write LCSC/LCSC2/LCSCX sourcing fields on every symbol instance across the \
             whole project hierarchy whose Value and Footprint exactly match. Fields are \
             identified by (value, footprint), not by an explicit reference list, so this is \
             self-healing across repeated calls. Omitted fields are left untouched; a \
             newly-created field is written as a hidden property (matching how LCSC is \
             normally authored). If lcsc is provided, best-effort refreshes LCSCstock/ \
             LCSCprice from the local JLCPCB database afterwards.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Root schematic to start from" },
                    "value": { "type": "string", "description": "Exact Value of the component group to update" },
                    "footprint": { "type": "string", "description": "Exact Footprint of the component group to update" },
                    "lcsc": { "type": "string", "description": "Preferred/primary LCSC part number" },
                    "lcsc2": { "type": "string", "description": "Second-source LCSC part number" },
                    "lcscx": { "type": "string", "description": "Comma-separated list of further alternative LCSC part numbers" }
                },
                "required": ["schematic", "value", "footprint"]
            }),
            |args, ctx| async move { handle_set_sourcing_fields(args, ctx).await }
        ),
        tool!(
            "refresh_sourcing_stock_price",
            "Re-sync LCSCstock/LCSCprice for every component group with a non-empty LCSC \
             field, from the local JLCPCB parts database cache. Does not touch \
             LCSC/LCSC2/LCSCX. Use before placing an order to make sure stock/price are \
             current; run download_jlcpcb_database first if the local cache is stale.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Root schematic to start from" }
                },
                "required": ["schematic"]
            }),
            |args, ctx| async move { handle_refresh_sourcing_stock_price(args, ctx).await }
        ),
        tool!(
            "open_sourcing_ui",
            "Start a small on-demand local web server with a table-based UI for linking \
             components to LCSC/JLCPCB part numbers (dropdowns for LCSC/LCSC2 sourced from \
             LCSCX, editable LCSCX list, read-only stock/price), and open it in the default \
             browser. Binds to a fresh port on 127.0.0.1 — not reachable from other machines, \
             and independent of the MCP server's own transport mode (stdio vs http).",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Root schematic to manage sourcing for" }
                },
                "required": ["schematic"]
            }),
            |args, ctx| async move { handle_open_sourcing_ui(args, ctx).await }
        ),
    ]
}

// ─── Grouping ───────────────────────────────────────────────────────────────

#[derive(Default, Clone)]
pub(crate) struct SourcingFields {
    pub(crate) lcsc: String,
    pub(crate) lcsc2: String,
    pub(crate) lcscx: String,
    pub(crate) lcsc_stock: String,
    pub(crate) lcsc_price: String,
}

impl SourcingFields {
    fn read_from(sym: &cse::Symbol) -> Self {
        SourcingFields {
            lcsc: sym.property("LCSC").unwrap_or("").to_string(),
            lcsc2: sym.property("LCSC2").unwrap_or("").to_string(),
            lcscx: sym.property("LCSCX").unwrap_or("").to_string(),
            lcsc_stock: sym.property("LCSCstock").unwrap_or("").to_string(),
            lcsc_price: sym.property("LCSCprice").unwrap_or("").to_string(),
        }
    }
}

pub(crate) struct SourcingGroup {
    pub(crate) value: String,
    pub(crate) footprint: String,
    pub(crate) references: BTreeSet<String>,
    pub(crate) fields: SourcingFields,
}

/// Walk every reachable schematic file and group placed, in-BOM symbols by
/// exact (Value, Footprint). Sourcing fields are read once, from the first
/// symbol encountered for a given group — the write path is what keeps a
/// group's members in sync afterwards. Multi-unit symbols (same Reference,
/// different `unit`) naturally collapse to `quantity == 1` because
/// `references` is a set of unique Reference strings.
pub(crate) fn collect_sourcing_groups(root: &Path) -> BTreeMap<(String, String), SourcingGroup> {
    let mut groups: BTreeMap<(String, String), SourcingGroup> = BTreeMap::new();
    for file in walk_schematic_files(root) {
        let sch = match cse::Schematic::load(&file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        for sym in sch.symbols.iter() {
            if !sym.in_bom {
                continue;
            }
            let reference = match sym.reference() {
                Some(r) if !r.starts_with('#') => r.to_string(),
                _ => continue,
            };
            let value = sym.value_str().unwrap_or("").to_string();
            let footprint = sym.footprint().unwrap_or("").to_string();
            let key = (value.clone(), footprint.clone());
            let group = groups.entry(key).or_insert_with(|| SourcingGroup {
                value: value.clone(),
                footprint: footprint.clone(),
                references: BTreeSet::new(),
                fields: SourcingFields::read_from(sym),
            });
            group.references.insert(reference);
        }
    }
    groups
}

fn group_to_json(g: &SourcingGroup) -> Value {
    json!({
        "value": g.value,
        "footprint": g.footprint,
        "quantity": g.references.len(),
        "references": g.references.iter().cloned().collect::<Vec<_>>(),
        "lcsc": g.fields.lcsc,
        "lcsc2": g.fields.lcsc2,
        "lcscx": g.fields.lcscx,
        "lcsc_stock": g.fields.lcsc_stock,
        "lcsc_price": g.fields.lcsc_price,
    })
}

// ─── Hidden-property write helper ───────────────────────────────────────────

/// Set a property's value, adding hidden `(at 0 0 0)` + `(effects ... (hide
/// yes))` sub-nodes only when the property didn't already exist on this
/// symbol — matching how a custom field like LCSC is normally authored — and
/// leaving an already-existing property's own position/visibility untouched
/// (only its value changes).
fn set_hidden_property(sym: &mut cse::Symbol, name: &str, value: &str) {
    let is_new = sym.property(name).is_none();
    sym.set_property(name, value);
    if is_new {
        if let Some(prop) = sym.properties.iter_mut().find(|p| p.name == name) {
            prop.sub_nodes = vec![
                cse::At::new(0.0, 0.0).to_sexp(),
                tagged(
                    "effects",
                    vec![
                        tagged(
                            "font",
                            vec![tagged("size", vec![atom("1.27"), atom("1.27")])],
                        ),
                        tagged("hide", vec![atom("yes")]),
                    ],
                ),
            ];
        }
    }
}

fn format_price(p: f64) -> String {
    let s = format!("{:.4}", p);
    let s = s.trim_end_matches('0');
    let s = s.trim_end_matches('.');
    if s.is_empty() {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// Look up `lcsc_id` in the local JLCPCB cache and, if found, write
/// LCSCstock/LCSCprice onto every symbol in the (value, footprint) group.
/// Returns the written (stock, price) strings, `Ok(None)` if the part isn't
/// in the local database, or `Err` if the database itself is missing/
/// unreadable.
async fn refresh_stock_price_for_group(
    root: &Path,
    value: &str,
    footprint: &str,
    db_path: &Path,
    lcsc_id: &str,
) -> anyhow::Result<Option<(String, String)>> {
    if !db_path.exists() {
        anyhow::bail!("local JLCPCB database not found at {}", db_path.display());
    }
    let part = fetch_jlcpcb_part(db_path, lcsc_id).await?;
    let Some(part) = part else {
        return Ok(None);
    };
    let stock = part["stock"].as_i64().unwrap_or(0).to_string();
    let price = format_price(part["price"].as_f64().unwrap_or(0.0));

    for file in walk_schematic_files(root) {
        let mut sch = match cse::Schematic::load(&file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut touched = false;
        for sym in sch.symbols.iter_mut() {
            if sym.value_str() != Some(value) || sym.footprint() != Some(footprint) {
                continue;
            }
            set_hidden_property(sym, "LCSCstock", &stock);
            set_hidden_property(sym, "LCSCprice", &price);
            touched = true;
        }
        if touched {
            sch.overwrite()?;
        }
    }

    Ok(Some((stock, price)))
}

// ─── Handlers ───────────────────────────────────────────────────────────────

pub(crate) async fn handle_list_sourcing_groups(
    args: &Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let root = get_path(args, "schematic")?;
    if !root.exists() {
        return Ok(CallToolResult::error(format!(
            "Schematic '{}' not found",
            root.display()
        )));
    }

    let groups = collect_sourcing_groups(&root);
    let out: Vec<Value> = groups.values().map(group_to_json).collect();
    Ok(CallToolResult::json(&json!({
        "schematic": root.display().to_string(),
        "count": out.len(),
        "groups": out
    })))
}

pub(crate) async fn handle_set_sourcing_fields(
    args: &Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let root = get_path(args, "schematic")?;
    let value = match require_str(args, "value") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let footprint = match require_str(args, "footprint") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let lcsc = opt_str(args, "lcsc").map(str::to_string);
    let lcsc2 = opt_str(args, "lcsc2").map(str::to_string);
    let lcscx = opt_str(args, "lcscx").map(str::to_string);

    if lcsc.is_none() && lcsc2.is_none() && lcscx.is_none() {
        return Ok(CallToolResult::error(
            "Provide at least one of: lcsc, lcsc2, lcscx",
        ));
    }

    let mut files_updated: Vec<String> = Vec::new();
    let mut symbols_updated: usize = 0;

    for file in walk_schematic_files(&root) {
        let mut sch = match cse::Schematic::load(&file) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut touched = false;
        for sym in sch.symbols.iter_mut() {
            if sym.value_str() != Some(value.as_str())
                || sym.footprint() != Some(footprint.as_str())
            {
                continue;
            }
            if let Some(v) = &lcsc {
                set_hidden_property(sym, "LCSC", v);
            }
            if let Some(v) = &lcsc2 {
                set_hidden_property(sym, "LCSC2", v);
            }
            if let Some(v) = &lcscx {
                set_hidden_property(sym, "LCSCX", v);
            }
            touched = true;
            symbols_updated += 1;
        }
        if touched {
            sch.overwrite()?;
            files_updated.push(file.display().to_string());
        }
    }

    if symbols_updated == 0 {
        return Ok(CallToolResult::error(format!(
            "No components found with value='{}' footprint='{}'",
            value, footprint
        )));
    }

    let mut result = json!({
        "value": value,
        "footprint": footprint,
        "files_updated": files_updated,
        "symbols_updated": symbols_updated
    });

    // Best-effort stock/price refresh — only when this call actually set a
    // new LCSC value. If lcsc wasn't provided, the group's LCSC is unchanged
    // and a full resync belongs to refresh_sourcing_stock_price instead.
    match &lcsc {
        Some(lcsc_val) if !lcsc_val.is_empty() => {
            let db_path = ctx
                .config
                .jlcpcb_db_path
                .clone()
                .unwrap_or_else(default_jlcpcb_db_path);
            match refresh_stock_price_for_group(&root, &value, &footprint, &db_path, lcsc_val).await
            {
                Ok(Some((stock, price))) => {
                    result["lcsc_stock"] = json!(stock);
                    result["lcsc_price"] = json!(price);
                }
                Ok(None) => {
                    result["stock_refresh_note"] = json!(format!(
                        "part '{}' not found in local JLCPCB database",
                        lcsc_val
                    ));
                }
                Err(e) => {
                    result["stock_refresh_note"] =
                        json!(format!("stock/price refresh skipped: {}", e));
                }
            }
        }
        _ => {
            result["stock_refresh_note"] = json!(
                "skipped: LCSC not changed in this call — use refresh_sourcing_stock_price to resync"
            );
        }
    }

    Ok(CallToolResult::json(&result))
}

pub(crate) async fn handle_refresh_sourcing_stock_price(
    args: &Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let root = get_path(args, "schematic")?;
    if !root.exists() {
        return Ok(CallToolResult::error(format!(
            "Schematic '{}' not found",
            root.display()
        )));
    }
    let db_path = ctx
        .config
        .jlcpcb_db_path
        .clone()
        .unwrap_or_else(default_jlcpcb_db_path);
    if !db_path.exists() {
        return Ok(CallToolResult::error(
            "JLCPCB database not found. Run download_jlcpcb_database first.",
        ));
    }

    let groups = collect_sourcing_groups(&root);
    let mut refreshed = Vec::new();
    let mut not_found_or_error = Vec::new();

    for group in groups.values() {
        if group.fields.lcsc.is_empty() {
            continue;
        }
        match refresh_stock_price_for_group(
            &root,
            &group.value,
            &group.footprint,
            &db_path,
            &group.fields.lcsc,
        )
        .await
        {
            Ok(Some((stock, price))) => {
                refreshed.push(json!({
                    "value": group.value,
                    "footprint": group.footprint,
                    "lcsc": group.fields.lcsc,
                    "lcsc_stock": stock,
                    "lcsc_price": price
                }));
            }
            Ok(None) | Err(_) => {
                not_found_or_error.push(json!({
                    "value": group.value,
                    "footprint": group.footprint,
                    "lcsc": group.fields.lcsc
                }));
            }
        }
    }

    Ok(CallToolResult::json(&json!({
        "refreshed": refreshed,
        "not_found_or_error": not_found_or_error
    })))
}

pub(crate) async fn handle_open_sourcing_ui(
    args: &Value,
    ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let root = get_path(args, "schematic")?;
    if !root.exists() {
        return Ok(CallToolResult::error(format!(
            "Schematic '{}' not found",
            root.display()
        )));
    }

    // Own context so the mini-server can run independently of the caller's
    // borrow; keeps the *real* observer so sourcing-UI actions still show up
    // in get_recent_calls/server_stats.
    let owned_ctx = std::sync::Arc::new(ToolContext::new_with_observer(
        ctx.config.clone(),
        ctx.router.clone(),
        ctx.observer.clone(),
    ));

    let port = crate::tools::sourcing_ui_server::spawn(owned_ctx, root.clone()).await?;
    let url = format!("http://127.0.0.1:{}/", port);
    let launched = open::that(&url).is_ok();

    Ok(CallToolResult::json(&json!({
        "launched": launched,
        "url": url,
        "port": port,
        "schematic": root.display().to_string()
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::protocol::ToolContent;
    use crate::tools::ServerConfig;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn test_ctx() -> ToolContext {
        ctx_with_db_path(None)
    }

    fn ctx_with_db_path(jlcpcb_db_path: Option<PathBuf>) -> ToolContext {
        let config = ServerConfig {
            kicad_cli: "kicad-cli".into(),
            kicad_binary: "kicad".into(),
            ipc_address: String::new(),
            project_dir: None,
            jlcpcb_db_path,
            auto_load_toolsets: false,
            eager_toolsets: false,
        };
        ToolContext::new(config, Arc::new(crate::router::ToolRouter::new()))
    }

    fn blank_schematic(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        let template = crate::tools::blank_schematic_template();
        konnect_sexp::writer::write_atomic(&path, &template).unwrap();
        path
    }

    fn add_symbol(
        sch: &mut cse::Schematic,
        reference: &str,
        value: &str,
        footprint: &str,
        unit: u32,
    ) {
        let mut sym = cse::Symbol::new("Device:R", 10.0, 10.0);
        sym.unit = unit;
        sym.set_reference(reference);
        sym.set_value_str(value);
        sym.set_footprint(footprint);
        sch.add_symbol(sym);
    }

    fn json_body(result: &CallToolResult) -> Value {
        let text = match &result.content[0] {
            ToolContent::Text { text } => text.clone(),
            _ => panic!("expected text content"),
        };
        serde_json::from_str(&text).unwrap()
    }

    #[test]
    fn groups_identical_value_and_footprint_with_correct_quantity() {
        let tmp = TempDir::new().unwrap();
        let path = blank_schematic(tmp.path(), "root.kicad_sch");
        let mut sch = cse::Schematic::load(&path).unwrap();
        add_symbol(&mut sch, "R1", "10k", "R_0603_1608Metric", 1);
        add_symbol(&mut sch, "R2", "10k", "R_0603_1608Metric", 1);
        add_symbol(&mut sch, "R3", "10k", "R_0805_2012Metric", 1);
        sch.overwrite().unwrap();

        let groups = collect_sourcing_groups(&path);
        assert_eq!(
            groups.len(),
            2,
            "two distinct (value, footprint) groups expected"
        );

        let g_0603 = groups
            .get(&("10k".to_string(), "R_0603_1608Metric".to_string()))
            .unwrap();
        assert_eq!(g_0603.references.len(), 2);
        assert!(g_0603.references.contains("R1"));
        assert!(g_0603.references.contains("R2"));

        let g_0805 = groups
            .get(&("10k".to_string(), "R_0805_2012Metric".to_string()))
            .unwrap();
        assert_eq!(g_0805.references.len(), 1);
    }

    #[test]
    fn missing_sourcing_fields_read_as_empty_strings() {
        let tmp = TempDir::new().unwrap();
        let path = blank_schematic(tmp.path(), "root.kicad_sch");
        let mut sch = cse::Schematic::load(&path).unwrap();
        add_symbol(&mut sch, "R1", "10k", "R_0603_1608Metric", 1);
        sch.overwrite().unwrap();

        let groups = collect_sourcing_groups(&path);
        let g = groups
            .get(&("10k".to_string(), "R_0603_1608Metric".to_string()))
            .unwrap();
        assert_eq!(g.fields.lcsc, "");
        assert_eq!(g.fields.lcsc2, "");
        assert_eq!(g.fields.lcscx, "");
        assert_eq!(g.fields.lcsc_stock, "");
        assert_eq!(g.fields.lcsc_price, "");
    }

    #[test]
    fn multi_unit_symbol_counts_as_one_in_quantity() {
        let tmp = TempDir::new().unwrap();
        let path = blank_schematic(tmp.path(), "root.kicad_sch");
        let mut sch = cse::Schematic::load(&path).unwrap();
        add_symbol(&mut sch, "U1", "LM324", "SOIC-14_3.9x8.7mm_P1.27mm", 1);
        add_symbol(&mut sch, "U1", "LM324", "SOIC-14_3.9x8.7mm_P1.27mm", 2);
        add_symbol(&mut sch, "U1", "LM324", "SOIC-14_3.9x8.7mm_P1.27mm", 3);
        add_symbol(&mut sch, "U1", "LM324", "SOIC-14_3.9x8.7mm_P1.27mm", 4);
        sch.overwrite().unwrap();

        let groups = collect_sourcing_groups(&path);
        let g = groups
            .get(&("LM324".to_string(), "SOIC-14_3.9x8.7mm_P1.27mm".to_string()))
            .unwrap();
        assert_eq!(
            g.references.len(),
            1,
            "a quad-unit IC's 4 unit-symbols share one Reference and must count once"
        );

        // But the write path still needs to touch every unit — set a field
        // and confirm all 4 Symbol nodes on disk carry it.
        let mut sch2 = cse::Schematic::load(&path).unwrap();
        for sym in sch2.symbols.iter_mut() {
            set_hidden_property(sym, "LCSC", "C12345");
        }
        sch2.overwrite().unwrap();
        let reloaded = cse::Schematic::load(&path).unwrap();
        assert_eq!(reloaded.symbols.len(), 4);
        for sym in reloaded.symbols.iter() {
            assert_eq!(sym.property("LCSC"), Some("C12345"));
        }
    }

    #[tokio::test]
    async fn set_sourcing_fields_requires_at_least_one_field() {
        let tmp = TempDir::new().unwrap();
        let path = blank_schematic(tmp.path(), "root.kicad_sch");
        let ctx = test_ctx();

        let args = json!({
            "schematic": path.display().to_string(),
            "value": "10k",
            "footprint": "R_0603_1608Metric"
        });
        let result = handle_set_sourcing_fields(&args, &ctx).await.unwrap();
        assert!(result.is_error);
    }

    #[tokio::test]
    async fn set_sourcing_fields_errors_when_no_component_matches() {
        let tmp = TempDir::new().unwrap();
        let path = blank_schematic(tmp.path(), "root.kicad_sch");
        let ctx = test_ctx();

        let args = json!({
            "schematic": path.display().to_string(),
            "value": "10k",
            "footprint": "R_0603_1608Metric",
            "lcsc": "C111111"
        });
        let result = handle_set_sourcing_fields(&args, &ctx).await.unwrap();
        assert!(result.is_error);
    }

    /// Regression test for the hidden-property subtlety: a brand-new field
    /// (LCSC2) must get a hidden `(at ...)`/`(effects ... (hide yes))`
    /// sub-node so it doesn't render visibly on the schematic, while an
    /// already-existing field (LCSC, here given a distinctive non-hidden
    /// sub-node up front) keeps its own sub-nodes untouched — only the value
    /// changes.
    #[tokio::test]
    async fn set_sourcing_fields_hides_new_properties_and_preserves_existing_ones() {
        let tmp = TempDir::new().unwrap();
        let path = blank_schematic(tmp.path(), "root.kicad_sch");
        let mut sch = cse::Schematic::load(&path).unwrap();

        let mut r1 = cse::Symbol::new("Device:R", 10.0, 10.0);
        r1.set_reference("R1");
        r1.set_value_str("10k");
        r1.set_footprint("R_0603_1608Metric");
        r1.properties.push(cse::Property {
            name: "LCSC".to_string(),
            value: "C999999".to_string(),
            sub_nodes: vec![tagged("at", vec![atom("5"), atom("5"), atom("0")])],
        });
        sch.add_symbol(r1);
        add_symbol(&mut sch, "R2", "10k", "R_0603_1608Metric", 1);
        sch.overwrite().unwrap();

        let ctx = test_ctx();
        let args = json!({
            "schematic": path.display().to_string(),
            "value": "10k",
            "footprint": "R_0603_1608Metric",
            "lcsc": "C111111",
            "lcsc2": "C222222",
            "lcscx": "C333333,C444444"
        });
        let result = handle_set_sourcing_fields(&args, &ctx).await.unwrap();
        assert!(!result.is_error);
        let body = json_body(&result);
        assert_eq!(body["symbols_updated"], 2);

        let reloaded = cse::Schematic::load(&path).unwrap();

        let r1 = reloaded.symbols.by_reference("R1").unwrap();
        assert_eq!(r1.property("LCSC"), Some("C111111"));
        let lcsc_prop = r1.properties.iter().find(|p| p.name == "LCSC").unwrap();
        assert!(
            lcsc_prop.sub_nodes.iter().any(|n| n.tag() == Some("at")),
            "pre-existing LCSC sub-node must be preserved"
        );
        assert!(
            !lcsc_prop
                .sub_nodes
                .iter()
                .any(|n| n.tag() == Some("effects")),
            "pre-existing LCSC must not gain a new hidden-effects node"
        );
        let lcsc2_prop = r1.properties.iter().find(|p| p.name == "LCSC2").unwrap();
        assert!(
            lcsc2_prop
                .sub_nodes
                .iter()
                .any(|n| n.tag() == Some("effects")),
            "brand-new LCSC2 must get a hidden-effects sub-node"
        );

        let r2 = reloaded.symbols.by_reference("R2").unwrap();
        assert_eq!(r2.property("LCSC"), Some("C111111"));
        let r2_lcsc = r2.properties.iter().find(|p| p.name == "LCSC").unwrap();
        assert!(
            r2_lcsc.sub_nodes.iter().any(|n| n.tag() == Some("effects")),
            "LCSC created fresh on R2 must also be hidden"
        );
    }

    #[tokio::test]
    async fn set_sourcing_fields_notes_skipped_refresh_when_lcsc_not_provided() {
        let tmp = TempDir::new().unwrap();
        let path = blank_schematic(tmp.path(), "root.kicad_sch");
        let mut sch = cse::Schematic::load(&path).unwrap();
        add_symbol(&mut sch, "R1", "10k", "R_0603_1608Metric", 1);
        sch.overwrite().unwrap();
        let ctx = test_ctx();

        let args = json!({
            "schematic": path.display().to_string(),
            "value": "10k",
            "footprint": "R_0603_1608Metric",
            "lcsc2": "C222222"
        });
        let result = handle_set_sourcing_fields(&args, &ctx).await.unwrap();
        assert!(!result.is_error);
        let body = json_body(&result);
        assert!(body["stock_refresh_note"]
            .as_str()
            .unwrap()
            .contains("skipped"));
    }

    #[tokio::test]
    async fn set_sourcing_fields_notes_but_does_not_fail_when_lcsc_db_missing() {
        let tmp = TempDir::new().unwrap();
        let path = blank_schematic(tmp.path(), "root.kicad_sch");
        let mut sch = cse::Schematic::load(&path).unwrap();
        add_symbol(&mut sch, "R1", "10k", "R_0603_1608Metric", 1);
        sch.overwrite().unwrap();

        // Point at a DB path guaranteed not to exist, regardless of whatever
        // the machine running this test actually has under ~/.konnect/.
        let ctx = ctx_with_db_path(Some(tmp.path().join("no-such.db")));

        let args = json!({
            "schematic": path.display().to_string(),
            "value": "10k",
            "footprint": "R_0603_1608Metric",
            "lcsc": "C111111"
        });
        let result = handle_set_sourcing_fields(&args, &ctx).await.unwrap();
        assert!(!result.is_error, "write itself must still succeed");
        let body = json_body(&result);
        assert!(body["stock_refresh_note"]
            .as_str()
            .unwrap()
            .contains("skipped"));

        let reloaded = cse::Schematic::load(&path).unwrap();
        assert_eq!(
            reloaded
                .symbols
                .by_reference("R1")
                .unwrap()
                .property("LCSC"),
            Some("C111111")
        );
    }

    #[tokio::test]
    async fn refresh_sourcing_stock_price_errors_when_db_missing() {
        let tmp = TempDir::new().unwrap();
        let path = blank_schematic(tmp.path(), "root.kicad_sch");
        let ctx = ctx_with_db_path(Some(tmp.path().join("no-such.db")));

        let args = json!({ "schematic": path.display().to_string() });
        let result = handle_refresh_sourcing_stock_price(&args, &ctx)
            .await
            .unwrap();
        assert!(result.is_error);
    }

    #[tokio::test]
    async fn list_sourcing_groups_handler_reports_count_and_quantity() {
        let tmp = TempDir::new().unwrap();
        let path = blank_schematic(tmp.path(), "root.kicad_sch");
        let mut sch = cse::Schematic::load(&path).unwrap();
        add_symbol(&mut sch, "R1", "10k", "R_0603_1608Metric", 1);
        add_symbol(&mut sch, "R2", "10k", "R_0603_1608Metric", 1);
        sch.overwrite().unwrap();
        let ctx = test_ctx();

        let args = json!({ "schematic": path.display().to_string() });
        let result = handle_list_sourcing_groups(&args, &ctx).await.unwrap();
        assert!(!result.is_error);
        let body = json_body(&result);
        assert_eq!(body["count"], 1);
        assert_eq!(body["groups"][0]["quantity"], 2);
    }

    #[tokio::test]
    async fn list_sourcing_groups_errors_on_missing_schematic() {
        let ctx = test_ctx();
        let args = json!({ "schematic": "/nonexistent/root.kicad_sch" });
        let result = handle_list_sourcing_groups(&args, &ctx).await.unwrap();
        assert!(result.is_error);
    }
}
