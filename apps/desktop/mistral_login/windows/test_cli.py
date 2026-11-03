#!/usr/bin/python3
"""Synthetic collector fixture. Never contacts Mistral."""
import json
import os
from pathlib import Path
import stat
import sys
import time

root = Path.home()
mode = (root / "mode").read_text()
args = sys.argv[1:]


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
    if mode == "auth":
        record["check_error"] = "auth_required: synthetic refusal"
    if mode == "invalid":
        record["account_usage"] = None
    store.write_text("x" * 20000 if mode == "oversized" else json.dumps(record))
    sys.exit(1 if mode in ("auth", "collector") else 0)
elif args[:2] == ["credentials", "save"]:
    cookie = sys.stdin.read()
    assert cookie == "session=synthetic-new"
    assert value("--account-label") == 'Work "literal" & account'
    if mode == "save":
        sys.exit(1)
    saved = json.loads((root / "saved.json").read_text())
    saved[value("--id")] = cookie
    (root / "saved.json").write_text(json.dumps(saved))
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
