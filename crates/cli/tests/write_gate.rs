//! Write gate (03-program-design.md, Test plan › CLI): a CLI write without
//! `--yes` only prints the plan and changes nothing on disk.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn skilldesk(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_skilldesk"))
        .args(args)
        .env("SKILLDESK_HOME", home)
        .env("SKILLDESK_DATA_DIR", home.join("data"))
        .output()
        .unwrap()
}

/// Every path under `dir` with its kind, link target or file bytes.
fn snapshot(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.file_name().is_some_and(|n| n == "data") && p.parent() == Some(dir) {
            continue; // SkillDesk's own database
        }
        let m = std::fs::symlink_metadata(&p).unwrap();
        if m.file_type().is_symlink() {
            out.push((p.clone(), format!("link {}", std::fs::read_link(&p).unwrap().display())));
        } else if m.is_dir() {
            out.push((p.clone(), "dir".into()));
            snapshot(&p, out);
        } else {
            out.push((p.clone(), format!("file {:?}", std::fs::read(&p).unwrap())));
        }
    }
}

fn tree(home: &Path) -> Vec<(PathBuf, String)> {
    let mut v = Vec::new();
    snapshot(home, &mut v);
    v
}

#[test]
fn cli_write_without_yes_changes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().canonicalize().unwrap();
    std::fs::create_dir_all(home.join(".Trash")).unwrap();
    let skill_md = |dir: &Path, name: &str| {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), format!("---\nname: {name}\n---\nbody\n")).unwrap();
    };
    let lib = home.join("elsewhere/demo");
    skill_md(&lib, "demo");
    std::fs::create_dir_all(home.join(".claude/skills")).unwrap();
    std::fs::create_dir_all(home.join(".cursor/skills")).unwrap();
    std::os::unix::fs::symlink(&lib, home.join(".claude/skills/demo")).unwrap();
    let skill = home.join("incoming/fresh");
    skill_md(&skill, "fresh");
    // A link that points nowhere gives the Fix list a card with an action.
    std::os::unix::fs::symlink(home.join("gone/old"), home.join(".claude/skills/old")).unwrap();

    assert!(skilldesk(&home, &["scan"]).status.success());
    let fixes: serde_json::Value = serde_json::from_slice(&skilldesk(&home, &["fixes", "--json"]).stdout).unwrap();
    let fix = fixes.as_array().unwrap().iter().find(|f| !f["action"].is_null()).expect("a fix with an action");
    let fix_id = fix["id"].as_str().unwrap().to_string();

    let before = tree(&home);
    for args in [
        vec!["fix", fix_id.as_str()],
        vec!["link", "demo", "cursor"],
        vec!["unlink", "demo", "claude_code"],
        vec!["install", skill.to_str().unwrap(), "--agent", "cursor"],
    ] {
        let out = skilldesk(&home, &args);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {}", String::from_utf8_lossy(&out.stderr));
        assert!(String::from_utf8_lossy(&out.stdout).contains("Nothing changed"), "{args:?}");
        assert_eq!(tree(&home), before, "{args:?} changed the disk");
    }
    let history = skilldesk(&home, &["history", "--json"]);
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&history.stdout).unwrap(), serde_json::json!([]));

    // The same command with --yes does write, and undo without --yes does not.
    let out = skilldesk(&home, &["link", "demo", "cursor", "--yes", "--json"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(std::fs::symlink_metadata(home.join(".cursor/skills/demo")).is_ok());
    let rec: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let after = tree(&home);
    let out = skilldesk(&home, &["undo", rec["record"]["id"].as_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(tree(&home), after, "undo without --yes changed the disk");
}
