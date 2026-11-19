//! The honest example in examples/honest must stay consistent: its
//! tasks.toml parses and validates, it has 16 train and 8 held out tasks,
//! every task has a stub, a hidden check and a reference solution, and no
//! reference code sits inside the repo the model sees. Whether each check
//! passes on the reference and fails on the stub is examples/honest/selfcheck.py.

use rules_evolve_core::tasks::{Split, TasksFile};
use std::path::{Path, PathBuf};

fn honest() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/honest")
}

#[test]
fn the_honest_tasks_parse_and_every_task_has_a_stub_a_check_and_a_reference() {
    let dir = honest();
    let tasks = TasksFile::load(&dir.join("tasks.toml")).unwrap();
    let count = |s: Split| tasks.task.iter().filter(|t| t.split == s).count();
    assert_eq!(count(Split::Train), 16);
    assert_eq!(count(Split::HeldOut), 8);
    let repo = dir.join("repo");
    assert_eq!(tasks.sandbox.hidden, vec!["checks".to_string()]);
    for t in &tasks.task {
        assert_eq!(t.context, vec![format!("kit/{}.py", t.id)], "{}", t.id);
        let stub = std::fs::read_to_string(repo.join(&t.context[0])).unwrap();
        assert!(stub.contains("raise NotImplementedError"), "{}", t.id);
        assert!(
            repo.join(format!("checks/test_{}.py", t.id)).is_file(),
            "{}",
            t.id
        );
        assert!(
            dir.join("reference").join(&t.context[0]).is_file(),
            "{}",
            t.id
        );
    }
    assert!(!repo.join("reference").exists());
}
