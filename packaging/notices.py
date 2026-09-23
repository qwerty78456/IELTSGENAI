"""Collect actual locked dependency licenses into the distributable (no secrets)."""
import json
import pathlib
import subprocess
import sys

packages = {}
for manifest in sys.argv[2:] or ["Cargo.toml"]:
    metadata = json.loads(subprocess.check_output([
        "cargo", "metadata", "--locked", "--format-version", "1", "--manifest-path", manifest,
    ]))
    for package in metadata["packages"]:
        packages[package["id"]] = package
with open(sys.argv[1], "w", encoding="utf-8") as output:
    for package in sorted(packages.values(), key=lambda p: (p["name"], p["version"])):
        if not package["source"]:
            continue
        output.write(f"\n{'=' * 72}\n{package['name']} {package['version']}\n")
        output.write(f"License: {package.get('license')}\nSource: {package.get('repository') or package['source']}\n")
        root = pathlib.Path(package["manifest_path"]).parent
        for path in sorted(root.iterdir()):
            if path.is_file() and path.name.lower().startswith(("license", "licence", "copying", "notice")):
                output.write(f"\n--- {path.name} ---\n")
                output.write(path.read_text(encoding="utf-8", errors="replace"))
