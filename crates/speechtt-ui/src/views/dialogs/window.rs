//! Root view of a standalone dialog window.
//!
//! The window holds no product state: it renders the controller-backed dialog
//! and forwards every intent to [`SpeechTtApp`], then closes itself. Its one
//! piece of view state is the pair of Settings dropdown states, refreshed from
//! the app's controller on every frame. It uses the same Header + Body
//! structure as the widget window.

use gpui_kit::assets::IconName;
use gpui_kit::base::v_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::select::{SearchableVec, SelectEvent};
use gpui_kit::prelude::*;
use gpui_kit::{
    Context, Entity, FocusHandle, IntoElement, KeyDownEvent, Render, Subscription, Window,
};

use crate::app::SpeechTtApp;
use crate::components::{
    AppHeader, IconButton, ModelOption, ProviderOption, SettingsSelectFields, SettingsSnapshot,
};
use crate::controller::{Dialog, HelpTab};
use crate::views::dialogs::{help, settings, window_title};

pub(crate) struct DialogWindow {
    dialog: Dialog,
    app: Entity<SpeechTtApp>,
    focus: FocusHandle,
    /// The provider, streaming and batch dropdowns of the Settings dialog.
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
                app_observer,
            ],
        }
    }

    pub(crate) fn app(&self) -> &Entity<SpeechTtApp> {
        &self.app
    }

    /// The Settings provider and default-model dropdown states.
    pub(crate) fn selects(&self) -> &SettingsSelectFields {
        &self.selects
    }

    /// Cancel/Escape/close-button path: discard the draft and close the window.
    pub(crate) fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.app.update(cx, |app, cx| app.close_dialog(window, cx));
        window.remove_window();
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
