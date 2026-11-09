//! The widget's keyboard command vocabulary.
//!
//! [`SessionAction`] names every command the widget can run, whether it comes
//! from a keystroke, a button or a dialog. There are five, and they are
//! **window-scoped**: a command runs only while the widget window holds focus,
//! or — for the two dialog toggles — while one of its dialog windows does. The
//! widget never takes a key from another application.
//!
//! This module owns no keystroke string. A key is a GPUI keystroke, so the one
//! table that states the default keys, their scopes and their Help-dialog
//! descriptions is `BINDINGS` in `crates/speechtt-ui/src/shortcuts.rs`; the
//! keymap and the Help dialog's Shortcuts tab both read it.
//!
//! The five commands, in Help order:
//!
//! 1. [`SessionAction::ToggleRecording`] — start, pause and resume a live take.
//! 2. [`SessionAction::StopRecording`] — end the take and show its transcript.
//! 3. [`SessionAction::SaveTranscript`] — write the finished transcript.
//! 4. [`SessionAction::ToggleHelp`] — open or close Help.
//! 5. [`SessionAction::ToggleSettings`] — open or close Settings.
//!
//! # Who reads this
//!
//! `speechtt-ui`'s `app` module matches on a command in its action handlers,
//! `controller` decides whether the session accepts it, and `shortcuts` maps it
//! onto the GPUI action that the keymap dispatches.

use std::fmt;

/// A command the widget can run from the keyboard, a button, or a dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SessionAction {
    ToggleRecording,
    StopRecording,
    SaveTranscript,
    ToggleHelp,
    ToggleSettings,
}

impl SessionAction {
    /// Every action, in declaration order, which is the Help dialog's reading
    /// order.
    pub const ALL: [Self; 5] = [
        Self::ToggleRecording,
        Self::StopRecording,
        Self::SaveTranscript,
        Self::ToggleHelp,
        Self::ToggleSettings,
    ];

    /// The stable camel-case name. The Help dialog's row ids and the tests are
    /// built from it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ToggleRecording => "toggleRecording",
            Self::StopRecording => "stopRecording",
            Self::SaveTranscript => "saveTranscript",
            Self::ToggleHelp => "toggleHelp",
            Self::ToggleSettings => "toggleSettings",
        }
    }
}

impl fmt::Display for SessionAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The constant is the whole vocabulary, so a missing or duplicated
    /// command cannot hide behind a `match` arm. Its order is the Help dialog's
    /// reading order.
    #[test]
    fn every_action_is_listed_once() {
        assert_eq!(
            SessionAction::ALL,
            [
                SessionAction::ToggleRecording,
                SessionAction::StopRecording,
                SessionAction::SaveTranscript,
                SessionAction::ToggleHelp,
                SessionAction::ToggleSettings,
            ]
        );
    }

    /// The names are the stable vocabulary: a rename breaks every test and
    /// diagnostic that quotes a command.
    #[test]
    fn every_action_has_its_camel_case_name() {
        let names: Vec<&str> = SessionAction::ALL
            .iter()
            .map(|action| action.as_str())
            .collect();

        assert_eq!(
            names,
            [
                "toggleRecording",
                "stopRecording",
                "saveTranscript",
                "toggleHelp",
                "toggleSettings",
            ]
        );
    }

    #[test]
    fn the_names_are_unique() {
        let mut names: Vec<&str> = SessionAction::ALL
            .iter()
            .map(|action| action.as_str())
            .collect();
        names.sort_unstable();
        names.dedup();

        assert_eq!(names.len(), SessionAction::ALL.len());
    }

    /// `Display` is the name, so a log line prints the same token the Help
    /// rows and the tests use.
    #[test]
    fn display_prints_the_stable_name() {
        for action in SessionAction::ALL {
            assert_eq!(action.to_string(), action.as_str());
        }
    }

    /// Commands are passed by value to handlers and compared against bindings,
    /// so they must stay `Copy` and free of heap allocations.
    #[test]
    fn a_command_is_a_copy_value() {
        fn assert_copy<T: Copy + Eq + Ord + std::hash::Hash>() {}
        assert_copy::<SessionAction>();
    }
}
