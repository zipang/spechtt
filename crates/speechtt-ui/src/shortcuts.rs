//! The window-scoped, state-scoped keyboard bindings, and the Help dialog's
//! shortcut rows.
//!
//! [`BINDINGS`] is the single source of truth for both halves of a keyboard
//! command: [`bind_keys`] registers it on the application keymap at startup
//! (`lib.rs::run`), and the Help dialog's Shortcuts tab
//! (`views::dialogs::help`) draws one row per entry. A command therefore cannot
//! be advertised without being bound, or bound without being documented.
//!
//! No keystroke is registered with the operating system: the widget never takes
//! a key from another application.
//!
//! # State scoping
//!
//! A command is available only in the state where it can act, because a
//! keystroke that matches but cannot act is a keystroke stolen from a button, a
//! toggle or a text field. Each row names the key-context predicate it is
//! registered under, and the predicate combines the window fact with the state
//! fact the widget root declares every frame
//! (`SpeechTT Take`, `SpeechTT SessionEnd`, or `SpeechTT` alone):
//!
//! - **Space** and **Enter** are bound under `SpeechTT && Take`. They act only
//!   on the live take screen, so the mode chooser, the setup form and the
//!   transcript screen keep their own Enter and Space.
//! - **Ctrl+S** is bound under `SpeechTT && SessionEnd`. Save has no meaning
//!   before a finished transcript exists, and binding it earlier would take
//!   `Ctrl+S` away from every screen with nothing to save.
//! - **F1** and **F2** are bound under `SpeechTT || SpeechTTDialog`, because a
//!   dialog lives in its own window and opening one must not strand the user
//!   with no way back. The predicate never mentions the take states, so a
//!   dialog's focused button keeps **Enter** and its API-key field keeps
//!   **Space**.
//!
//! The predicates are literals because [`KeyBinding::new`] takes a
//! `&'static str`; `every_predicate_names_the_declared_context_constants`
//! compares each predicate's terms with the context constants the window roots
//! declare, so a rename cannot silently disable a binding.
//!
//! # The stacked context form
//!
//! `SpeechTT Take` and `SpeechTT SessionEnd` are **context declarations**, not
//! predicates. The `KeyContext` type holds a *set* of identifiers separated by
//! whitespace, so one `.key_context(...)` call declares both the window fact and
//! the state fact, and a binding tests them one at a time:
//! `SpeechTT && Take`. The KeyBinding page's "Declare Key Contexts" section
//! shows the two shapes side by side (`Editor mode=normal` versus
//! `Editor && mode == normal`). Do not "fix" the declaration into
//! `SpeechTT mode=take`: the state fact is an identifier, not a key/value fact,
//! and the `Take` binding stops matching if it becomes one.
//! <https://gpui-kit.com/docs/keybinding.md#declare-key-contexts>

use gpui_kit::{App, KeyBinding};
use speechtt_core::actions::SessionAction;

/// One keyboard command: what it is called, when it applies, and its keys.
pub struct Binding {
    pub action: SessionAction,
    /// The row's description in the Help dialog.
    pub description: &'static str,
    /// The key-context predicate the command is registered under.
    ///
    /// [`KeyBinding::new`] requires a `&'static str`; the predicate names the
    /// window context and, for a take command, the state fact the widget root
    /// declares (see the module docs).
    pub context: &'static str,
    /// GPUI keystroke on Linux and Windows (see the KeyBinding page).
    pub keys: &'static str,
    /// The same command with the platform primary modifier on macOS.
    pub mac_keys: &'static str,
}

impl Binding {
    /// The keystroke of the active platform.
    fn keystroke(&self, is_mac: bool) -> &'static str {
        if is_mac { self.mac_keys } else { self.keys }
    }
}

/// Every binding, in the order the Help dialog lists them.
///
/// Recording comes first because it is the widget's whole purpose, then the
/// command that ends the take and the one that keeps its text. The two dialog
/// toggles close the table.
pub const BINDINGS: [Binding; 5] = [
    Binding {
        action: SessionAction::ToggleRecording,
        description: "Start · pause · resume recording",
        context: "SpeechTT && Take",
        keys: "space",
        mac_keys: "space",
    },
    Binding {
        action: SessionAction::StopRecording,
        description: "Stop recording and show the transcript",
        context: "SpeechTT && Take",
        keys: "enter",
        mac_keys: "enter",
    },
    Binding {
        action: SessionAction::SaveTranscript,
        description: "Save transcript to a file",
        context: "SpeechTT && SessionEnd",
        keys: "ctrl-s",
        mac_keys: "cmd-s",
    },
    Binding {
        action: SessionAction::ToggleHelp,
        description: "Open or close Help",
        context: "SpeechTT || SpeechTTDialog",
        keys: "f1",
        mac_keys: "f1",
    },
    Binding {
        action: SessionAction::ToggleSettings,
        description: "Open or close Settings",
        context: "SpeechTT || SpeechTTDialog",
        keys: "f2",
        mac_keys: "f2",
    },
];

/// Whether this build follows macOS keyboard conventions.
///
/// The pinned framework has no runtime platform accessor: neither `App` nor the
/// `Platform` trait reports the host operating system at this version, and the
/// documented `cx.platform_info()` belongs to a later release. A native build's
/// key conventions are those of the target it was compiled for, so the answer
/// comes from the build target.
#[must_use]
pub fn is_mac() -> bool {
    cfg!(target_os = "macos")
}

/// Registers [`BINDINGS`] on the application keymap.
///
/// Each row is registered under its own `context` predicate, so a command only
/// matches in the state where it can act. Must run after `gpui_kit::init(cx)`,
/// which installs the component bindings.
pub fn bind_keys(cx: &mut App) {
    let is_mac = is_mac();
    let bindings: Vec<KeyBinding> = BINDINGS
        .iter()
        .map(|binding| {
            let keys = binding.keystroke(is_mac);
            let context = binding.context;
            match binding.action {
                SessionAction::ToggleRecording => {
                    KeyBinding::new(keys, crate::app::ToggleRecording, Some(context))
                }
                SessionAction::StopRecording => {
                    KeyBinding::new(keys, crate::app::StopRecording, Some(context))
                }
                SessionAction::ToggleHelp => {
                    KeyBinding::new(keys, crate::app::ToggleHelp, Some(context))
                }
                SessionAction::ToggleSettings => {
                    KeyBinding::new(keys, crate::app::ToggleSettings, Some(context))
                }
                SessionAction::SaveTranscript => {
                    KeyBinding::new(keys, crate::app::SaveTranscript, Some(context))
                }
            }
        })
        .collect();

    cx.bind_keys(bindings);
}

/// The keycaps the Help dialog draws for `binding` (`["Ctrl", "S"]`).
///
/// The modifier order is fixed — Ctrl/Cmd, Alt, Shift, Super — whatever order
/// the keystroke string happens to use, so every row reads the same way. A
/// keystroke no cap can spell yields no caps at all; `key_caps_spell_each_key_of_the_table`
/// and `every_binding_has_display_caps` forbid that for every table row.
#[must_use]
pub fn key_caps(binding: &Binding, is_mac: bool) -> Vec<&'static str> {
    caps_of(binding.keystroke(is_mac), is_mac).unwrap_or_default()
}

/// The modifier caps of one keystroke, in display order.
#[derive(Default)]
struct ModifierCaps {
    ctrl: bool,
    cmd: bool,
    alt: bool,
    shift: bool,
    super_key: bool,
}

/// Upper-case display names for single-letter keys, indexed from `a`.
const LETTERS: [&str; 26] = [
    "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q", "R", "S",
    "T", "U", "V", "W", "X", "Y", "Z",
];

/// Display names for digit keys, indexed from `0`.
const DIGITS: [&str; 10] = ["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"];

/// Display names for `F1`..`F12`, indexed from `F1`.
const FUNCTION_KEYS: [&str; 12] = [
    "F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8", "F9", "F10", "F11", "F12",
];

/// Splits one GPUI keystroke into the caps a keycap row draws.
///
/// `secondary` is the platform primary modifier: Control on Linux and Windows,
/// Command on macOS. A token no cap can spell — an unknown modifier such as
/// `hyper`, or a second key inside one stroke — yields `None` rather than a
/// shortened row that silently drops part of the keystroke.
fn caps_of(keystroke: &'static str, is_mac: bool) -> Option<Vec<&'static str>> {
    let mut caps = ModifierCaps::default();
    let mut key: Option<&'static str> = None;

    for token in keystroke.split('-') {
        match token {
            "ctrl" | "control" => caps.ctrl = true,
            "cmd" | "command" | "win" | "windows" => caps.cmd = true,
            "secondary" => {
                if is_mac {
                    caps.cmd = true;
                } else {
                    caps.ctrl = true;
                }
            }
            "alt" | "option" | "opt" => caps.alt = true,
            "shift" => caps.shift = true,
            "super" => caps.super_key = true,
            other => {
                if key.is_some() {
                    return None;
                }
                key = Some(key_cap(other)?);
            }
        }
    }

    let mut ordered = Vec::with_capacity(5);
    if caps.ctrl {
        ordered.push("Ctrl");
    }
    if caps.cmd {
        ordered.push("Cmd");
    }
    if caps.alt {
        ordered.push("Alt");
    }
    if caps.shift {
        ordered.push("Shift");
    }
    if caps.super_key {
        ordered.push("Super");
    }
    ordered.push(key?);
    Some(ordered)
}

/// The display cap of a single key token.
fn key_cap(token: &str) -> Option<&'static str> {
    let named = match token {
        "space" => Some("Space"),
        "enter" | "return" => Some("Enter"),
        "tab" => Some("Tab"),
        "escape" | "esc" => Some("Esc"),
        "backspace" => Some("Backspace"),
        "delete" => Some("Delete"),
        "insert" => Some("Insert"),
        "home" => Some("Home"),
        "end" => Some("End"),
        "pageup" => Some("PageUp"),
        "pagedown" => Some("PageDown"),
        "up" => Some("Up"),
        "down" => Some("Down"),
        "left" => Some("Left"),
        "right" => Some("Right"),
        _ => None,
    };
    if named.is_some() {
        return named;
    }

    if let Some(rest) = token.strip_prefix('f')
        && let Ok(number) = rest.parse::<usize>()
        && (1..=FUNCTION_KEYS.len()).contains(&number)
    {
        return Some(FUNCTION_KEYS[number - 1]);
    }

    let mut characters = token.chars();
    let character = characters.next()?;
    if characters.next().is_some() {
        return None;
    }
    match character {
        'a'..='z' => Some(LETTERS[usize::from(character as u8 - b'a')]),
        'A'..='Z' => Some(LETTERS[usize::from(character.to_ascii_lowercase() as u8 - b'a')]),
        '0'..='9' => Some(DIGITS[usize::from(character as u8 - b'0')]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_covers_every_command_exactly_once() {
        let mut actions: Vec<SessionAction> =
            BINDINGS.iter().map(|binding| binding.action).collect();
        actions.sort_unstable();
        actions.dedup();

        let mut expected = SessionAction::ALL.to_vec();
        expected.sort_unstable();

        assert_eq!(actions, expected, "every command has exactly one row");
    }

    /// The rows are the Help dialog's reading order: the three commands of a
    /// take first, then the two dialogs.
    #[test]
    fn the_table_is_in_help_order() {
        let actions: Vec<SessionAction> = BINDINGS.iter().map(|binding| binding.action).collect();

        assert_eq!(actions, SessionAction::ALL);
    }

    /// The take commands act only on the live take screen; only the two toggles
    /// are bound while a dialog holds focus.
    #[test]
    fn the_take_commands_are_take_scoped_and_the_toggles_are_not() {
        let context = |action: SessionAction| {
            BINDINGS
                .iter()
                .find(|binding| binding.action == action)
                .expect("the action is in the table")
                .context
        };

        assert_eq!(context(SessionAction::ToggleRecording), "SpeechTT && Take");
        assert_eq!(context(SessionAction::StopRecording), "SpeechTT && Take");
        assert_eq!(
            context(SessionAction::SaveTranscript),
            "SpeechTT && SessionEnd"
        );
        assert_eq!(
            context(SessionAction::ToggleHelp),
            "SpeechTT || SpeechTTDialog"
        );
        assert_eq!(
            context(SessionAction::ToggleSettings),
            "SpeechTT || SpeechTTDialog"
        );
    }

    /// The identifiers a predicate combines, split on the two boolean
    /// operators and trimmed. Comparing terms (not substrings) is what keeps a
    /// near-miss such as `NotSpeechTT && Take` from satisfying a check.
    fn predicate_terms(predicate: &str) -> Vec<&str> {
        predicate
            .split("&&")
            .flat_map(|part| part.split("||"))
            .map(str::trim)
            .collect()
    }

    /// The predicates are literals because `KeyBinding::new` needs a
    /// `&'static str`, so this test is what keeps their terms spelled like the
    /// context constants the window roots declare: renaming a constant without
    /// touching the table fails here instead of silently disabling a binding.
    #[test]
    fn every_predicate_names_the_declared_context_constants() {
        let widget = crate::app::SPEECHTT_CONTEXT;
        let dialog = crate::app::SPEECHTT_DIALOG_CONTEXT;

        for binding in &BINDINGS {
            let terms = predicate_terms(binding.context);
            assert!(
                terms.contains(&widget),
                "{} registers under {:?}, whose terms {terms:?} do not include the widget context {widget:?}",
                binding.action,
                binding.context
            );
        }

        for action in [SessionAction::ToggleHelp, SessionAction::ToggleSettings] {
            let binding = BINDINGS
                .iter()
                .find(|binding| binding.action == action)
                .expect("the action is in the table");
            let terms = predicate_terms(binding.context);
            assert!(
                terms.contains(&dialog),
                "{} registers under {:?}, whose terms {terms:?} do not include the dialog context {dialog:?}",
                action,
                binding.context
            );
        }
    }

    /// The live take and the transcript screen declare a composed context, so
    /// their predicates have to name both facts: losing either one unbinds the
    /// command on that screen. `NotSpeechTT && Take` names the take fact only.
    #[test]
    fn the_state_facts_appear_in_the_take_and_save_predicates() {
        let cases = [
            (
                SessionAction::ToggleRecording,
                crate::app::SPEECHTT_TAKE_CONTEXT,
            ),
            (
                SessionAction::StopRecording,
                crate::app::SPEECHTT_TAKE_CONTEXT,
            ),
            (
                SessionAction::SaveTranscript,
                crate::app::SPEECHTT_SESSION_END_CONTEXT,
            ),
        ];

        for (action, context) in cases {
            let binding = BINDINGS
                .iter()
                .find(|binding| binding.action == action)
                .expect("the action is in the table");
            let terms = predicate_terms(binding.context);

            for fact in context.split_whitespace() {
                assert!(
                    terms.contains(&fact),
                    "{action} registers under {:?}, which does not name the {fact:?} fact of the {context:?} context",
                    binding.context
                );
            }
        }
    }

    /// A take or save command must not reach into a dialog window: Enter is the
    /// activation key of its focused button and Space is a character in its
    /// API-key field.
    #[test]
    fn the_take_and_save_predicates_never_mention_a_dialog() {
        let dialog = crate::app::SPEECHTT_DIALOG_CONTEXT;

        for action in [
            SessionAction::ToggleRecording,
            SessionAction::StopRecording,
            SessionAction::SaveTranscript,
        ] {
            let binding = BINDINGS
                .iter()
                .find(|binding| binding.action == action)
                .expect("the action is in the table");
            assert!(
                !binding.context.contains(dialog),
                "{} must not match inside a dialog window ({:?})",
                action,
                binding.context
            );
        }
    }

    /// Two commands on one keystroke would make one of them unreachable.
    #[test]
    fn no_two_rows_share_a_keystroke() {
        for is_mac in [false, true] {
            let mut seen: Vec<&str> = BINDINGS
                .iter()
                .map(|binding| binding.keystroke(is_mac))
                .collect();
            seen.sort_unstable();
            seen.dedup();

            assert_eq!(seen.len(), BINDINGS.len(), "is_mac={is_mac}");
        }
    }

    /// Every row has both descriptions: an empty one would render a blank row.
    #[test]
    fn every_row_describes_the_command() {
        for binding in &BINDINGS {
            assert!(!binding.description.trim().is_empty());
            assert!(!binding.keys.is_empty());
            assert!(!binding.mac_keys.is_empty());
        }
    }

    #[test]
    fn key_caps_spell_each_key_of_the_table() {
        let caps = |action: SessionAction, is_mac: bool| {
            let binding = BINDINGS
                .iter()
                .find(|binding| binding.action == action)
                .expect("the action is in the table");
            key_caps(binding, is_mac)
        };

        assert_eq!(caps(SessionAction::ToggleRecording, false), vec!["Space"]);
        assert_eq!(caps(SessionAction::StopRecording, false), vec!["Enter"]);
        assert_eq!(caps(SessionAction::ToggleHelp, false), vec!["F1"]);
        assert_eq!(caps(SessionAction::ToggleSettings, false), vec!["F2"]);
        assert_eq!(
            caps(SessionAction::SaveTranscript, false),
            vec!["Ctrl", "S"]
        );
        assert_eq!(caps(SessionAction::SaveTranscript, true), vec!["Cmd", "S"]);
    }

    /// The modifier order is fixed, so a keystroke written in another order
    /// still reads `Ctrl + Shift + S`.
    #[test]
    fn modifiers_keep_the_documented_order() {
        assert_eq!(
            caps_of("shift-ctrl-s", false),
            Some(vec!["Ctrl", "Shift", "S"])
        );
        assert_eq!(
            caps_of("alt-shift-cmd-x", false),
            Some(vec!["Cmd", "Alt", "Shift", "X"])
        );
    }

    /// `secondary` is the platform primary modifier: Control everywhere but
    /// macOS, where it is Command.
    #[test]
    fn secondary_follows_the_platform() {
        assert_eq!(
            caps_of("secondary-s", false),
            Some(vec!["Ctrl", "S"]),
            "Control on Linux and Windows"
        );
        assert_eq!(
            caps_of("secondary-s", true),
            Some(vec!["Cmd", "S"]),
            "Command on macOS"
        );
    }

    /// A token no cap can spell makes the whole row unrenderable rather than
    /// silently dropping part of the keystroke: a shortened row such as `["S"]`
    /// for an unhandled modifier would read as a valid `S` shortcut.
    #[test]
    fn an_unhandled_token_has_no_caps() {
        assert_eq!(caps_of("hyper-s", false), None, "unknown modifier");
        assert_eq!(caps_of("fn-s", false), None, "a GPUI modifier with no cap");
        assert_eq!(caps_of("secondary-hyper-s", false), None);
        assert_eq!(caps_of("a-s", false), None, "two keys in one stroke");
        assert_eq!(caps_of("ctrl", false), None, "no key at all");

        // The same function does answer for a keystroke it can spell, so the
        // assertions above cannot pass by refusing everything.
        assert!(caps_of("ctrl-s", false).is_some());
    }

    /// Nothing may render a partial row: every table entry must resolve to its
    /// key.
    #[test]
    fn every_binding_has_display_caps() {
        for binding in &BINDINGS {
            for is_mac in [false, true] {
                let caps = key_caps(binding, is_mac);
                assert!(!caps.is_empty(), "{} has no caps", binding.description);
                assert_eq!(
                    caps.last().copied(),
                    key_cap(binding.keystroke(is_mac).rsplit('-').next().expect("a key")),
                    "{} ends on its key",
                    binding.description
                );
            }
        }
    }

    /// `KeyBinding::new` panics on an unparsable keystroke or predicate, so the
    /// table is registered once here: every row must parse on both platforms.
    #[gpui_kit::test]
    fn every_binding_parses_on_the_application_keymap(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            bind_keys(cx);
        });
    }
}
