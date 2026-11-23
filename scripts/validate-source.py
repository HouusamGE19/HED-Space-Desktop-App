from __future__ import annotations

import json
import re
import sqlite3
import subprocess
import sys
import tempfile
from html.parser import HTMLParser
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
APP = ROOT / "app"
TAURI = ROOT / "src-tauri"


def ok(msg: str) -> None:
    print(f"[OK] {msg}")


def fail(msg: str) -> None:
    raise SystemExit(f"[FAIL] {msg}")


# JSON/config parse.
for rel in ["package.json", "src-tauri/tauri.conf.json", "src-tauri/capabilities/default.json"]:
    json.loads((ROOT / rel).read_text(encoding="utf-8"))
    ok(f"parsed {rel}")

# HTML parse.
class Parser(HTMLParser):
    pass

for rel in ["index.html", "player-overlay.html"]:
    parser = Parser()
    parser.feed((APP / rel).read_text(encoding="utf-8"))
    parser.close()
    ok(f"parsed app/{rel}")

# JavaScript syntax via Node when present.
node = None
for candidate in ["node", "node.exe"]:
    try:
        subprocess.run([candidate, "--version"], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        node = candidate
        break
    except (OSError, subprocess.CalledProcessError):
        pass
if node:
    for rel in ["desktop.js", "player-overlay.js"]:
        subprocess.run([node, "--check", str(APP / rel)], check=True)
        ok(f"Node syntax app/{rel}")
    inline = re.findall(r"<script(?:\s[^>]*)?>(.*?)</script>", (APP / "index.html").read_text(encoding="utf-8"), re.S | re.I)
    for idx, source in enumerate(inline):
        with tempfile.NamedTemporaryFile("w", suffix=".js", delete=False, encoding="utf-8") as handle:
            handle.write(source)
            name = handle.name
        subprocess.run([node, "--check", name], check=True)
    ok("Node syntax for original inline prototype scripts")
else:
    print("[SKIP] Node not found; JavaScript syntax not checked")

# SQLite migration SQL: execute the CREATE TABLE batch from db.rs in memory.
db_source = (TAURI / "src" / "db.rs").read_text(encoding="utf-8")
match = re.search(r"conn\.execute_batch\(\s*r#\"(.*?)\"#", db_source, re.S)
if not match:
    fail("could not locate SQLite migration batch")
conn = sqlite3.connect(":memory:")
conn.executescript(match.group(1))
tables = {row[0] for row in conn.execute("SELECT name FROM sqlite_master WHERE type='table'")}
required = {"settings", "media", "series", "progress"}
if not required.issubset(tables):
    fail(f"missing SQLite tables: {sorted(required - tables)}")
ok("SQLite migration schema")

# Critical integration checks.
desktop = (APP / "desktop.js").read_text(encoding="utf-8")
lib = (TAURI / "src" / "lib.rs").read_text(encoding="utf-8")
for command in [
    "scan_library", "list_library_sources", "add_library_source", "update_library_source_type",
    "remove_library_source", "get_library", "update_metadata", "set_artwork",
    "open_player", "get_player_snapshot", "player_action", "select_player_track",
    "close_player", "open_atmosphere_fullscreen",
]:
    if command not in lib:
        fail(f"Rust command missing: {command}")
    if command not in desktop and command not in (APP / "player-overlay.js").read_text(encoding="utf-8"):
        fail(f"frontend never invokes command: {command}")
ok("Tauri command/frontend wiring names")

for folder in ["Movies & Series", "Anime", "Childhood", "Songs", "Tube"]:
    if folder not in (TAURI / "src" / "scanner.rs").read_text(encoding="utf-8"):
        fail(f"scanner missing HED Space folder: {folder}")
ok("scanner knows HED Space folder layout")


# Custom single-window player UI asset.
player_source = (TAURI / "src" / "player.rs").read_text(encoding="utf-8")
osc_path = TAURI / "assets" / "hedspace-osc.lua"
if not osc_path.exists():
    fail("custom mpv OSC script is missing")
osc_source = osc_path.read_text(encoding="utf-8")
for required_token in ["hedspace-osc.lua", "--script=", "--osc=no", "--force-media-title"]:
    if required_token not in player_source:
        fail(f"player integration missing: {required_token}")
for required_token in ["HED SPACE", "Subtitles", "Audio", "fullscreen_icon", "ui.fullscreen", "hedspace-close", "MBTN_LEFT"]:
    if required_token not in osc_source:
        fail(f"custom player UI missing: {required_token}")
ok("custom single-window mpv player UI wiring")

print("\nHED Space source validation passed.")
