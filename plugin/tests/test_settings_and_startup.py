"""Settings survive a dialog save, and a server that dies at startup says why."""

import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from test_kicad_detection import load_settings_dialog  # noqa: E402
from test_pid_lifecycle import _load_plugin  # noqa: E402


class MergeSettings(unittest.TestCase):
    def setUp(self):
        self.dialog = load_settings_dialog()

    def test_keys_the_dialog_does_not_show_are_kept(self):
        previous = {"transport": "stdio", "eager_toolsets": True}
        merged = self.dialog.merge_settings(previous, {"transport": "http"})
        self.assertEqual(merged, {"transport": "http", "eager_toolsets": True})

    def test_http_address_has_the_server_default(self):
        self.assertEqual(self.dialog.DEFAULT_SETTINGS["http_address"], "127.0.0.1:3000")


class StartupFailureDetail(unittest.TestCase):
    def setUp(self):
        self.plugin = _load_plugin()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self._orig_log = self.plugin._LOG_FILE
        self.plugin._LOG_FILE = os.path.join(self.tmp.name, "server.log")
        self.addCleanup(setattr, self.plugin, "_LOG_FILE", self._orig_log)

    def test_busy_port_gets_a_hint(self):
        with open(self.plugin._LOG_FILE, "w") as f:
            f.write("INFO starting\nError: Address already in use (os error 98)\n")
        detail = self.plugin._startup_failure_detail()
        self.assertIn("Address already in use", detail)
        self.assertIn("HTTP Address", detail)

    def test_missing_log_is_reported_not_raised(self):
        self.assertIn("No output was captured", self.plugin._startup_failure_detail())

    def test_start_reports_a_server_that_exits_immediately(self):
        messages = []
        wx = sys.modules["wx"]
        wx.MessageBox = lambda text, *a, **k: messages.append(text)
        wx.OK = wx.ICON_ERROR = 0
        self.plugin.BINARY_PATH = __file__  # exists; _run_server is replaced
        self.plugin._kill_tracked = lambda: None
        self.plugin._unsupported_architecture = lambda: None
        self.plugin._server_thread = None
        self.plugin._run_server = lambda: None  # exits at once
        self.plugin._STARTUP_GRACE_SECONDS = 1.0
        self.assertFalse(self.plugin.start_server())
        self.assertEqual(len(messages), 1)
        self.assertIn("exited right after starting", messages[0])


if __name__ == "__main__":
    unittest.main()
