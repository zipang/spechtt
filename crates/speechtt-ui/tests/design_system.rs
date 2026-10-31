//! Design-system gate: views consume components, they never style raw text.
//!
//! `AGENTS.md` §2 (component rule) says visual decisions live in
//! `crates/speechtt-ui/src/components/`: a view composes components and passes
//! data plus intent. This test is the mechanical half of that rule — the
//! three-role type scale is only enforced if a stray `text_size` cannot compile
//! its way back in.
//!
//! Two checks:
//! 1. `text_size`/`font_weight` may only appear in `components/text.rs` (the
//!    scale itself) and `tokens.rs` (the mirror of the stylesheet).
//! 2. A view file may not choose spacing, type or radius values: no `px(`,
//!    `rems(` or `radius::` literals. It passes `tokens::*` constants to a
//!    component instead.

use std::path::{Path, PathBuf};

fn src_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// Every `.rs` file under `src/`, sorted for a stable failure message.
fn rust_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

/// Relative path, for readable assertion messages.
fn relative(path: &Path) -> String {
    path.strip_prefix(src_root())
        .unwrap_or(path)
        .display()
        .to_string()
}

/// Files allowed to call the low-level type API.
fn type_owner(relative: &str) -> bool {
    matches!(relative, "components/text.rs" | "tokens.rs")
}

#[test]
fn only_the_text_scale_sets_font_size_or_weight() {
    let mut offenders = Vec::new();
    for file in rust_files(&src_root()) {
        let name = relative(&file);
        if type_owner(&name) {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (index, line) in source.lines().enumerate() {
            // Skip comments: prose about the rule is not a violation.
            let code = line.trim_start();
            if code.starts_with("//") || code.starts_with("///") {
                continue;
            }
            if line.contains(".text_size(") || line.contains(".font_weight(") {
                offenders.push(format!("{name}:{}: {}", index + 1, line.trim()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "text must be rendered by components::Title / Body / Label, never by a raw \
         text_size or font_weight ({}) — see AGENTS.md §2 component rule:\n{}",
        offenders.len(),
        offenders.join("\n")
    );
}

#[test]
fn views_do_not_choose_spacing_type_or_radius() {
    let views = src_root().join("views");
    let mut offenders = Vec::new();
    for file in rust_files(&views) {
        let name = relative(&file);
        let Ok(source) = std::fs::read_to_string(&file) else {
            continue;
        };
        for (index, line) in source.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") || code.starts_with("///") {
                continue;
            }
            for needle in ["px(", "rems(", "radius::"] {
                if line.contains(needle) {
                    offenders.push(format!("{name}:{}: {}", index + 1, line.trim()));
                }
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "a view must not pick a spacing, type or radius value — pass a tokens::* \
         constant to a component instead ({}) — see AGENTS.md §2 component rule:\n{}",
        offenders.len(),
        offenders.join("\n")
    );
}

/// The three roles are the whole scale: the tokens they resolve to must exist
/// and stay ordered, so the gate above cannot silently shrink to two sizes.
#[test]
fn the_type_scale_has_exactly_three_ordered_roles() {
    use speechtt_ui::components::TextRole;
    use speechtt_ui::tokens::font;

    let roles = [TextRole::Title, TextRole::Body, TextRole::Label];
    assert_eq!(roles.len(), 3);

    let sizes: Vec<f32> = roles.iter().map(|role| role.size() * 16.0).collect();
    assert_eq!(sizes, vec![font::LG, font::MD, font::SM]);
    assert_eq!(
        sizes.len(),
        3,
        "no role may be added without updating the rule"
    );
}
