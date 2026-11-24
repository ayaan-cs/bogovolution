//! The starter in examples/starter must stay usable: its tasks.toml template
//! parses and validates, it has train and held out tasks, every path it names
//! exists in the sample repo, and mock.toml answers exactly its tasks. The
//! end to end run (real checks, mock and replay) is examples/starter/try.sh.

use graft_rules_core::model::MockConfig;
use graft_rules_core::tasks::{Split, TasksFile};
use std::path::{Path, PathBuf};

fn starter() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/starter")
}

#[test]
fn the_starter_tasks_template_parses_and_fits_the_sample_repo() {
    let dir = starter();
    let tasks = TasksFile::load(&dir.join("tasks.toml")).unwrap();
    let train = tasks
        .task
        .iter()
        .filter(|t| t.split == Split::Train)
        .count();
    let held = tasks
        .task
        .iter()
        .filter(|t| t.split == Split::HeldOut)
        .count();
    assert!(train >= 5, "{train} train tasks");
    assert!(held >= 2, "{held} held out tasks");
    let repo = dir.join("sample-repo");
    assert!(repo.join("AGENTS.md").is_file());
    for h in &tasks.sandbox.hidden {
        assert!(repo.join(h).exists(), "hidden path {h} is missing");
    }
    for t in &tasks.task {
        assert!(!t.context.is_empty(), "{} shows the whole repo", t.id);
        for c in &t.context {
            assert!(repo.join(c).is_file(), "{}: context {c} is missing", t.id);
        }
        assert!(
            t.check.contains(&format!("checks.test_{}", t.id)),
            "{}",
            t.id
        );
        assert!(
            repo.join(format!("checks/test_{}.py", t.id)).is_file(),
            "{}: no hidden test",
            t.id
        );
    }
}

#[test]
fn the_starter_mock_answers_every_task_and_ships_the_current_files() {
    let dir = starter();
    let tasks = TasksFile::load(&dir.join("tasks.toml")).unwrap();
    let mock = MockConfig::load(&dir.join("mock.toml")).unwrap();
    let mut ids: Vec<&str> = tasks.task.iter().map(|t| t.id.as_str()).collect();
    let mut answered: Vec<&str> = mock.answer.iter().map(|a| a.task.as_str()).collect();
    ids.sort();
    answered.sort();
    assert_eq!(ids, answered);
    for a in &mock.answer {
        let shipped = std::fs::read_to_string(dir.join("sample-repo").join(&a.path)).unwrap();
        assert_eq!(
            shipped.replace("\r\n", "\n"),
            a.fail_content,
            "{}: fail_content must be the file as shipped",
            a.task
        );
        assert_ne!(a.pass_content, a.fail_content, "{}", a.task);
    }
    assert!(!mock.mutation.is_empty());
}

#[test]
fn the_starter_text_has_no_em_or_en_dashes() {
    let dir = starter();
    let mut files = vec![
        dir.join("STARTER.md"),
        dir.join("tasks.toml"),
        dir.join("mock.toml"),
        dir.join("try.sh"),
        dir.join("selfcheck.py"),
    ];
    files.push(dir.join("sample-repo/README.md"));
    for f in files {
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(
            !text.contains('\u{2014}') && !text.contains('\u{2013}'),
            "{} has an em or en dash",
            f.display()
        );
    }
}
