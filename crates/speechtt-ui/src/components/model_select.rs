//! Provider and model dropdowns shared by the Session setup screen and the
//! Settings dialog.
//!
//! Rendered by `views::session_setup` and `views::dialogs::settings`, which
//! compose [`ProviderSelect`] and [`ModelSelect`] into their form rows.
//! `SpeechTtApp` owns one [`SelectFields`] pair for the setup form and refreshes
//! it from a [`SelectSnapshot`]; `DialogWindow` owns the
//! [`SettingsSelectFields`] quartet — provider, the two take defaults, the
//! cleanup model — plus the dictation-language menu from
//! [`crate::components::LanguageSelect`], and refreshes them from a
//! [`SettingsSnapshot`].
//!
//! Implements `DESIGN.md` § "Session setup form" and § "Settings panel": the
//! rows reuse the existing [`TranscriptionModeBadge`] and no new chip, colour
//! or type size is introduced.

use gpui_kit::base::h_flex;
use gpui_kit::component::Sizable as _;
use gpui_kit::component::select::{SearchableVec, Select, SelectItem, SelectState};
use gpui_kit::prelude::*;
use gpui_kit::{App, ElementId, Entity, IntoElement, RenderOnce, SharedString, Window, rems};

use speechtt_core::protocol::TranscriptionMode;

use crate::components::language_select::apply_languages;
use crate::components::{Label, LanguageItem, TranscriptionModeBadge};
use crate::controller::SelectOption;

/// Case-insensitive substring match over an id or a label.
fn matches_query(query: &str, id: &str, label: &str) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty() || id.to_lowercase().contains(&query) || label.to_lowercase().contains(&query)
}

/// One provider row in the provider menu. `Value` is the provider id.
#[derive(Debug, Clone)]
pub struct ProviderOption {
    pub id: SharedString,
    pub label: SharedString,
}

impl SelectItem for ProviderOption {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.id
    }

    fn matches(&self, query: &str) -> bool {
        matches_query(query, &self.id, &self.label)
    }
}

impl From<&SelectOption> for ProviderOption {
    fn from(option: &SelectOption) -> Self {
        Self {
            id: option.id.as_str().into(),
            label: option.label.as_str().into(),
        }
    }
}

/// One model row in the model menu. `Value` is the model id and `mode` draws
/// the row's [`TranscriptionModeBadge`].
///
/// The trigger renders `title()` only: the mode of the committed model is
/// already shown by the badge the setup form and the Settings dialog keep beside
/// the field, so repeating it inside the trigger would show the same chip twice.
#[derive(Debug, Clone)]
pub struct ModelOption {
    pub id: SharedString,
    pub label: SharedString,
    pub mode: Option<TranscriptionMode>,
}

impl ModelOption {
    /// The label, followed by the mode badge when the catalogue reports one.
    fn row(&self) -> impl IntoElement {
        let mut row = h_flex()
            .items_center()
            .gap_2()
            .child(Label::new(self.label.clone()));
        if let Some(mode) = self.mode {
            row = row.child(TranscriptionModeBadge::new(mode));
        }
        row
    }
}

impl SelectItem for ModelOption {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        self.label.clone()
    }

    fn value(&self) -> &Self::Value {
        &self.id
    }

    fn matches(&self, query: &str) -> bool {
        matches_query(query, &self.id, &self.label)
    }

    fn render(&self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        self.row()
    }
}

impl From<&SelectOption> for ModelOption {
    fn from(option: &SelectOption) -> Self {
        Self {
            id: option.id.as_str().into(),
            label: option.label.as_str().into(),
            mode: option.mode,
        }
    }
}

/// Provider dropdown for the Session setup form.
///
/// Non-searchable: the provider list is short and every row is a single label.
/// Defaults to the "Choose provider" placeholder and an enabled trigger.
#[derive(IntoElement)]
pub struct ProviderSelect {
    id: ElementId,
    state: Entity<SelectState<SearchableVec<ProviderOption>>>,
    placeholder: SharedString,
    disabled: bool,
}

impl ProviderSelect {
    pub fn new(
        id: impl Into<ElementId>,
        state: &Entity<SelectState<SearchableVec<ProviderOption>>>,
    ) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            placeholder: "Choose provider".into(),
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

impl RenderOnce for ProviderSelect {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        Select::new(&self.state)
            .id(self.id)
            .small()
            .accessibility_label("Transcription service")
            .placeholder(self.placeholder)
            .disabled(self.disabled)
            .menu_width(rems(18.0))
    }
}

/// Model dropdown for the Session setup form.
///
/// Searchable, because a provider can offer many gateway models. Defaults to
/// the "Choose model" placeholder and an enabled trigger; claimants disable it
/// until a provider is selected.
#[derive(IntoElement)]
pub struct ModelSelect {
    id: ElementId,
    state: Entity<SelectState<SearchableVec<ModelOption>>>,
    placeholder: SharedString,
    disabled: bool,
}

impl ModelSelect {
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

impl RenderOnce for ModelSelect {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        Select::new(&self.state)
            .id(self.id)
            .small()
            .accessibility_label("Model")
            .placeholder(self.placeholder)
            .disabled(self.disabled)
            .menu_width(rems(20.0))
            .search_placeholder("Search models…")
            .empty(|_, _| Label::new("No models available").muted())
    }
}

/// The choices and current selection one provider/model pair should show.
///
/// Built by a window from the controller, then handed to
/// [`SelectFields::sync`], which rebuilds the menu items only when this value
/// changes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectSnapshot {
    pub providers: Vec<SelectOption>,
    pub selected_provider: Option<String>,
    pub models: Vec<SelectOption>,
    pub selected_model: Option<String>,
}

impl SelectSnapshot {
    /// Whether this snapshot differs from the one last applied.
    ///
    /// Kept separate from [`SelectFields::sync`] so the idempotence rule can be
    /// tested without a window.
    #[must_use]
    pub fn differs_from(&self, applied: Option<&SelectSnapshot>) -> bool {
        applied != Some(self)
    }
}

/// The two select states of one provider/model pair plus the snapshot they were
/// last built from.
pub struct SelectFields {
    provider: Entity<SelectState<SearchableVec<ProviderOption>>>,
    model: Entity<SelectState<SearchableVec<ModelOption>>>,
    applied: Option<SelectSnapshot>,
}

impl SelectFields {
    /// Creates both states empty. Only the model menu is searchable.
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        let provider = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(Vec::<ProviderOption>::new()),
                None,
                window,
                cx,
            )
        });
        let model = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(Vec::<ModelOption>::new()),
                None,
                window,
                cx,
            )
            .searchable(true)
        });
        Self {
            provider,
            model,
            applied: None,
        }
    }

    #[must_use]
    pub fn provider(&self) -> &Entity<SelectState<SearchableVec<ProviderOption>>> {
        &self.provider
    }

    #[must_use]
    pub fn model(&self) -> &Entity<SelectState<SearchableVec<ModelOption>>> {
        &self.model
    }

    /// Rebuilds items and selection only when `snapshot` differs from the last
    /// one applied. Returns `true` when it changed something.
    ///
    /// A selection whose id is no longer offered is cleared rather than left
    /// dangling, so a catalogue refresh cannot show a value that is not in the
    /// menu.
    pub fn sync(&mut self, snapshot: SelectSnapshot, window: &mut Window, cx: &mut App) -> bool {
        if !snapshot.differs_from(self.applied.as_ref()) {
            return false;
        }
        apply_providers(
            &self.provider,
            &snapshot.providers,
            &snapshot.selected_provider,
            window,
            cx,
        );
        apply_models(
            &self.model,
            &snapshot.models,
            &snapshot.selected_model,
            window,
            cx,
        );
        self.applied = Some(snapshot);
        true
    }
}

/// The choices and selections the Settings dialog should show: every provider,
/// the streaming default, the batch default, the cleanup model and the dictation
/// language.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsSnapshot {
    pub providers: Vec<SelectOption>,
    pub selected_provider: Option<String>,
    pub streaming_models: Vec<SelectOption>,
    pub selected_streaming_model: Option<String>,
    pub batch_models: Vec<SelectOption>,
    pub selected_batch_model: Option<String>,
    /// Text models offered for the cleanup pass; empty when the worker reports
    /// none, in which case the dialog names the resolved default instead of
    /// showing a menu.
    pub cleanup_models: Vec<SelectOption>,
    pub selected_cleanup_model: Option<String>,
    /// Offered dictation languages, Auto first.
    pub languages: Vec<SelectOption>,
    pub selected_language: Option<String>,
}

impl SettingsSnapshot {
    /// Whether this snapshot differs from the one last applied.
    #[must_use]
    pub fn differs_from(&self, applied: Option<&SettingsSnapshot>) -> bool {
        applied != Some(self)
    }
}

/// The Settings dialog's select states — provider, streaming default, batch
/// default, cleanup model and dictation language — plus the snapshot they were
/// last built from.
pub struct SettingsSelectFields {
    provider: Entity<SelectState<SearchableVec<ProviderOption>>>,
    streaming: Entity<SelectState<SearchableVec<ModelOption>>>,
    batch: Entity<SelectState<SearchableVec<ModelOption>>>,
    cleanup: Entity<SelectState<SearchableVec<ModelOption>>>,
    language: Entity<SelectState<SearchableVec<LanguageItem>>>,
    applied: Option<SettingsSnapshot>,
}

impl SettingsSelectFields {
    /// Creates every state empty. Both take-default menus and the cleanup menu
    /// are searchable; the provider and language menus are not.
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        let provider = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(Vec::<ProviderOption>::new()),
                None,
                window,
                cx,
            )
        });
        let streaming = cx.new(|cx| searchable_model_state(window, cx));
        let batch = cx.new(|cx| searchable_model_state(window, cx));
        let cleanup = cx.new(|cx| searchable_model_state(window, cx));
        let language = cx.new(|cx| {
            SelectState::new(
                SearchableVec::new(Vec::<LanguageItem>::new()),
                None,
                window,
                cx,
            )
        });
        Self {
            provider,
            streaming,
            batch,
            cleanup,
            language,
            applied: None,
        }
    }

    #[must_use]
    pub fn provider(&self) -> &Entity<SelectState<SearchableVec<ProviderOption>>> {
        &self.provider
    }

    #[must_use]
    pub fn streaming(&self) -> &Entity<SelectState<SearchableVec<ModelOption>>> {
        &self.streaming
    }

    #[must_use]
    pub fn batch(&self) -> &Entity<SelectState<SearchableVec<ModelOption>>> {
        &self.batch
    }

    /// The cleanup-model menu.
    #[must_use]
    pub fn cleanup(&self) -> &Entity<SelectState<SearchableVec<ModelOption>>> {
        &self.cleanup
    }

    /// The dictation-language menu.
    #[must_use]
    pub fn language(&self) -> &Entity<SelectState<SearchableVec<LanguageItem>>> {
        &self.language
    }

    /// Rebuilds the menus only when `snapshot` differs from the last one
    /// applied, using the same rule as [`SelectFields::sync`].
    pub fn sync(&mut self, snapshot: SettingsSnapshot, window: &mut Window, cx: &mut App) -> bool {
        if !snapshot.differs_from(self.applied.as_ref()) {
            return false;
        }
        apply_providers(
            &self.provider,
            &snapshot.providers,
            &snapshot.selected_provider,
            window,
            cx,
        );
        apply_models(
            &self.streaming,
            &snapshot.streaming_models,
            &snapshot.selected_streaming_model,
            window,
            cx,
        );
        apply_models(
            &self.batch,
            &snapshot.batch_models,
            &snapshot.selected_batch_model,
            window,
            cx,
        );
        apply_models(
            &self.cleanup,
            &snapshot.cleanup_models,
            &snapshot.selected_cleanup_model,
            window,
            cx,
        );
        apply_languages(
            &self.language,
            &snapshot.languages,
            &snapshot.selected_language,
            window,
            cx,
        );
        self.applied = Some(snapshot);
        true
    }
}

/// Replaces a provider menu's items and selection.
fn apply_providers(
    state: &Entity<SelectState<SearchableVec<ProviderOption>>>,
    options: &[SelectOption],
    selected: &Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let items: Vec<ProviderOption> = options.iter().map(ProviderOption::from).collect();
    let value = offered_value(selected, options);
    state.update(cx, |state, cx| {
        state.set_items(SearchableVec::new(items), window, cx);
        match &value {
            Some(value) => state.set_selected_value(value, window, cx),
            None => state.set_selected_index(None, window, cx),
        }
    });
}

/// Replaces a model menu's items and selection.
pub(crate) fn apply_models(
    state: &Entity<SelectState<SearchableVec<ModelOption>>>,
    options: &[SelectOption],
    selected: &Option<String>,
    window: &mut Window,
    cx: &mut App,
) {
    let items: Vec<ModelOption> = options.iter().map(ModelOption::from).collect();
    let value = offered_value(selected, options);
    state.update(cx, |state, cx| {
        state.set_items(SearchableVec::new(items), window, cx);
        match &value {
            Some(value) => state.set_selected_value(value, window, cx),
            None => state.set_selected_index(None, window, cx),
        }
    });
}

/// An empty, searchable model menu.
pub(crate) fn searchable_model_state(
    window: &mut Window,
    cx: &mut Context<'_, SelectState<SearchableVec<ModelOption>>>,
) -> SelectState<SearchableVec<ModelOption>> {
    SelectState::new(
        SearchableVec::new(Vec::<ModelOption>::new()),
        None,
        window,
        cx,
    )
    .searchable(true)
}

/// The selected id as a `SharedString` when the menu still offers it.
///
/// Shared by every menu in `components`, so the "clear a dangling selection"
/// rule is written once.
pub(crate) fn offered_value(
    selected: &Option<String>,
    options: &[SelectOption],
) -> Option<SharedString> {
    let id = selected.as_deref()?;
    options
        .iter()
        .any(|option| option.id == id)
        .then(|| SharedString::from(id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use speechtt_core::protocol::TranscriptionMode;

    fn provider() -> ProviderOption {
        ProviderOption {
            id: "vercel-ai-gateway".into(),
            label: "Vercel AI Gateway".into(),
        }
    }

    fn model(mode: Option<TranscriptionMode>) -> ModelOption {
        ModelOption {
            id: "openai/gpt-realtime-whisper".into(),
            label: "GPT Realtime Whisper".into(),
            mode,
        }
    }

    #[test]
    fn provider_value_is_the_catalogue_id() {
        assert_eq!(provider().value().as_ref(), "vercel-ai-gateway");
    }

    #[test]
    fn provider_title_is_the_catalogue_label() {
        assert_eq!(provider().title().as_ref(), "Vercel AI Gateway");
    }

    #[test]
    fn provider_matches_id_and_label_in_any_case() {
        let provider = provider();
        assert!(provider.matches("gateway"));
        assert!(provider.matches("VERCEL"));
        assert!(provider.matches("vercel-ai-gateway"));
        assert!(provider.matches("AI Gateway"));
        assert!(!provider.matches("mistral"));
    }

    #[test]
    fn model_value_is_the_catalogue_id() {
        assert_eq!(
            model(Some(TranscriptionMode::Streaming)).value().as_ref(),
            "openai/gpt-realtime-whisper"
        );
    }

    #[test]
    fn model_title_is_the_label_without_the_mode() {
        assert_eq!(
            model(Some(TranscriptionMode::Streaming)).title().as_ref(),
            "GPT Realtime Whisper"
        );
    }

    #[test]
    fn model_matches_id_and_label_in_any_case() {
        let model = model(Some(TranscriptionMode::Streaming));
        assert!(model.matches("realtime"));
        assert!(model.matches("WHISPER"));
        assert!(model.matches("openai/"));
        assert!(model.matches("gpt-realtime-whisper"));
        assert!(!model.matches("transcribe"));
    }

    #[test]
    fn model_retains_its_catalogue_mode() {
        assert_eq!(
            model(Some(TranscriptionMode::Segmented)).mode,
            Some(TranscriptionMode::Segmented)
        );
        assert_eq!(model(None).mode, None);
    }

    #[test]
    fn a_snapshot_only_differs_when_its_contents_change() {
        let snapshot = SelectSnapshot {
            providers: vec![SelectOption {
                id: "vercel-ai-gateway".to_string(),
                label: "Vercel AI Gateway".to_string(),
                mode: None,
            }],
            selected_provider: Some("vercel-ai-gateway".to_string()),
            models: Vec::new(),
            selected_model: None,
        };

        assert!(snapshot.differs_from(None));
        assert!(!snapshot.differs_from(Some(&snapshot.clone())));

        let mut changed = snapshot.clone();
        changed.selected_model = Some("openai/gpt-4o-transcribe".to_string());
        assert!(changed.differs_from(Some(&snapshot)));
    }
}
