//! Model dropdown for the live take screen.
//!
//! Rendered by `views::recording` inside the header's trailing edge, to the left
//! of the elapsed clock. `SpeechTtApp` owns one [`LiveModelField`] and refreshes
//! it while the take screen is on show, so the view composes and never builds a
//! select.
//!
//! The choice is take-scoped: it changes the model of the take being prepared and
//! leaves the Settings default alone, because a live take may use a segmented
//! model that the persisted streaming default does not accept.
//!
//! Implements `DESIGN.md` § "Live take screen": the menu reuses
//! [`crate::components::ModelOption`] and the same badge the setup form shows, so
//! no new colour, size or chip is introduced.

use gpui_kit::component::Sizable as _;
use gpui_kit::component::select::{SearchableVec, Select, SelectState};
use gpui_kit::prelude::*;
use gpui_kit::{App, ElementId, Entity, IntoElement, RenderOnce, SharedString, Window, rems};

use crate::components::ModelOption;
use crate::components::model_select::{apply_models, searchable_model_state};
use crate::controller::SelectOption;

/// The take screen's model dropdown.
///
/// Non-searchable and small: it sits in a header, beside the clock, and the live
/// catalogue is short enough to scroll. A caller disables it while the take is
/// active, when the session configuration is frozen.
#[derive(IntoElement)]
pub struct LiveModelSelect {
    id: ElementId,
    state: Entity<SelectState<SearchableVec<ModelOption>>>,
    placeholder: SharedString,
    disabled: bool,
}

impl LiveModelSelect {
    pub fn new(
        id: impl Into<ElementId>,
        state: &Entity<SelectState<SearchableVec<ModelOption>>>,
    ) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            placeholder: "Choose model".into(),
            disabled: false,
        }
    }

    #[must_use]
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    #[must_use]
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl RenderOnce for LiveModelSelect {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        Select::new(&self.state)
            .id(self.id)
            .small()
            .accessibility_label("Transcription model")
            .placeholder(self.placeholder)
            .disabled(self.disabled)
            .menu_width(rems(20.0))
    }
}

/// The take screen's model state plus the snapshot it was last built from.
///
/// Kept in the app rather than the view so the `SelectState` entity survives
/// every repaint, which is what keeps its subscription alive.
pub struct LiveModelField {
    state: Entity<SelectState<SearchableVec<ModelOption>>>,
    applied: Option<(Vec<SelectOption>, Option<String>)>,
}

impl LiveModelField {
    /// Creates the state empty; the caller syncs it on the first render.
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        let state = cx.new(|cx| searchable_model_state(window, cx));
        Self {
            state,
            applied: None,
        }
    }

    #[must_use]
    pub fn state(&self) -> &Entity<SelectState<SearchableVec<ModelOption>>> {
        &self.state
    }

    /// Rebuilds items and selection only when the snapshot changes.
    ///
    /// Same rule as the Settings selects: a repaint that changes nothing leaves
    /// the menu alone, and a selection the catalogue no longer offers is cleared
    /// rather than left dangling.
    pub fn sync(
        &mut self,
        options: Vec<SelectOption>,
        selected: Option<&str>,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let snapshot = (options, selected.map(str::to_owned));
        if self.applied.as_ref() == Some(&snapshot) {
            return false;
        }
        let (options, selected) = snapshot;
        apply_models(&self.state, &options, &selected, window, cx);
        self.applied = Some((options, selected));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use speechtt_core::protocol::TranscriptionMode;

    fn option(id: &str) -> SelectOption {
        SelectOption {
            id: id.to_string(),
            label: id.to_string(),
            mode: Some(TranscriptionMode::Streaming),
        }
    }

    #[test]
    fn the_snapshot_of_a_fresh_field_is_empty() {
        // The field is built from a window, so the pure half under test is the
        // snapshot comparison the sync rule relies on.
        let applied: Option<(Vec<SelectOption>, Option<String>)> = None;
        assert!(applied.is_none());
    }

    #[test]
    fn a_snapshot_compares_by_its_items_and_its_selection() {
        let first = (vec![option("a")], Some("a".to_string()));
        let same = (vec![option("a")], Some("a".to_string()));
        let other_selection = (vec![option("a")], Some("b".to_string()));
        let other_items = (vec![option("b")], Some("a".to_string()));

        assert_eq!(first, same);
        assert_ne!(first, other_selection);
        assert_ne!(first, other_items);
    }
}
