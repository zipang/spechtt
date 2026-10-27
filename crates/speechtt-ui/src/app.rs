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
use gpui_kit::base::v_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::input::{InputEvent, InputState};
use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::{
    AnyWindowHandle, App, Bounds, Context, Entity, Pixels, Point, Size, Subscription, Task, Window,
    WindowBounds, WindowId, WindowKind, WindowOptions, px,
};
use speechtt_core::backend::SttSessionSink;
use speechtt_core::session::{OutputTarget, SessionField, SessionState};

use crate::components::{AppHeader, IconButton};
use crate::controller::{
    Dialog, FinishedTake, SessionController, SettingsChange, SettingsDraft, ViewState,
};
use crate::views;
use crate::views::dialogs::{
    DIALOG_WIDTH, DialogWindow, HELP_DIALOG_HEIGHT, SETTINGS_DIALOG_HEIGHT,
};

/// Application title shown in the header strip.
pub const APP_TITLE: &str = "SpeechTT";

/// Root view of the widget window.
pub struct SpeechTtApp {
    controller: SessionController,
    session_name: Entity<InputState>,
    settings_api_key: Entity<InputState>,
    settings_key_revealed: bool,
    /// The open dialog window, if any. Dialogs are separate floating windows.
    dialog_window: Option<(Dialog, AnyWindowHandle)>,
    /// Mirrors the dialog window id for the window-closed observer, which runs
    /// outside this entity and must not re-enter it for our own closes.
    dialog_window_id: Rc<Cell<Option<WindowId>>>,
    pending_settings: Option<SettingsChange>,
    /// Keeps the input subscriptions alive; dropped with the view.
    _subscriptions: Vec<Subscription>,
    /// Backend event pump; dropped (and cancelled) with the view.
    _event_task: Option<Task<()>>,
}

impl SpeechTtApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let session_name =
            cx.new(|cx| InputState::new(window, cx).placeholder("e.g. meeting-notes"));
        let settings_api_key = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Paste your provider API key")
                .masked(true)
        });
        // Mirror the input into the controller so `Create session` enables live.
        let subscription = cx.subscribe(&session_name, |this, input, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.controller
                    .set_draft_name(input.read(cx).value().to_string());
                cx.notify();
            }
        });
        let settings_subscription =
            cx.subscribe(&settings_api_key, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.controller
                        .set_settings_api_key(input.read(cx).value().to_string());
                    cx.notify();
                }
            });

        // An OS-level close (window manager X button) must clear the dialog
        // state just like the in-window Cancel button does. Our own closes
        // clear the tracked id first, so this observer never re-enters the
        // entity while it is being updated.
        let dialog_window_id = Rc::new(Cell::new(None));
        let weak_app = cx.entity().downgrade();
        let tracked_id = dialog_window_id.clone();
        let window_closed = cx.on_window_closed(move |cx, window_id| {
            if tracked_id.get() != Some(window_id) {
                return;
            }
            if let Some(app) = weak_app.upgrade() {
                app.update(cx, |app, cx| app.handle_window_closed(window_id, cx));
            }
        });

        let mut app = Self {
            controller: SessionController::new(),
            session_name,
            settings_api_key,
            settings_key_revealed: false,
            dialog_window: None,
            dialog_window_id,
            pending_settings: None,
            _subscriptions: vec![subscription, settings_subscription, window_closed],
            _event_task: None,
        };
        // No backend is attached yet (the Bun worker lands in T0003), so this
        // is dormant until `connect_backend` supplies a duplex sink.
        app.start_event_loop(cx);
        app
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

    pub fn active_dialog(&self) -> Option<Dialog> {
        self.controller.active_dialog()
    }

    pub fn settings_draft(&self) -> &SettingsDraft {
        self.controller.settings_draft()
    }

    /// The change staged by `Save changes`, consumed by the settings store in
    /// T0009.
    pub fn pending_settings_change(&self) -> Option<&SettingsChange> {
        self.pending_settings.as_ref()
    }

    /// Whether a dialog currently has its own window open.
    pub fn has_dialog_window(&self) -> bool {
        self.dialog_window.is_some()
    }

    pub(crate) fn settings_api_key_input(&self) -> &Entity<InputState> {
        &self.settings_api_key
    }

    pub(crate) fn settings_key_revealed(&self) -> bool {
        self.settings_key_revealed
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

    // ----- Dialogs -----------------------------------------------------

    /// Opens Help in its own floating window beside the widget.
    pub fn open_help(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_dialog_window(cx);
        self.controller.open_help();
        self.request_dialog_window(Dialog::Help, window, cx);
        cx.notify();
    }

    /// Opens Settings in its own floating window beside the widget.
    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_dialog_window(cx);
        self.controller.open_settings();
        self.settings_key_revealed = false;
        self.settings_api_key.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.set_masked(true, window, cx);
        });
        self.request_dialog_window(Dialog::Settings, window, cx);
        cx.notify();
    }

    /// Cancel semantics: discard the draft. The dialog window closes itself.
    pub fn close_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.controller.close_dialog();
        self.clear_settings_key(window, cx);
        cx.notify();
    }

    /// Escape path — discards the draft and closes the dialog window.
    pub fn dismiss_dialog_with_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.controller.dismiss_dialog_with_escape();
        self.clear_settings_key(window, cx);
        self.close_dialog_window(cx);
        cx.notify();
    }

    /// Schedules the dialog window beside the widget, on the side with room.
    ///
    /// Opening is deferred because a window draws as soon as it is created.
    /// The Settings root reads this entity, so creating it inside an entity
    /// update would violate GPUI's borrow rules. The deferred pass checks the
    /// request is still current, opens outside any entity borrow, then records
    /// the handle. `WindowKind::Floating` keeps it above the widget on the
    /// platforms that support transient windows.
    fn request_dialog_window(
        &mut self,
        dialog: Dialog,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let app = cx.entity();
        let main_window = window.window_handle();
        cx.defer(move |cx| {
            if app.update(cx, |app, _| app.active_dialog()) != Some(dialog) {
                return;
            }

            let title = views::dialogs::window_title(dialog);
            let app_for_window = app.clone();
            let opened = main_window.update(cx, |_, window, cx| {
                let bounds = dialog_window_bounds(dialog, window, cx);
                let options = WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    kind: WindowKind::Floating,
                    focus: true,
                    show: true,
                    is_resizable: false,
                    is_minimizable: false,
                    ..WindowOptions::default()
                };
                gpui_kit::open_window(options, cx, move |window, cx| {
                    window.set_window_title(&title);
                    cx.new(|cx| DialogWindow::new(dialog, app_for_window, window, cx))
                })
            });

            match opened {
                Ok(Ok((handle, _))) => {
                    app.update(cx, |app, cx| app.attach_dialog_window(dialog, handle, cx));
                }
                Ok(Err(_)) | Err(_) => {
                    app.update(cx, |app, cx| app.abandon_dialog(dialog, cx));
                }
            }
        });
    }

    /// Records a freshly opened dialog window, or closes it when the request
    /// was superseded while it was being created.
    fn attach_dialog_window(
        &mut self,
        dialog: Dialog,
        handle: AnyWindowHandle,
        cx: &mut Context<Self>,
    ) {
        if self.controller.active_dialog() == Some(dialog) {
            self.dialog_window_id.set(Some(handle.window_id()));
            self.dialog_window = Some((dialog, handle));
        } else {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    /// Keeps the controller consistent when the platform refuses a window.
    fn abandon_dialog(&mut self, dialog: Dialog, cx: &mut Context<Self>) {
        if self.controller.active_dialog() == Some(dialog) {
            self.controller.close_dialog();
            self.settings_key_revealed = false;
            cx.notify();
        }
    }

    fn close_dialog_window(&mut self, cx: &mut Context<Self>) {
        // Clear first: removing the window synchronously notifies the
        // window-closed observer, which must not re-enter this entity.
        self.dialog_window_id.set(None);
        if let Some((_, handle)) = self.dialog_window.take() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    fn handle_window_closed(&mut self, window_id: WindowId, cx: &mut Context<Self>) {
        let Some((_, handle)) = self.dialog_window else {
            return;
        };
        if handle.window_id() != window_id {
            return;
        }
        self.dialog_window_id.set(None);
        self.dialog_window = None;
        if self.controller.active_dialog().is_some() {
            self.controller.close_dialog();
            self.settings_key_revealed = false;
            cx.notify();
        }
    }

    pub fn cycle_settings_provider(&mut self, cx: &mut Context<Self>) {
        self.controller.cycle_settings_provider();
        cx.notify();
    }

    pub fn cycle_settings_model(&mut self, cx: &mut Context<Self>) {
        self.controller.cycle_settings_model();
        cx.notify();
    }

    /// Mirrors a typed key into both the controller and the masked input.
    pub fn set_settings_api_key_value(
        &mut self,
        value: impl Into<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let value = value.into();
        self.controller.set_settings_api_key(value.clone());
        self.settings_api_key
            .update(cx, |state, cx| state.set_value(value, window, cx));
        cx.notify();
    }

    pub fn toggle_settings_key_reveal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_key_revealed = !self.settings_key_revealed;
        let masked = !self.settings_key_revealed;
        self.settings_api_key
            .update(cx, |state, cx| state.set_masked(masked, window, cx));
        cx.notify();
    }

    /// Commits the Settings draft. Returns `false` and leaves the dialog open
    /// when the draft is incomplete.
    pub fn save_settings(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(change) = self.controller.save_settings() else {
            return false;
        };
        self.apply_settings_change(change);
        cx.notify();
        true
    }

    /// Single hook between the dialog and the future settings store (T0006 /
    /// T0009). For now it only stages the change for the caller.
    pub fn apply_settings_change(&mut self, change: SettingsChange) {
        self.pending_settings = Some(change);
    }

    fn clear_settings_key(&self, window: &mut Window, cx: &mut Context<Self>) {
        self.settings_api_key.update(cx, |state, cx| {
            state.set_value("", window, cx);
            state.set_masked(true, window, cx);
        });
    }

    /// Attach the duplex STT backend and pump its events into the controller.
    ///
    /// Each received event updates live take state and re-renders once, so the
    /// caption well grows token by token while the take is running.
    pub fn connect_backend(&mut self, sink: Box<dyn SttSessionSink>, cx: &mut Context<Self>) {
        self.controller.set_sink(sink);
        self.start_event_loop(cx);
        cx.notify();
    }

    /// Own the backend event receiver, if the controller has one. The task
    /// lives as long as the view; dropping it cancels the pump.
    fn start_event_loop(&mut self, cx: &mut Context<Self>) {
        // Replacing the backend cancels any pump that was reading the old one.
        self._event_task = None;
        let Some(events) = self.controller.take_event_stream() else {
            return;
        };
        self._event_task = Some(cx.spawn(async move |this, cx| {
            while let Ok(message) = events.recv().await {
                let updated = this.update(cx, |this, cx| {
                    match message {
                        Ok(event) => this.controller.handle_stt_event(event),
                        Err(error) => this.controller.report_backend_error(&error),
                    }
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
            }
        }));
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
            .child(
                AppHeader::new(APP_TITLE)
                    .action(
                        IconButton::new("btn-help", IconName::CircleQuestionMark)
                            .label("Open help and shortcuts")
                            .on_click(
                                cx.listener(|this, _event, window, cx| this.open_help(window, cx)),
                            ),
                    )
                    .action(
                        IconButton::new("btn-settings", IconName::Settings)
                            .label("Open settings")
                            .on_click(cx.listener(|this, _event, window, cx| {
                                this.open_settings(window, cx)
                            })),
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
                        ViewState::Recording => views::transport::render_recording(self, cx),
                        ViewState::Paused => views::transport::render_paused(self, cx),
                        ViewState::Stopped => views::stopped::render_stopped(self, cx),
                    }),
            )
    }
}

/// The dialog sits beside the widget, preferring the right edge and falling
/// back to the left when the display runs out of room there.
fn dialog_window_bounds(dialog: Dialog, window: &mut Window, cx: &App) -> Bounds<Pixels> {
    const GAP: f32 = 16.;
    let width = px(DIALOG_WIDTH);
    let height = px(match dialog {
        Dialog::Help => HELP_DIALOG_HEIGHT,
        Dialog::Settings => SETTINGS_DIALOG_HEIGHT,
    });
    let main = window.bounds();
    let mut origin = Point {
        x: main.origin.x + main.size.width + px(GAP),
        y: main.origin.y,
    };

    if let Some(display) = window.display(cx) {
        let display = display.visible_bounds();
        if origin.x + width > display.origin.x + display.size.width {
            origin.x = main.origin.x - width - px(GAP);
        }
        origin.x = origin.x.max(display.origin.x);
        let max_y = display.origin.y + display.size.height - height;
        origin.y = origin.y.min(max_y).max(display.origin.y);
    }

    Bounds {
        origin,
        size: Size { width, height },
    }
}
