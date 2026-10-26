//! Stateful fake gh/glab backed by one inspectable JSON file. Python uses only
//! its standard library; each invocation locks, reloads and atomically saves
//! the file so parallel provider reads and claim renewal cannot lose writes.

use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct FakeForge {
    pub state_path: PathBuf,
    pub log_path: PathBuf,
    bin_dir: PathBuf,
}

impl FakeForge {
    /// Install both provider CLIs. Callers seed or change facts with `update`
    /// between iterations; commands and their mutations persist until drop.
    pub fn new(root: &Path, bin_dir: &Path) -> Self {
        fs::create_dir_all(root).unwrap();
        fs::create_dir_all(bin_dir).unwrap();
        for (name, source) in [
            ("main.py", include_str!("main.py")),
            ("model.py", include_str!("model.py")),
            ("api.py", include_str!("api.py")),
        ] {
            fs::write(root.join(name), source).unwrap();
        }
        let forge = Self {
            state_path: root.join("state.json"),
            log_path: root.join("commands.log"),
            bin_dir: bin_dir.to_path_buf(),
        };
        let now = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap();
        forge.save(&json!({
            "now": now, "login": "gah-bot", "repo": "owner/repo", "project": "42",
            "issues": {}, "pulls": {}, "branches": {}, "checks": {}, "pipelines": {},
            "labels": [], "next_number": 1, "next_id": 1, "calls": [], "effects": []
        }));
        fs::write(&forge.log_path, "").unwrap();
        let quote =
            |path: &Path| format!("'{}'", path.display().to_string().replace('\'', "'\\''"));
        for cli in ["gh", "glab"] {
            super::write_executable(
                &bin_dir.join(cli),
                &format!(
                    "#!/bin/sh\nexec python3 {} {} {} {cli} \"$@\"\n",
                    quote(&root.join("main.py")),
                    quote(&forge.state_path),
                    quote(&forge.log_path),
                ),
            );
        }
        forge
    }

    pub fn command(&self, cli: &str) -> Command {
        assert!(matches!(cli, "gh" | "glab"));
        Command::new(self.bin_dir.join(cli))
    }

    pub fn state(&self) -> Value {
        serde_json::from_slice(&fs::read(&self.state_path).unwrap()).unwrap()
    }

    /// Change facts only while no forge command is running. `effects` can
    /// schedule a JSON-path replacement before a specific API read to model
    /// another actor during a worker run without wall-clock races.
    pub fn update(&self, change: impl FnOnce(&mut Value)) {
        let mut state = self.state();
        change(&mut state);
        self.save(&state);
    }

    fn save(&self, state: &Value) {
        let replacement = self.state_path.with_extension("tmp");
        fs::write(&replacement, serde_json::to_vec_pretty(state).unwrap()).unwrap();
        fs::rename(replacement, &self.state_path).unwrap();
    }

    /// Seed a PR using the same creation path as the fake command. Numbers
    /// can be chosen to preserve a migrated test's existing identities.
    pub fn pull(&self, number: u64, branch: &str, sha: &str, title: &str) {
        self.update(|state| {
            state["next_number"] = json!(number);
            state["branches"][branch] = json!(sha);
        });
        let output = self
            .command("gh")
            .args([
                "pr", "create", "--head", branch, "--title", title, "--body", "", "--draft",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    pub fn calls(&self) -> Vec<Vec<String>> {
        serde_json::from_value(self.state()["calls"].clone()).unwrap()
    }
}
