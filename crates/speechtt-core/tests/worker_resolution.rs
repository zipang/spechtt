//! Hermetic specifications for the worker lookup that runs when the widget
//! spawns its Bun child.
//!
//! `BunSubprocess::spawn_default` cannot be exercised without a real process,
//! so its rules live in two pure functions that it calls: `worker_candidates`
//! lists every supported layout in precedence order, and `resolve_worker_in`
//! takes an injected `exists` predicate. Every case below supplies its own
//! predicate and uses path arithmetic only — no directory is created and no
//! file is read.
//!
//! Run by `cargo test` (AGENTS.md §5).

use std::path::{Path, PathBuf};

use speechtt_core::backend::{BackendError, WORKER_SCRIPT, resolve_worker_in, worker_candidates};

/// A representative Linux AppDir executable directory.
fn appdir_bin() -> PathBuf {
    PathBuf::from("/opt/SpeechTT.AppDir/usr/bin")
}

#[test]
fn the_appdir_layout_is_the_first_candidate() {
    let exe_dir = appdir_bin();
    let worker = exe_dir.join("../share/speechtt/worker.js");

    let resolved = resolve_worker_in(&exe_dir, |candidate| candidate == worker);

    assert_eq!(resolved, Some(worker));
}

#[test]
fn the_macos_resources_layout_is_second() {
    let exe_dir = Path::new("/Applications/SpeechTT.app/Contents/MacOS");
    let worker = exe_dir.join("../Resources/worker.js");

    let resolved = resolve_worker_in(exe_dir, |candidate| candidate == worker);

    assert_eq!(resolved, Some(worker));
}

#[test]
fn a_worker_beside_the_executable_is_third() {
    let exe_dir = Path::new("/opt/speechtt");
    let worker = exe_dir.join("worker.js");

    let resolved = resolve_worker_in(exe_dir, |candidate| candidate == worker);

    assert_eq!(resolved, Some(worker));
}

#[test]
fn the_cargo_build_tree_resolves_to_the_repository_worker() {
    let exe_dir = Path::new("/home/user/speechtt/target/release");
    let worker = exe_dir.join("../../src/stt/worker.ts");

    let resolved = resolve_worker_in(exe_dir, |candidate| candidate == worker);

    assert_eq!(resolved, Some(worker));
}

#[test]
fn the_cargo_deps_directory_resolves_to_the_repository_worker() {
    let exe_dir = Path::new("/home/user/speechtt/target/release/deps");
    let worker = exe_dir.join("../../../src/stt/worker.ts");

    let resolved = resolve_worker_in(exe_dir, |candidate| candidate == worker);

    assert_eq!(resolved, Some(worker));
}

#[test]
fn no_candidate_existing_is_none() {
    let resolved = resolve_worker_in(&appdir_bin(), |_| false);

    assert_eq!(resolved, None);
}

#[test]
fn the_first_existing_candidate_wins() {
    let exe_dir = appdir_bin();
    let worker = exe_dir.join("../share/speechtt/worker.js");

    // Both the AppDir worker and the beside-the-executable worker exist; the
    // resolver must choose the more specific layout.
    let resolved = resolve_worker_in(&exe_dir, |candidate| {
        candidate == worker || candidate == exe_dir.join("worker.js")
    });

    assert_eq!(resolved, Some(worker));
}

#[test]
fn the_candidate_list_is_ordered_and_joined_onto_the_executable_directory() {
    let exe_dir = appdir_bin();

    assert_eq!(
        worker_candidates(&exe_dir),
        vec![
            exe_dir.join("../share/speechtt/worker.js"),
            exe_dir.join("../Resources/worker.js"),
            exe_dir.join("worker.js"),
            exe_dir.join("../../src/stt/worker.ts"),
            exe_dir.join("../../../src/stt/worker.ts"),
        ]
    );
}

#[test]
fn a_root_like_executable_directory_does_not_panic() {
    let candidates = worker_candidates(Path::new("/"));

    assert_eq!(candidates.len(), 5);
    assert!(candidates.iter().all(|candidate| candidate.is_absolute()));
}

#[test]
fn a_missing_worker_reports_the_searched_paths() {
    let searched = worker_candidates(&appdir_bin());
    let message = BackendError::WorkerMissing { searched }.to_string();

    assert!(
        message.contains("could not find the STT worker"),
        "{message}"
    );
    assert!(message.contains("worker.js"), "{message}");
    assert!(!message.contains("api_key"), "{message}");
}

#[test]
fn the_repository_worker_constant_still_names_the_cargo_fallback() {
    assert_eq!(WORKER_SCRIPT, "src/stt/worker.ts");
}
