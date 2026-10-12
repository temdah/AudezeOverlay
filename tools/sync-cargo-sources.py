#!/usr/bin/env python3
"""Generate Flatpak's vendored Cargo sources without downloading executable code."""
import argparse
import json
import re
import tomllib
from pathlib import Path


def sources(packages):
    entries = []
    for package in sorted(packages, key=lambda p: (p["name"], p["version"])):
        if "source" not in package:
            continue
        if package["source"] != "registry+https://github.com/rust-lang/crates.io-index":
            raise ValueError(f"Unsupported dependency source: {package['source']}")
        name, version, checksum = (package[k] for k in ("name", "version", "checksum"))
        if not re.fullmatch(r"[a-zA-Z0-9_-]+", name) or not re.fullmatch(r"[a-zA-Z0-9.+-]+", version):
            raise ValueError("Invalid crate name or version")
        if not re.fullmatch(r"[0-9a-f]{64}", checksum):
            raise ValueError(f"Missing or invalid checksum for {name}")
        destination = f"cargo/vendor/{name}-{version}"
        entries.extend([
            {"type": "archive", "url": f"https://static.crates.io/crates/{name}/{name}-{version}.crate",
             "sha256": checksum, "dest": destination, "archive-type": "tar-gzip"},
            {"type": "inline", "contents": json.dumps({"package": checksum, "files": {}}),
             "dest": destination, "dest-filename": ".cargo-checksum.json"},
        ])
    entries.append({
        "type": "inline",
        "contents": '[source.vendored-sources]\ndirectory = "cargo/vendor"\n\n[source.crates-io]\nreplace-with = "vendored-sources"\n',
        "dest": "cargo", "dest-filename": "config",
    })
    return entries


def canonical(entries):
    return sorted(json.dumps(entry, sort_keys=True) for entry in entries)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    packages = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))["package"]
    generated = sources(packages)
    target = root / "packaging/flatpak/cargo-sources.json"
    if args.check:
        if canonical(generated) != canonical(json.loads(target.read_text(encoding="utf-8"))):
            parser.exit(1, "Flatpak sources are stale; run tools/sync-cargo-sources.sh.\n")
    else:
        target.write_text(json.dumps(generated, indent=4) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
