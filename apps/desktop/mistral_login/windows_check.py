"""Native-owned WSL helper. Cookie input is private stdin; stdout is a fixed result."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile


def interrupted(_signal, _frame):
    raise TimeoutError("verification interrupted")


def check():
    root = Path.home() / ".config/gah"
    gah = Path.home() / ".cargo/bin/gah"
    if not gah.is_file() or not os.access(gah, os.X_OK):
        return "not_installed"
    mode, credential_id, label = sys.argv[1:]
    destination = root / "mistral-dashboard.cookie"
    configured = os.environ.get("MISTRAL_DASHBOARD_COOKIE_FILE")
    if not credential_id and configured is not None and Path(configured) != destination:
        return "custom_source"
    if mode == "preflight":
        return "ready"
    cookie = sys.stdin.buffer.read(32769)
    if not cookie or len(cookie) > 32768 or any(c < 32 or c >= 127 for c in cookie):
        return "invalid_cookie"
    root.mkdir(mode=0o700, parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=".mistral-check-", dir=root) as temporary:
        temporary = Path(temporary)
        cookie_file = temporary / "cookie"
        with cookie_file.open("xb") as output:
            os.chmod(cookie_file, 0o600)
            output.write(cookie)
        store = temporary / "quota.jsonl"

        def run(args, stdin=None, env=None):
            return subprocess.run([str(gah), *args], stdin=stdin, env=env,
                                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                                  timeout=95, check=False).returncode == 0

        env = dict(os.environ, MISTRAL_DASHBOARD_COOKIE_FILE=str(cookie_file))
        collected = run(["quota", "refresh", "--backend", "mistral-dashboard",
                         "--store-path", str(store)], env=env)
        try:
            with store.open("rb") as source:
                data = source.read(16385)
            record = json.loads(data) if len(data) <= 16384 else None
        except (OSError, ValueError):
            record = None
        if not collected:
            error = record.get("check_error") if isinstance(record, dict) else None
            return "auth_required" if isinstance(error, str) and error.startswith("auth_required:") else "collector"
        if (not isinstance(record, dict) or record.get("backend") != "mistral-dashboard"
                or not isinstance(record.get("account_usage"), dict)
                or record.get("check_error") is not None):
            return "invalid_observation"
        if credential_id:
            with cookie_file.open("rb") as source:
                if not run(["credentials", "save", "--id", credential_id, "--provider", "mistral",
                            "--kind", "mistral_dashboard", "--account-label", label, "--json"], stdin=source):
                    return "save"
            return "connected" if run(["quota", "refresh", "--credential", credential_id]) else "usage"
        os.replace(cookie_file, destination)
        with store.open("rb") as source:
            return "connected" if run(["quota", "record"], stdin=source) else "publish"


os.umask(0o077)
signal.signal(signal.SIGTERM, interrupted)
try:
    result = check()
except (OSError, ValueError, subprocess.SubprocessError):
    result = "unavailable"
print(json.dumps({"result": result}))
