//! The SpeechTT widget window view.
//!
//! Phase 2 shell: header + body views derived from the session controller.
//!
//! ```text
//! Blank  → "New Session" prompt → New Session form → Ready (record)
//! Ready  → Recording ⇄ Paused → (Stop → Blank prompt again)
//! ```
//!
//! This module owns the window shell, the workflow state and the public
//! command/query surface. Screen composition lives in [`crate::views`] and
//! reusable controls in [`crate::components`].

use gpui_kit::assets::IconName;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Icon};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, FontWeight, Subscription, Window, div};
use speechtt_core::session::{OutputTarget, SessionField, SessionState};

use crate::controller::{FinishedTake, SessionController, ViewState};
use crate::views;

/// Application title shown in the header strip.
pub const APP_TITLE: &str = "SpeechTT";

/// Root view of the widget window.
pub struct SpeechTtApp {
    controller: SessionController,
    session_name: Entity<InputState>,
    /// Keeps the input subscription alive; dropped with the view.
    _subscriptions: Vec<Subscription>,
}

impl SpeechTtApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let session_name =
            cx.new(|cx| InputState::new(window, cx).placeholder("e.g. meeting-notes"));
        // Mirror the input into the controller so `Create session` enables live.
        let subscription = cx.subscribe(&session_name, |this, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.controller
                    .set_draft_name(input.read(cx).value().to_string());
                cx.notify();
            }
        });

        Self {
            controller: SessionController::new(),
            session_name,
            _subscriptions: vec![subscription],
        }
    }

    // ----- Queries -----------------------------------------------------

    /// What screen the widget currently shows (test surface, no window needed
    /// once the view exists — the controller alone answers this).
    pub fn view_state(&self) -> ViewState {
        self.controller.view_state()
    }

    pub fn session_state(&self) -> SessionState {
        self.controller.session_state()
    }

    pub fn session_missing(&self) -> Vec<SessionField> {
        self.controller.session_missing()
    }

    pub fn is_draft_complete(&self) -> bool {
        self.controller.is_draft_complete()
    }

    pub fn finished_take(&self) -> Option<&FinishedTake> {
        self.controller.finished_take()
    }

    /// Read access used by the screen renderers.
    pub(crate) fn controller(&self) -> &SessionController {
        &self.controller
    }

    /// The editable session-name input, owned by this view.
    pub(crate) fn session_name_input(&self) -> &Entity<InputState> {
        &self.session_name
    }

    // ----- Commands ----------------------------------------------------

    pub fn open_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.controller.open_form();
        self.clear_session_name(window, cx);
        cx.notify();
    }

    pub fn cancel_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.controller.cancel_form();
        self.clear_session_name(window, cx);
        cx.notify();
    }

    pub fn cycle_provider(&mut self, cx: &mut Context<Self>) {
        self.controller.cycle_provider();
        cx.notify();
    }

    pub fn cycle_model(&mut self, cx: &mut Context<Self>) {
        self.controller.cycle_model();
        cx.notify();
    }

    pub fn select_output(&mut self, output: OutputTarget, cx: &mut Context<Self>) {
        self.controller.set_draft_output(output);
        cx.notify();
    }

    /// Set the session name in both the controller and the editable input.
    pub fn set_draft_name(
        &mut self,
        name: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = name.into();
        self.controller.set_draft_name(name.clone());
        self.session_name
            .update(cx, |state, cx| state.set_value(name, window, cx));
        cx.notify();
    }

    pub fn create_session(&mut self, cx: &mut Context<Self>) {
        // Read the editable input directly: change events are not guaranteed
        // to have reached the controller before the click handler runs.
        let name = self.session_name.read(cx).value().to_string();
        self.controller.set_draft_name(name);
        self.controller.create_session();
        cx.notify();
    }

    pub fn start_recording(&mut self, cx: &mut Context<Self>) {
        self.controller.start_recording();
        cx.notify();
    }

    pub fn pause_recording(&mut self, cx: &mut Context<Self>) {
        self.controller.pause_recording();
        cx.notify();
    }

    pub fn resume_recording(&mut self, cx: &mut Context<Self>) {
        self.controller.resume_recording();
        cx.notify();
    }

    pub fn finish_take(&mut self, cx: &mut Context<Self>) {
        self.controller.finish_take();
        cx.notify();
    }

    pub fn new_take(&mut self, cx: &mut Context<Self>) {
        self.controller.new_take();
        cx.notify();
    }

    fn clear_session_name(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.session_name
            .update(cx, |state, cx| state.set_value("", window, cx));
    }
}

impl gpui_kit::Render for SpeechTtApp {
    fn render(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl gpui_kit::IntoElement {
        v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            // Top strip — `--color-surface-dark` via the `title_bar` role.
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .px_4()
                    .py_2()
                    .bg(cx.theme().title_bar)
                    .border_b_1()
                    .border_color(cx.theme().title_bar_border)
                    .child(
                        div()
                            .id("app-title")
                            .font_weight(FontWeight::BOLD)
                            .child(APP_TITLE),
                    )
                    .child(
                        h_flex()
                            .gap_1()
                            .child(help_button())
                            .child(settings_button()),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .overflow_hidden()
                    .px_5()
                    .py_2p5()
                    .justify_between()
                    .child(match self.view_state() {
                        ViewState::Prompt => views::setup::render_prompt(cx),
                        ViewState::Form => views::setup::render_form(self, cx),
                        ViewState::Ready => views::transport::render_ready(cx),
                        ViewState::Recording => views::transport::render_recording(cx),
                        ViewState::Paused => views::transport::render_paused(cx),
                        ViewState::Stopped => views::stopped::render_stopped(self, cx),
                    }),
            )
    }
}

/// The `?` (help/shortcuts) header button. Opens the Help dialog in its
/// own task; a no-op stub for now.
fn help_button() -> gpui_kit::AnyElement {
    Button::new("btn-help")
        .icon(Icon::new(IconName::CircleQuestionMark))
        .accessibility_label("Open help and shortcuts")
        .into_any_element()
}

/// The cog (settings) header button. Same stub status.
fn settings_button() -> gpui_kit::AnyElement {
    Button::new("btn-settings")
        .icon(Icon::new(IconName::Settings))
        .accessibility_label("Open settings")
        .into_any_element()
}
