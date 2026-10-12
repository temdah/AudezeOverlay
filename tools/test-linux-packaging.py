#!/usr/bin/env python3
"""Offline regression checks for the Flatpak generator and user installer."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
SPEC = importlib.util.spec_from_file_location("cargo_sources", ROOT / "tools/sync-cargo-sources.py")
GENERATOR = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GENERATOR)


class PackagingTests(unittest.TestCase):
    def test_sources_keep_locked_checksums_and_reject_unknown_origins(self):
        package = {"name": "example", "version": "1.2.3", "checksum": "a" * 64,
                   "source": "registry+https://github.com/rust-lang/crates.io-index"}
        entries = GENERATOR.sources([package])
        self.assertEqual(entries[0]["sha256"], package["checksum"])
        self.assertEqual(json.loads(entries[1]["contents"])["package"], package["checksum"])
        with self.assertRaises(ValueError):
            GENERATOR.sources([dict(package, source="git+https://example.com/code")])
        with self.assertRaises(ValueError):
            GENERATOR.sources([dict(package, checksum="")])

    @unittest.skipUnless(sys.platform == "linux", "requires the Linux installer utilities")
    def test_installed_desktop_entry_quotes_special_paths(self):
        with tempfile.TemporaryDirectory(prefix="audeze-package-test-") as temporary:
            root = Path(temporary)
            prefix = root / 'My Apps & | $ ` " % \\ folder'
            binary = root / "fake-overlay"
            binary.write_text("#!/bin/sh\nexit 0\n", encoding="utf-8")
            binary.chmod(0o755)
            env = dict(os.environ, XDG_CONFIG_HOME=str(root / "config"))
            script = ROOT / "packaging/linux/install.sh"
            subprocess.run(["sh", str(script), "--prefix", str(prefix), "--binary", str(binary)],
                           check=True, env=env, capture_output=True)
            entry = prefix / "share/applications/io.github.temdah.AudezeOverlay.desktop"
            executable = str(prefix / "bin/audeze-overlay")
            escaped = "".join("\\\\\\\\" if c == "\\" else "\\\\" + c if c in '\"`$'
                              else "%%" if c == "%" else c for c in executable)
            self.assertIn('Exec="' + escaped + '"\n', entry.read_text())
            subprocess.run(["desktop-file-validate", str(entry)], check=True)
            subprocess.run(["sh", str(script), "--uninstall", "--prefix", str(prefix)],
                           check=True, env=env, capture_output=True)
            self.assertFalse(entry.exists())
            self.assertFalse((prefix / "bin/audeze-overlay").exists())


if __name__ == "__main__":
    unittest.main()
