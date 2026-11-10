//! Dictation-language dropdown for the Settings dialog.
//!
//! Rendered by `dialogs::settings` inside its **Dictation Language**
//! row. `dialogs::window` owns one [`LanguageSelect`] state through
//! [`crate::components::SettingsSelectFields`] and refreshes it from a
//! [`crate::components::SettingsSnapshot`], so no view builds a select.
//!
//! Implements `DESIGN.md` § "Settings panel": the first entry is
//! [`AUTO_LANGUAGE_LABEL`] and its value is the empty string, which is exactly
//! what an unset [`speechtt_core::settings::Settings::language`] means. The rest
//! of the list is data the worker publishes (or
//! [`crate::controller::FALLBACK_LANGUAGES`] when it publishes none), so adding
//! a language never means editing this file.

use gpui_kit::component::Sizable as _;
use gpui_kit::component::select::{SearchableVec, Select, SelectItem, SelectState};
use gpui_kit::{App, ElementId, Entity, IntoElement, RenderOnce, SharedString, Window, rems};

use crate::components::model_select::offered_value;
use crate::controller::{AUTO_LANGUAGE_LABEL, SelectOption};

/// One language row in the dictation-language menu. `Value` is the ISO-639-1
/// code; the Auto entry's value is the empty string.
#[derive(Debug, Clone)]
pub struct LanguageItem {
    pub id: SharedString,
    pub label: SharedString,
}

impl SelectItem for LanguageItem {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.id
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty()
            || self.id.to_lowercase().contains(&query)
            || self.label.to_lowercase().contains(&query)
    }
}

impl From<&SelectOption> for LanguageItem {
    fn from(option: &SelectOption) -> Self {
        Self {
            id: option.id.as_str().into(),
            label: option.label.as_str().into(),
        }
    }
}

/// Dictation-language dropdown for the Settings dialog.
///
/// Non-searchable: the list is one word per row. Defaults to the
/// [`AUTO_LANGUAGE_LABEL`] placeholder, which is what an unset language shows
/// until the user picks one, and to an enabled trigger.
#[derive(IntoElement)]
pub struct LanguageSelect {
    id: ElementId,
    state: Entity<SelectState<SearchableVec<LanguageItem>>>,
    placeholder: SharedString,
    disabled: bool,
}

impl LanguageSelect {
    pub fn new(
        id: impl Into<ElementId>,
        state: &Entity<SelectState<SearchableVec<LanguageItem>>>,
    ) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            placeholder: AUTO_LANGUAGE_LABEL.into(),
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

impl RenderOnce for LanguageSelect {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        Select::new(&self.state)
            .id(self.id)
            .small()
            .accessibility_label("Dictation language")
            .placeholder(self.placeholder)
            .disabled(self.disabled)
            .menu_width(rems(18.0))
    }
}

/// Replaces a language menu's items and selection.
///
/// A selection the menu no longer offers is cleared rather than left dangling,
/// so a catalogue refresh cannot show a value that is not in the list.
pub(crate) fn apply_languages(
    state: &Entity<SelectState<SearchableVec<LanguageItem>>>,
    options: &[SelectOption],
    selected: &Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let items: Vec<LanguageItem> = options.iter().map(LanguageItem::from).collect();
    let value = offered_value(selected, options);
    state.update(cx, |state, cx| {
        state.set_items(SearchableVec::new(items), window, cx);
        match &value {
            Some(value) => state.set_selected_value(value, window, cx),
            None => state.set_selected_index(None, window, cx),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn french() -> LanguageItem {
        LanguageItem {
            id: "fr".into(),
            label: "French".into(),
        }
    }

    #[test]
    fn a_language_row_commits_its_iso_code() {
        assert_eq!(french().value().as_ref(), "fr");
    }

    #[test]
    fn a_language_row_shows_its_english_name() {
        assert_eq!(french().title().as_ref(), "French");
    }

    #[test]
    fn a_language_row_matches_its_code_and_name_in_any_case() {
        let french = french();
        assert!(french.matches("fr"));
        assert!(french.matches("FRENCH"));
        assert!(french.matches("fren"));
        assert!(!french.matches("german"));
    }

    #[test]
    fn a_language_option_converts_to_its_row() {
        let option = SelectOption {
            id: "fr".to_string(),
            label: "French".to_string(),
            mode: None,
        };

        let item = LanguageItem::from(&option);

        assert_eq!(item.value().as_ref(), "fr");
        assert_eq!(item.title().as_ref(), "French");
    }

    #[test]
    fn the_auto_label_names_the_detected_language_option() {
        // The Auto entry itself commits the empty id, which is the value an
        // unset `Settings::language` resolves to.
        assert_eq!(AUTO_LANGUAGE_LABEL, "Auto (detect)");
    }
}
