"""Verify the real binary's captures without printing clipboard content or paths."""
import json
import os
from pathlib import Path
import sqlite3
import sys

data_dir = Path(os.environ["COPYBOOK_DATA_DIR"])
with sqlite3.connect(data_dir / "copybook.db") as db:
    rows = db.execute(
        "SELECT kind, use_count FROM items WHERE text = ?",
        (os.environ["COPYBOOK_SMOKE_TEXT"],),
    ).fetchall()
    assert len(rows) == 1, f"Expected exactly one matching row, found {len(rows)}"
    assert rows[0] == ("text", int(sys.argv[1])), rows
    assert db.execute("PRAGMA journal_mode").fetchone()[0] == "wal"

defaults = {
    "language": "system", "theme": "system", "density": "comfortable",
    "autostart": False, "hotkey": "Alt+KeyV", "winV": False,
    "panelPosition": "cursor", "hideOnBlur": True,
    "autoPaste": True, "pastePlain": False, "moveToTop": True,
    "captureText": True, "captureRich": True, "captureImages": True, "captureFiles": True,
    "historyLimit": 1000, "retentionDays": 0, "maxImageMB": 20,
    "ignoredApps": ["KeePass.exe", "KeePassXC.exe", "1Password.exe", "Bitwarden.exe"],
    "respectPrivacyHints": True, "skipSecrets": True,
}
assert json.loads((data_dir / "settings.json").read_text(encoding="utf-8")) == defaults
print(f"SQLite: exactly 1 matching text row; use_count={rows[0][1]}; WAL enabled")
print("settings.json: all 22 defaults match the contract")
