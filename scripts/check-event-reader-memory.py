#!/usr/bin/env python3
"""WSL/Linux regression: a four-event query must not retain a 128 MiB journal.

Run with an already built CLI: python3 scripts/check-event-reader-memory.py /path/to/gah
Only the fixture child gets a 192 MiB address-space limit. No providers are called.
"""

import datetime
import json
import os
from pathlib import Path
import resource
import subprocess
import sys
import tempfile


def limit_child():
    resource.setrlimit(resource.RLIMIT_CORE, (0, 0))
    resource.setrlimit(resource.RLIMIT_AS, (192 * 1024 * 1024,) * 2)


def main():
    binary = Path(sys.argv[1]).resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="parasaur-event-memory-") as directory:
        root = Path(directory)
        config = root / "config.toml"
        config.write_text(f'''[defaults]
artifact_root = "{root}"
worktree_base = "{root}/worktrees"
llm_base_url = "http://127.0.0.1:1"
llm_model_local = "fixture"
llm_model_cloud = "fixture"
[profiles.real]
display_name = "Memory fixture"
repo_id = "fixture"
provider = "github"
repo = "fixture/fixture"
local_path = "{root}"
artifact_root = "{root}/real"
default_target_branch = "main"
''')
        journal = root / "events.jsonl"
        stamp = datetime.datetime.now(datetime.timezone.utc).isoformat().replace("+00:00", "Z")
        with journal.open("w") as output:
            for index in range(2048):
                output.write(json.dumps(dict(timestamp=stamp, event_type="context_built",
                    profile="real", work_id=str(index), details="x" * 65536)) + "\n")
        result = subprocess.run([
            str(binary), "events", "--config-path", str(config), "--profile", "real",
            "--since", "1d", "--limit", "4", "--json",
        ], env=dict(os.environ, GAH_EVENTS_PATH=str(journal)), preexec_fn=limit_child,
            capture_output=True, text=True, timeout=60)
        if result.returncode:
            raise RuntimeError(f"Limited query failed ({result.returncode}): {result.stderr[:1000]}")
        assert [event["work_id"] for event in json.loads(result.stdout)] == [
            "2044", "2045", "2046", "2047"
        ]
        print("Passed: newest four events from 128 MiB journal within 192 MiB address space.")


if __name__ == "__main__":
    main()
