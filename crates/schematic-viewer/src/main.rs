#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

//! Konnect — Live Schematic Viewer
//!
//! Watches a .kicad_sch file, renders to SVG via kicad-cli, and displays
//! in a native window with pan/zoom and auto-refresh.
//!
//! Usage: schematic-viewer [--kicad-cli <path>] [path/to/file.kicad_sch]

use notify::{EventKind, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

// ─── State ──────────────────────────────────────────────────────────────────

struct ViewerState {
    schematic_path: Mutex<Option<PathBuf>>,
    kicad_cli: Mutex<String>,
    /// File passed on the command line, handed to the frontend when it asks.
    startup_file: Mutex<Option<String>>,
    /// The active file watcher. Replacing it drops (and stops) the previous
    /// one, so only the currently-open schematic is ever watched.
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
}

// ─── Binary discovery ───────────────────────────────────────────────────────

/// Resolve kicad-cli: explicit override → platform candidates → PATH.
/// Candidate lists mirror `plugin/settings_dialog.py::detect_kicad_cli`.
fn resolve_kicad_cli(override_path: Option<String>) -> String {
    if let Some(p) = override_path {
        if !p.is_empty() {
            return p;
        }
    }
    let candidates: &[&str] = if cfg!(target_os = "windows") {
        &[
            r"C:\KiCad\10.0\bin\kicad-cli.exe",
            r"C:\Program Files\KiCad\10.0\bin\kicad-cli.exe",
            r"C:\Program Files\KiCad\9.0\bin\kicad-cli.exe",
        ]
    } else if cfg!(target_os = "macos") {
        &[
            "/Applications/KiCad/KiCad.app/Contents/MacOS/kicad-cli",
            "/usr/local/bin/kicad-cli",
        ]
    } else {
        &[
            "/usr/bin/kicad-cli",
            "/usr/local/bin/kicad-cli",
            "/snap/kicad/current/usr/bin/kicad-cli",
        ]
    };
    for c in candidates {
        if Path::new(c).exists() {
            return c.to_string();
        }
    }
    "kicad-cli".to_string() // hope it's on PATH
}

/// Resolve the KiCAD GUI binary for "Open in KiCAD".
fn resolve_kicad_binary() -> String {
    let candidates: &[&str] = if cfg!(target_os = "windows") {
        &[
            r"C:\KiCad\10.0\bin\kicad.exe",
            r"C:\Program Files\KiCad\10.0\bin\kicad.exe",
            r"C:\Program Files\KiCad\9.0\bin\kicad.exe",
        ]
    } else if cfg!(target_os = "macos") {
        &["/Applications/KiCad/KiCad.app/Contents/MacOS/kicad"]
    } else {
        &["/usr/bin/kicad", "/usr/local/bin/kicad"]
    };
    for c in candidates {
        if Path::new(c).exists() {
            return c.to_string();
        }
    }
    "kicad".to_string()
}

// ─── SVG Rendering ──────────────────────────────────────────────────────────

/// Per-process temp dir so concurrent viewer instances don't clobber each
/// other's rendered SVGs.
fn render_temp_dir() -> PathBuf {
    std::env::temp_dir().join(format!("konnect-viewer-{}", std::process::id()))
}

fn render_to_svg(cli: &str, schematic: &Path) -> Result<String, String> {
    let temp_dir = render_temp_dir();
    std::fs::create_dir_all(&temp_dir).map_err(|e| e.to_string())?;

    let output = Command::new(cli)
        .args(["sch", "export", "svg", "--output"])
        .arg(&temp_dir)
        .arg(schematic)
        .output()
        .map_err(|e| format!("Failed to run kicad-cli ({}): {}", cli, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("kicad-cli failed: {}", stderr));
    }

    let stem = schematic.file_stem().unwrap_or_default().to_string_lossy();
    let svg_path = temp_dir.join(format!("{}.svg", stem));
    std::fs::read_to_string(&svg_path).map_err(|e| format!("Failed to read SVG: {}", e))
}

// ─── Tauri Commands ─────────────────────────────────────────────────────────

/// The file passed on the command line, if any. The frontend calls this once
/// its scripts are ready — no timing games with `window.eval`.
#[tauri::command]
fn get_startup_file(state: tauri::State<'_, ViewerState>) -> Option<String> {
    state.startup_file.lock().unwrap().take()
}

#[tauri::command]
fn open_schematic(
    app: AppHandle,
    state: tauri::State<'_, ViewerState>,
    path: String,
) -> Result<String, String> {
    let sch_path = PathBuf::from(&path);
    if !sch_path.exists() {
        return Err(format!("File not found: {}", path));
    }

    *state.schematic_path.lock().unwrap() = Some(sch_path.clone());

    let cli = state.kicad_cli.lock().unwrap().clone();
    let svg = render_to_svg(&cli, &sch_path)?;

    if let Some(window) = app.get_webview_window("main") {
        let name = sch_path.file_name().unwrap_or_default().to_string_lossy();
        let _ = window.set_title(&format!("{} — Schematic Viewer", name));
    }

    // Replace the watcher. Assigning drops the previous one, which stops
    // watching the old file — no stale renders overwriting the new view.
    let watcher = build_watcher(app.clone(), cli, sch_path)?;
    *state.watcher.lock().unwrap() = Some(watcher);

    Ok(svg)
}

#[tauri::command]
fn refresh(state: tauri::State<'_, ViewerState>) -> Result<String, String> {
    let path = state.schematic_path.lock().unwrap().clone();
    let cli = state.kicad_cli.lock().unwrap().clone();
    match path {
        Some(p) => render_to_svg(&cli, &p),
        None => Err("No schematic loaded".to_string()),
    }
}

#[tauri::command]
fn open_in_kicad(state: tauri::State<'_, ViewerState>) -> Result<(), String> {
    let path = state.schematic_path.lock().unwrap().clone();
    match path {
        Some(p) => {
            Command::new(resolve_kicad_binary())
                .arg(&p)
                .spawn()
                .map_err(|e| format!("Failed to launch KiCAD: {}", e))?;
            Ok(())
        }
        None => Err("No schematic loaded".to_string()),
    }
}

// ─── File Watcher ───────────────────────────────────────────────────────────

/// Build a watcher on the schematic's parent directory. The returned watcher
/// keeps its own background thread alive for as long as it is held; the
/// caller stores it in `ViewerState` so it lives exactly as long as this
/// schematic is the open one.
fn build_watcher(
    app: AppHandle,
    cli: String,
    schematic: PathBuf,
) -> Result<notify::RecommendedWatcher, String> {
    let target_name = schematic.file_name().unwrap_or_default().to_os_string();
    // Start "in the past" so the first save after opening triggers a refresh.
    let last_event = Arc::new(Mutex::new(Instant::now() - Duration::from_secs(1)));
    let watch_dir = schematic.parent().unwrap_or(Path::new(".")).to_path_buf();

    let mut watcher =
        notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
            let Ok(event) = res else { return };

            // Only file modifications / atomic-write renames
            match event.kind {
                EventKind::Modify(_) | EventKind::Create(_) => {}
                _ => return,
            }

            // Only our target file
            let is_our_file = event
                .paths
                .iter()
                .any(|p| p.file_name().map(|n| n == target_name).unwrap_or(false));
            if !is_our_file {
                return;
            }

            // Debounce (editors and atomic writes fire bursts of events)
            {
                let mut last = last_event.lock().unwrap();
                if last.elapsed() < Duration::from_millis(500) {
                    return;
                }
                *last = Instant::now();
            }

            // Stale guard: if the user opened a different schematic while
            // this event was in flight, drop it.
            {
                let state = app.state::<ViewerState>();
                let current = state.schematic_path.lock().unwrap().clone();
                if current.as_deref() != Some(schematic.as_path()) {
                    return;
                }
            }

            match render_to_svg(&cli, &schematic) {
                Ok(svg) => {
                    let _ = app.emit("schematic-updated", svg);
                }
                Err(e) => {
                    let _ = app.emit("viewer-error", format!("Render failed: {}", e));
                }
            }
        })
        .map_err(|e| format!("Failed to create file watcher: {}", e))?;

    watcher
        .watch(&watch_dir, RecursiveMode::NonRecursive)
        .map_err(|e| format!("Failed to watch {}: {}", watch_dir.display(), e))?;

    Ok(watcher)
}

// ─── Main ───────────────────────────────────────────────────────────────────

fn main() {
    // Minimal arg parsing: [--kicad-cli <path>] [schematic-file]
    let mut kicad_cli_override: Option<String> = None;
    let mut file_arg: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--kicad-cli" {
            kicad_cli_override = args.next();
        } else if !a.starts_with('-') {
            file_arg = Some(a);
        }
    }

    let state = ViewerState {
        schematic_path: Mutex::new(None),
        kicad_cli: Mutex::new(resolve_kicad_cli(kicad_cli_override)),
        startup_file: Mutex::new(file_arg),
        watcher: Mutex::new(None),
    };

    tauri::Builder::default()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_startup_file,
            open_schematic,
            refresh,
            open_in_kicad
        ])
        .build(tauri::generate_context!())
        .expect("error while building schematic viewer")
        .run(|_app, event| {
            if let tauri::RunEvent::Exit = event {
                // Best-effort cleanup of this instance's rendered SVGs
                let _ = std::fs::remove_dir_all(render_temp_dir());
            }
        });
}
