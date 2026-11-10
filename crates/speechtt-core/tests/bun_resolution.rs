//! The `bun` runtime resolution rules, proven without a filesystem.
//!
//! Every case injects its own `which` and `exists` predicates, so no test
//! creates a directory, reads a file, consults the process `PATH` or runs a
//! process. The rules under test are [`bun_candidates`]'s order and
//! [`resolve_bun_in`]'s first-hit-wins walk.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use speechtt_core::backend::{BackendError, bun_candidates, resolve_bun_in};

fn home() -> PathBuf {
    PathBuf::from("/home/ada")
}

/// Accepts only the exact paths in `accepted`; `seen` records what it was
/// asked for, so a test can prove which lookup a candidate used.
fn accepted<'a>(
    accepted: &'a [PathBuf],
    seen: &'a RefCell<Vec<PathBuf>>,
) -> impl Fn(&Path) -> bool + 'a {
    move |candidate| {
        seen.borrow_mut().push(candidate.to_path_buf());
        accepted.contains(&candidate.to_path_buf())
    }
}

#[test]
fn the_configured_path_wins_over_a_path_hit() {
    let configured = Path::new("/opt/custom/bin/bun");
    let resolved = resolve_bun_in(
        Some(configured),
        Some(&home()),
        |_| Some(PathBuf::from("/usr/bin/bun")),
        |candidate| candidate == configured,
    );

    assert_eq!(resolved, Some(PathBuf::from("/opt/custom/bin/bun")));
}

#[test]
fn an_empty_configured_value_falls_through_to_path() {
    let exists_seen = RefCell::new(Vec::new());
    let resolved = resolve_bun_in(
        None,
        Some(&home()),
        |_| Some(PathBuf::from("/usr/bin/bun")),
        accepted(&[], &exists_seen),
    );

    assert_eq!(resolved, Some(PathBuf::from("/usr/bin/bun")));
    assert!(
        exists_seen.borrow().is_empty(),
        "the bare name must be resolved through the injected which, not exists"
    );
}

#[test]
fn a_whitespace_only_configured_value_is_treated_as_empty() {
    let candidates = bun_candidates(Some(Path::new("   ")), Some(&home()));

    assert_eq!(candidates, bun_candidates(None, Some(&home())));
    assert_eq!(candidates.first(), Some(&PathBuf::from("bun")));
}

#[test]
fn path_resolves_before_the_home_install_location() {
    let exists_seen = RefCell::new(Vec::new());
    let resolved = resolve_bun_in(
        None,
        Some(&home()),
        |_| Some(PathBuf::from("/usr/bin/bun")),
        accepted(&[home().join(".bun/bin/bun")], &exists_seen),
    );

    assert_eq!(resolved, Some(PathBuf::from("/usr/bin/bun")));
    assert!(
        exists_seen.borrow().is_empty(),
        "the home location is only consulted after PATH misses"
    );
}

#[test]
fn the_home_install_location_resolves_when_path_misses() {
    let expected = home().join(".bun/bin/bun");
    let resolved = resolve_bun_in(
        None,
        Some(&home()),
        |_| None,
        |candidate| candidate == expected,
    );

    assert_eq!(resolved, Some(expected));
}

#[test]
fn usr_local_precedes_opt_when_path_and_home_miss() {
    let usr_local = PathBuf::from("/usr/local/bin/bun");
    let opt = PathBuf::from("/opt/bun/bin/bun");

    let usr_local_first = resolve_bun_in(
        None,
        Some(&home()),
        |_| None,
        |candidate| candidate == usr_local || candidate == opt,
    );
    assert_eq!(usr_local_first, Some(usr_local.clone()));

    let opt_only = resolve_bun_in(None, Some(&home()), |_| None, |candidate| candidate == opt);
    assert_eq!(opt_only, Some(opt));
}

#[test]
fn nothing_resolving_yields_none() {
    let resolved = resolve_bun_in(None, Some(&home()), |_| None, |_| false);

    assert_eq!(resolved, None);
}

#[test]
fn a_missing_home_skips_the_home_candidate_without_panicking() {
    let candidates = bun_candidates(None, None);
    assert!(
        candidates
            .iter()
            .all(|candidate| !candidate.starts_with("/home")),
        "no home candidate exists when the home directory is unknown: {candidates:?}"
    );

    let resolved = resolve_bun_in(None, None, |_| None, |_| false);
    assert_eq!(resolved, None);
}

#[test]
fn the_candidate_list_is_ordered_with_the_configured_path_first() {
    let configured = PathBuf::from("/opt/custom/bin/bun");
    let candidates = bun_candidates(Some(&configured), Some(&home()));

    assert_eq!(
        candidates,
        vec![
            configured,
            PathBuf::from("bun"),
            home().join(".bun/bin/bun"),
            PathBuf::from("/usr/local/bin/bun"),
            PathBuf::from("/opt/bun/bin/bun"),
        ]
    );
}

#[test]
fn the_candidate_list_skips_the_configured_entry_when_it_is_empty() {
    let candidates = bun_candidates(None, Some(&home()));

    assert_eq!(
        candidates,
        vec![
            PathBuf::from("bun"),
            home().join(".bun/bin/bun"),
            PathBuf::from("/usr/local/bin/bun"),
            PathBuf::from("/opt/bun/bin/bun"),
        ]
    );
}

#[test]
fn the_missing_runtime_error_names_every_searched_path() {
    let searched = bun_candidates(None, Some(&home()));
    let error = BackendError::BunMissing {
        searched: searched.clone(),
    };
    let rendered = error.to_string();

    assert!(
        rendered.contains("could not find the bun runtime"),
        "{rendered}"
    );
    for candidate in &searched {
        assert!(
            rendered.contains(&candidate.display().to_string()),
            "{rendered} must name {candidate:?}"
        );
    }
}
