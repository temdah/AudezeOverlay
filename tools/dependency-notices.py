#!/usr/bin/env python3
"""Writes the copyright and license notices of every dependency for a target.

Same output as dependency-notices.ps1, for the Linux archive:
    tools/dependency-notices.py OUTPUT [TARGET]
"""
import json
import re
import subprocess
import sys
from pathlib import Path

LICENSE_NAME = re.compile(r"^(LICEN[CS]E|COPYING|NOTICE)([._-].*)?$")


def main() -> None:
    output = Path(sys.argv[1])
    target = sys.argv[2] if len(sys.argv) > 2 else "x86_64-unknown-linux-gnu"
    metadata = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--offline", "--locked", "--format-version", "1", "--filter-platform", target],
            check=True,
            capture_output=True,
            text=True,
        ).stdout
    )
    resolved = {node["id"] for node in metadata["resolve"]["nodes"]}
    out = [
        "Audeze Overlay — dependency copyright and license notices",
        "This list also includes build-time dependencies for completeness.",
    ]
    packages = [p for p in metadata["packages"] if p["source"] and p["id"] in resolved]
    for package in sorted(packages, key=lambda p: (p["name"], p["version"])):
        directory = Path(package["manifest_path"]).parent
        files = {f for f in directory.iterdir() if f.is_file() and LICENSE_NAME.match(f.name)}
        if package.get("license_file"):
            files.add(directory / package["license_file"])
        if not files and package["license"] != "Unlicense":
            sys.exit(f"No license text found for {package['name']} {package['version']}.")
        out.append("\n" + "=" * 64)
        out.append(f"{package['name']} {package['version']} — {package['license']}")
        out.append(f"https://crates.io/crates/{package['name']}/{package['version']}")
        if not files:
            # A public-domain dedication needs no notice; the crate ships no text for it.
            out.append("\nDedicated to the public domain under the Unlicense (https://unlicense.org).")
        for file in sorted(files):
            out.append(f"\n--- {file.name} ---")
            out.append(file.read_text(encoding="utf-8", errors="replace"))
    output.write_text("\n".join(out) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
