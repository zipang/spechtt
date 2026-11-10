//! Root view of a standalone dialog window.
//!
//! The window holds no product state: it renders the controller-backed dialog
//! and forwards every intent to [`SpeechTtApp`], then closes itself. Its one
//! piece of view state is the Settings dropdown states, refreshed from the
//! app's controller on every frame. It uses the same Header + Body structure as
//! the widget window.
//!
//! The root declares [`SPEECHTT_DIALOG_CONTEXT`] and handles the two dialog
//! toggles, so **F1** closes Help or switches to it and **F2** does the same for
//! Settings while a dialog owns focus. A forwarded command runs against the
//! widget window, and this window closes itself when the command replaced or
//! closed its dialog.

use gpui_kit::assets::IconName;
use gpui_kit::base::v_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::select::{SearchableVec, SelectEvent};
use gpui_kit::prelude::*;
use gpui_kit::{
    Context, Entity, FocusHandle, IntoElement, KeyDownEvent, Render, Subscription, Window,
};

use speechtt_core::actions::SessionAction;

use crate::app::{SPEECHTT_DIALOG_CONTEXT, SpeechTtApp, ToggleHelp, ToggleSettings};
use crate::components::{
    AppHeader, IconButton, LanguageItem, ModelOption, ProviderOption, SettingsSelectFields,
    SettingsSnapshot,
};
use crate::controller::{Dialog, HelpTab};
use crate::dialogs::{help, settings, window_title};

pub(crate) struct DialogWindow {
    dialog: Dialog,
    app: Entity<SpeechTtApp>,
    focus: FocusHandle,
    /// The provider, take-default, cleanup-model and language dropdowns of the
    /// Settings dialog.
    selects: SettingsSelectFields,
    /// Keeps the select and app subscriptions alive; dropped with the window.
    _subscriptions: Vec<Subscription>,
}

impl DialogWindow {
    pub(crate) fn new(
        dialog: Dialog,
        app: Entity<SpeechTtApp>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);

        let selects = SettingsSelectFields::new(window, cx);
        let provider_subscription = cx.subscribe(
            selects.provider(),
            |this, _, event: &SelectEvent<SearchableVec<ProviderOption>>, cx| {
                if let SelectEvent::Confirm(Some(id)) = event {
                    let id = id.to_string();
                    this.app
                        .update(cx, |app, cx| app.set_settings_provider(&id, cx));
                }
            },
        );
        let streaming_subscription = cx.subscribe(
            selects.streaming(),
            |this, _, event: &SelectEvent<SearchableVec<ModelOption>>, cx| {
                if let SelectEvent::Confirm(Some(id)) = event {
                    let id = id.to_string();
                    this.app
                        .update(cx, |app, cx| app.set_settings_streaming_model(&id, cx));
                }
            },
        );
        let batch_subscription = cx.subscribe(
            selects.batch(),
            |this, _, event: &SelectEvent<SearchableVec<ModelOption>>, cx| {
                if let SelectEvent::Confirm(Some(id)) = event {
                    let id = id.to_string();
                    this.app
                        .update(cx, |app, cx| app.set_settings_batch_model(&id, cx));
                }
            },
        );
        let cleanup_subscription = cx.subscribe(
            selects.cleanup(),
            |this, _, event: &SelectEvent<SearchableVec<ModelOption>>, cx| {
                if let SelectEvent::Confirm(Some(id)) = event {
                    let id = id.to_string();
                    this.app
                        .update(cx, |app, cx| app.set_settings_cleanup_model(&id, cx));
                }
            },
        );
        let language_subscription = cx.subscribe(
            selects.language(),
            |this, _, event: &SelectEvent<SearchableVec<LanguageItem>>, cx| {
                if let SelectEvent::Confirm(Some(id)) = event {
                    let id = id.to_string();
                    this.app
                        .update(cx, |app, cx| app.set_settings_language(&id, cx));
                }
            },
        );
        // An arriving catalogue changes the menus, so the open dialog repaints.
        let app_observer = cx.observe(&app, |_, _, cx| cx.notify());

        Self {
            dialog,
            app,
            focus,
            selects,
            _subscriptions: vec![
                provider_subscription,
                streaming_subscription,
                batch_subscription,
                cleanup_subscription,
                language_subscription,
                app_observer,
            ],
        }
    }

    pub(crate) fn app(&self) -> &Entity<SpeechTtApp> {
        &self.app
    }

    /// The Settings dropdown states.
    pub(crate) fn selects(&self) -> &SettingsSelectFields {
        &self.selects
    }

    /// Cancel/Escape/close-button path: discard the draft and close the window.
    pub(crate) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.app.update(cx, |app, cx| app.close_dialog(window, cx));
        window.remove_window();
    }

    /// The `F1` handler: opens Help, or closes it when this is the Help window.
    fn on_toggle_help(&mut self, _: &ToggleHelp, window: &mut Window, cx: &mut Context<Self>) {
        self.forward(SessionAction::ToggleHelp, window, cx);
    }

    /// The `F2` handler: opens Settings, or closes it when this is that window.
    fn on_toggle_settings(
        &mut self,
        _: &ToggleSettings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.forward(SessionAction::ToggleSettings, window, cx);
    }

    /// Runs a dialog toggle in the app, then closes this window when the
    /// command closed its dialog or replaced it with the other one.
    ///
    /// The window removes itself rather than being removed through its handle:
    /// it is the window dispatching the keystroke, and reaching into it again
    /// from the app would re-enter a window that is already borrowed.
    fn forward(&mut self, action: SessionAction, window: &mut Window, cx: &mut Context<Self>) {
        self.app.update(cx, |app, cx| {
            app.apply_action(action, window, cx);
        });
        if self.app.read(cx).active_dialog() != Some(self.dialog) {
            window.remove_window();
        }
    }

    /// Switches the Help dialog page (Shortcuts / Models).
    pub(crate) fn select_help_tab(&mut self, tab: HelpTab, cx: &mut Context<Self>) {
        self.app.update(cx, |app, cx| app.set_help_tab(tab, cx));
    }

    pub(crate) fn toggle_key_reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.app
            .update(cx, |app, cx| app.toggle_settings_key_reveal(window, cx));
    }

    /// Save stays in the window when the draft is incomplete.
    pub(crate) fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let saved = self.app.update(cx, |app, cx| app.save_settings(cx));
        if saved {
            window.remove_window();
        }
    }
}

impl Render for DialogWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let snapshot = {
            let app = self.app.read(cx);
            let controller = app.controller();
            let draft = app.settings_draft();
            SettingsSnapshot {
                providers: controller.settings_provider_options(),
                selected_provider: draft.provider.clone(),
                streaming_models: controller.settings_streaming_model_options(),
                selected_streaming_model: draft.model_streaming.clone(),
                batch_models: controller.settings_batch_model_options(),
                selected_batch_model: draft.model_batch.clone(),
                cleanup_models: controller.settings_cleanup_model_options(),
                selected_cleanup_model: draft.language_model.clone(),
                languages: controller.settings_language_options(),
                // An unset language is Auto, whose value is the empty string.
                selected_language: Some(draft.language.clone().unwrap_or_default()),
            }
        };
        self.selects.sync(snapshot, window, cx);

        let body = match self.dialog {
            Dialog::Help => help::render(self, cx),
            Dialog::Settings => settings::render(self, cx),
        };

        let close = IconButton::new("btn-dialog-close", IconName::Close)
            .label("Close dialog")
            .on_click(cx.listener(|this, _event, window, cx| this.close(window, cx)));

        v_flex()
            .size_full()
            .track_focus(&self.focus)
            .key_context(SPEECHTT_DIALOG_CONTEXT)
            .on_action(cx.listener(Self::on_toggle_help))
            .on_action(cx.listener(Self::on_toggle_settings))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.close(window, cx);
                }
            }))
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(AppHeader::new(window_title(self.dialog)).action(close))
            .child(body)
    }
}
