#!/usr/bin/python3
"""Synthetic collector fixture. Never contacts Mistral."""
import json
import os
from pathlib import Path
import stat
import sys
import time
import uuid

root = Path.home()
mode = (root / "mode").read_text()
args = sys.argv[1:]
real = (root / "real-cli").read_text() if (root / "real-cli").exists() else None
if real and args[0] == "credentials":
    os.execv(real, [real, *args])


def value(flag):
    return args[args.index(flag) + 1]


if args[:2] == ["quota", "refresh"] and "--backend" in args:
    if mode == "hang":
        time.sleep(30)
    cookie = Path(os.environ["MISTRAL_DASHBOARD_COOKIE_FILE"])
    assert stat.S_IMODE(cookie.stat().st_mode) == 0o600
    assert stat.S_IMODE(cookie.parent.stat().st_mode) == 0o700
    assert cookie.read_text() == "session=synthetic-new"
    store = Path(value("--store-path"))
    record = {"backend": "mistral-dashboard", "account_usage": {"account_scope": "synthetic"}, "check_error": None}
    if mode == "changed":
        selected = (root / "mutation").read_text()
        if selected and real:
            import subprocess
            subprocess.run([real, "credentials", "save", "--id", selected, "--provider", "mistral", "--kind", "mistral_dashboard", "--account-label", "Newer", "--json"], input=b"session=newer-work", stdout=subprocess.DEVNULL, check=True)
        elif selected:
            saved = json.loads((root / "saved.json").read_text())
            saved[selected] = "session=newer-work"
            (root / "saved.json").write_text(json.dumps(saved))
        else:
            (root / ".config/gah/mistral-dashboard.cookie").write_text("session=newer-default")
        versions = json.loads((root / "versions.json").read_text())
        versions[selected] = str(uuid.uuid4())
        (root / "versions.json").write_text(json.dumps(versions))
    if mode == "auth":
        record["check_error"] = "auth_required: synthetic refusal"
    if mode == "invalid":
        record["account_usage"] = None
    store.write_text("x" * 20000 if mode == "oversized" else json.dumps(record))
    sys.exit(1 if mode in ("auth", "collector") else 0)
elif args[:2] == ["credentials", "mistral-session-snapshot"]:
    assert sys.stdin.read() == "", "snapshot must not inherit session input"
    if mode == "unsupported":
        sys.exit(2)
    selected = value("--id") if "--id" in args else ""
    print(json.dumps({"change_token": json.loads((root / "versions.json").read_text())[selected]}))
elif args[:2] == ["credentials", "mistral-session-save"]:
    cookie = sys.stdin.read()
    assert cookie == "session=synthetic-new"
    selected = value("--id") if "--id" in args else ""
    versions = json.loads((root / "versions.json").read_text())
    if value("--change-token") != versions[selected]:
        print(json.dumps({"saved": False}))
        sys.exit(0)
    if mode == "save":
        sys.exit(1)
    if selected:
        assert value("--account-label") == 'Work "literal" & account'
        saved = json.loads((root / "saved.json").read_text())
        saved[selected] = cookie
        (root / "saved.json").write_text(json.dumps(saved))
    else:
        (root / ".config/gah/mistral-dashboard.cookie").write_text(cookie)
    versions[selected] = str(uuid.uuid4())
    (root / "versions.json").write_text(json.dumps(versions))
    print(json.dumps({"saved": True}))
elif args[:2] == ["quota", "refresh"] and "--credential" in args:
    (root / "refreshed").write_text(value("--credential"))
    sys.exit(1 if mode == "usage" else 0)
elif args[:2] == ["quota", "record"]:
    record = json.load(sys.stdin)
    assert record["account_usage"]["account_scope"] == "synthetic"
    (root / "published").write_text("yes")
    sys.exit(1 if mode == "publish" else 0)
else:
    sys.exit(2)
