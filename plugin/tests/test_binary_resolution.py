"""Binary lookup for the single- and all-platform PCM packages.

The all-platform package (the one KiCad's repositories list) carries
bin/<platform>/kidig-mcp[.exe]; single-platform zips keep bin/kidig-mcp[.exe].
The plugin must find either and point plugin.json's IPC action at it.
"""

import json
import os
import stat
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from test_pid_lifecycle import _load_plugin  # noqa: E402


class BinaryResolution(unittest.TestCase):
    def setUp(self):
        self.plugin = _load_plugin()
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.dir = self.tmp.name
        self.name = self.plugin.BINARY_NAME
        with open(os.path.join(self.dir, "plugin.json"), "w") as f:
            json.dump({"actions": [{"entrypoint": "bin/windows/kidig-mcp.exe"}]}, f)

    def _touch(self, *parts):
        path = os.path.join(self.dir, *parts)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "wb") as f:
            f.write(b"\0")
        os.chmod(path, 0o644)
        return path

    def _entrypoint(self):
        with open(os.path.join(self.dir, "plugin.json")) as f:
            return json.load(f)["actions"][0]["entrypoint"]

    def test_all_platform_package_uses_own_platform_dir(self):
        key = self.plugin._platform_key()
        for p in ("windows", "macos", "linux"):
            self._touch("bin", p, "kidig-mcp.exe" if p == "windows" else "kidig-mcp")
        bin_dir = self.plugin._resolve_bin_dir(self.dir)
        self.assertEqual(bin_dir, os.path.join("bin", key))
        self.plugin._prepare_binaries(self.dir, bin_dir)
        self.assertEqual(self._entrypoint(), f"bin/{key}/{self.name}")

    def test_single_platform_package_uses_bin(self):
        self._touch("bin", self.name)
        bin_dir = self.plugin._resolve_bin_dir(self.dir)
        self.assertEqual(bin_dir, "bin")
        self.plugin._prepare_binaries(self.dir, bin_dir)
        self.assertEqual(self._entrypoint(), f"bin/{self.name}")

    def test_source_checkout_is_left_alone(self):
        self.plugin._prepare_binaries(self.dir, self.plugin._resolve_bin_dir(self.dir))
        self.assertEqual(self._entrypoint(), "bin/windows/kidig-mcp.exe")

    @unittest.skipIf(sys.platform == "win32", "no executable bit on Windows")
    def test_lost_executable_bit_is_restored(self):
        key = self.plugin._platform_key()
        path = self._touch("bin", key, self.name)
        viewer = self._touch("bin", key, "schematic-viewer")
        self.plugin._prepare_binaries(self.dir, os.path.join("bin", key))
        for p in (path, viewer):
            self.assertTrue(os.stat(p).st_mode & stat.S_IXUSR, p)


if __name__ == "__main__":
    unittest.main()
