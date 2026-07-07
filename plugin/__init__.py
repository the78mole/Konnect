"""
Konnect -- Action Plugin launcher.

This thin Python plugin registers a menu item in KiCAD's Tools menu.
Clicking it opens a settings dialog where users can configure paths
and start/stop the compiled Rust MCP server binary.

Installation (KiCAD PCM):
  The plugin appears in KiCAD's Plugin and Content Manager.
  After installation the binary is placed in the plugin directory.
  KiCAD loads this __init__.py automatically.
"""

import os
import sys
import subprocess
import threading

import pcbnew  # Available inside KiCAD

# Import the settings dialog (same directory)
_plugin_dir = os.path.dirname(os.path.abspath(__file__))
if _plugin_dir not in sys.path:
    sys.path.insert(0, _plugin_dir)

from settings_dialog import KonnectSettingsDialog, load_settings
import wx

PLUGIN_DIR = _plugin_dir
BINARY_NAME = "konnect.exe" if sys.platform == "win32" else "konnect"
BINARY_PATH = os.path.join(PLUGIN_DIR, "bin", BINARY_NAME)
SETTINGS_PATH = os.path.join(PLUGIN_DIR, "settings.json")

_server_process = None
_server_thread = None


def _run_server():
    """Run the MCP server subprocess. Restarts on crash (max 3 times)."""
    global _server_process
    for attempt in range(3):
        try:
            args = [BINARY_PATH]
            if os.path.exists(SETTINGS_PATH):
                args += ["--config", SETTINGS_PATH]

            _server_process = subprocess.Popen(
                args,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=None,  # inherit stderr so KiCAD scripting console shows logs
            )
            _server_process.wait()
        except Exception as e:
            print(f"[Konnect] Server error (attempt {attempt + 1}): {e}", file=sys.stderr)
        finally:
            _server_process = None


def start_server():
    """Start the MCP server in a background thread."""
    global _server_thread

    if not os.path.exists(BINARY_PATH):
        pcbnew.ShowInfoBarError(
            f"Konnect binary not found at:\n{BINARY_PATH}\n"
            "Please reinstall the plugin.",
            True,
        )
        return False

    if _server_thread and _server_thread.is_alive():
        pcbnew.ShowInfoBarMsg("Konnect is already running.", True)
        return True

    _server_thread = threading.Thread(target=_run_server, daemon=True)
    _server_thread.start()
    pcbnew.ShowInfoBarMsg("Konnect started.", True)
    return True


def stop_server():
    """Stop the MCP server subprocess."""
    global _server_process
    if _server_process:
        _server_process.terminate()
        _server_process = None
        pcbnew.ShowInfoBarMsg("Konnect stopped.", True)
    else:
        pcbnew.ShowInfoBarMsg("Konnect is not running.", True)


def is_server_running():
    """Check if the server subprocess is alive."""
    return _server_process is not None and _server_process.poll() is None


# ─── KiCAD Action Plugin entry point ─────────────────────────────────────────

class KonnectPlugin(pcbnew.ActionPlugin):
    def defaults(self):
        self.name = "Konnect"
        self.category = "AI Tools"
        self.description = (
            "Configure and control the Konnect -- enables AI assistants "
            "like Claude to design PCBs and schematics via the Model Context Protocol."
        )
        self.show_toolbar_button = True
        self.icon_file_name = os.path.join(PLUGIN_DIR, "resources", "icon.png")

    def Run(self):
        """Open the settings dialog. Server start/stop is handled via dialog buttons."""
        # Get the KiCAD main window as parent for the dialog
        parent = wx.GetTopLevelWindows()[0] if wx.GetTopLevelWindows() else None

        dlg = KonnectSettingsDialog(
            parent=parent,
            plugin_dir=PLUGIN_DIR,
            binary_path=BINARY_PATH,
            server_running=is_server_running(),
        )

        result = dlg.ShowModal()

        if result == wx.ID_YES:
            # User clicked "Start Server"
            start_server()
        elif result == wx.ID_NO:
            # User clicked "Stop Server"
            stop_server()

        dlg.Destroy()


# Register the plugin with KiCAD
KonnectPlugin().register()
