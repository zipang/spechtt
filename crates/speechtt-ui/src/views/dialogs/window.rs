//! Root view of a standalone dialog window.
//!
//! The window owns no product state: it renders the controller-backed dialog
//! and forwards every intent to [`SpeechTtApp`], then closes itself. It uses
//! the same Header + Body structure as the widget window.

use gpui_kit::assets::IconName;
use gpui_kit::base::v_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, FocusHandle, IntoElement, KeyDownEvent, Render, Window};

use crate::app::SpeechTtApp;
use crate::components::{AppHeader, IconButton};
use crate::controller::{Dialog, HelpTab};
use crate::views::dialogs::{help, settings, window_title};

pub(crate) struct DialogWindow {
    dialog: Dialog,
    app: Entity<SpeechTtApp>,
    focus: FocusHandle,
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
        Self { dialog, app, focus }
    }

    pub(crate) fn app(&self) -> &Entity<SpeechTtApp> {
        &self.app
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

    pub(crate) fn cycle_provider(&mut self, cx: &mut Context<Self>) {
        self.app
            .update(cx, |app, cx| app.cycle_settings_provider(cx));
    }

    pub(crate) fn cycle_model(&mut self, cx: &mut Context<Self>) {
        self.app.update(cx, |app, cx| app.cycle_settings_model(cx));
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
