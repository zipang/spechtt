//! The SpeechTT widget window view.
//!
//! Phase 2 shell: header + body views derived from the session controller.
//!
//! ```text
//! Blank  → "New Session" prompt → New Session form → Ready (record)
//! Ready  → Recording ⇄ Paused → (Stop → Blank prompt again)
//! ```
//!
//! The two setup screens are *inferred* from the design system because the
//! Paper mockups only cover the Ready/Recording/Paused/… frames. They reuse
//! the settings-dialog row language (uppercase micro-label, helper line,
//! value pill on the right).
//!
//! All workflow state lives in [`SessionController`]; this module only
//! translates it into elements and forwards user actions. Layout, copy and
//! element structure are free to change without affecting the tests.

use gpui_kit::assets::IconName;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, FontWeight, Hsla, Subscription, Window, div, px, rgb};
use speechtt_core::session::{OutputTarget, SessionField, SessionState};

use crate::components::{BadgeTone, CaptionWell, ElapsedTimer, StatusBadge, Waveform};
use crate::controller::{
    FinishedTake, MOCK_BARS, MOCK_COMMITTED, MOCK_ELAPSED_SECONDS, MOCK_LIVE, SessionController,
    ViewState,
};
use crate::tokens::{color, font, radius};

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
        let subscription = cx.subscribe_in(
            &session_name,
            window,
            |this, input, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    this.controller
                        .set_draft_name(input.read(cx).value().to_string());
                    cx.notify();
                }
            },
        );

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
                    .px(px(20.))
                    .py(px(10.))
                    .justify_between()
                    .child(match self.view_state() {
                        ViewState::Prompt => self.render_prompt(cx).into_any_element(),
                        ViewState::Form => self.render_form(cx).into_any_element(),
                        ViewState::Ready => self.render_ready(cx).into_any_element(),
                        ViewState::Recording => self.render_recording(cx).into_any_element(),
                        ViewState::Paused => self.render_paused(cx).into_any_element(),
                        ViewState::Stopped => self.render_stopped(cx).into_any_element(),
                    }),
            )
    }
}

impl SpeechTtApp {
    /// The transport circle from the mockups (76px): record, stop, resume.
    #[allow(clippy::too_many_arguments)]
    fn circle_action(
        &self,
        id: &'static str,
        icon: IconName,
        diameter: f32,
        bg: Hsla,
        hover_bg: Hsla,
        fg: Hsla,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> gpui_kit::AnyElement {
        let listener = cx.listener(move |this, _event, window, cx| on_click(this, window, cx));
        h_flex()
            .id(id)
            .size(px(diameter))
            .rounded(px(diameter / 2.0))
            .justify_center()
            .items_center()
            .bg(bg)
            .text_color(fg)
            .hover(move |this| this.bg(hover_bg))
            .cursor_pointer()
            .on_click(listener)
            .child(Icon::new(icon).with_size(gpui_kit::component::Size::Large))
            .into_any_element()
    }

    fn primary_circle(
        &self,
        id: &'static str,
        icon: IconName,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
    ) -> gpui_kit::AnyElement {
        let (bg, hover, fg) = (
            cx.theme().primary,
            cx.theme().primary_hover,
            cx.theme().primary_foreground,
        );
        self.circle_action(id, icon, 74.0, bg, hover, fg, cx, on_click)
    }

    /// Ghost square with an action (finish, open file, copy, save).
    fn ghost_control(
        &self,
        id: &'static str,
        icon: IconName,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> gpui_kit::AnyElement {
        let listener = cx.listener(move |this, _event, _window, cx| on_click(this, cx));
        h_flex()
            .id(id)
            .size(px(34.))
            .rounded(px(10.))
            .justify_center()
            .items_center()
            .bg(cx.theme().secondary)
            .text_color(cx.theme().muted_foreground)
            .cursor_pointer()
            .hover(|this| this.bg(cx.theme().secondary_hover))
            .on_click(listener)
            .child(Icon::new(icon))
            .into_any_element()
    }

    fn render_prompt(&mut self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .id("blank-prompt")
            .items_center()
            .gap_4()
            .child(StatusBadge::new("NO SESSION"))
            .child(self.primary_circle(
                "btn-new-session",
                IconName::Plus,
                cx,
                |this, window, cx| this.open_form(window, cx),
            ))
            .child(
                v_flex()
                    .items_center()
                    .gap_1()
                    .child(
                        div()
                            .text_size(px(font::LG))
                            .font_weight(FontWeight::MEDIUM)
                            .child("New Session"),
                    )
                    .child(
                        div()
                            .text_size(px(font::SM))
                            .text_color(cx.theme().muted_foreground)
                            .child("Choose a service, model and output"),
                    ),
            )
            .into_any_element()
    }

    fn form_row(
        &self,
        label: &'static str,
        help: &'static str,
        control: impl IntoElement,
        cx: &Context<Self>,
    ) -> gpui_kit::AnyElement {
        h_flex()
            .justify_between()
            .items_center()
            .gap_3()
            .child(
                v_flex()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(font::XS))
                            .font_weight(FontWeight::BOLD)
                            .text_color(cx.theme().muted_foreground)
                            .child(label),
                    )
                    .child(div().text_size(px(font::SM)).child(help)),
            )
            .child(control)
            .into_any_element()
    }

    fn value_pill(
        &self,
        id: &'static str,
        value: String,
        selected: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> gpui_kit::AnyElement {
        let listener = cx.listener(move |this, _event, _window, cx| on_click(this, cx));
        let (bg, fg) = if selected {
            (cx.theme().primary, cx.theme().primary_foreground)
        } else {
            (cx.theme().secondary, cx.theme().foreground)
        };
        div()
            .id(id)
            .px_3()
            .py_1()
            .rounded(px(radius::SM))
            .bg(bg)
            .text_color(fg)
            .text_size(px(font::SM))
            .cursor_pointer()
            .hover(|this| this.bg(cx.theme().secondary_hover))
            .on_click(listener)
            .child(value)
            .into_any_element()
    }

    fn render_form(&mut self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let provider_label = self
            .controller
            .provider_label()
            .unwrap_or("Choose…")
            .to_string();
        let model_label = self
            .controller
            .model_label()
            .unwrap_or("Choose…")
            .to_string();
        let can_create = self.controller.is_draft_complete();
        let has_provider = self.controller.has_provider();
        let has_model = self.controller.has_model();
        let selected_output = self.controller.selected_output().cloned();

        // Build interactive controls first: they need `&mut Context`, while
        // the row helper below only reads it.
        let provider_pill = self.value_pill(
            "field-provider",
            provider_label,
            has_provider,
            cx,
            |this, cx| this.cycle_provider(cx),
        );
        let model_pill = self.value_pill("field-model", model_label, has_model, cx, |this, cx| {
            this.cycle_model(cx)
        });
        let memory_pill = self.value_pill(
            "field-output-memory",
            "In memory".to_string(),
            selected_output == Some(OutputTarget::Memory),
            cx,
            |this, cx| this.select_output(OutputTarget::Memory, cx),
        );
        let file_selected = matches!(selected_output, Some(OutputTarget::File(_)));
        let file_pill = self.value_pill(
            "field-output-file",
            "To file".to_string(),
            file_selected,
            cx,
            |this, cx| this.select_output(OutputTarget::File(String::new()), cx),
        );

        v_flex()
            .id("session-form")
            .size_full()
            // Compact: the 480×280 widget body must fit pill + 4 rows +
            // footer without clipping.
            .gap_1()
            .child(StatusBadge::accent(
                "NEW SESSION",
                rgb(color::BRAND_ACCENT).into(),
            ))
            .child(self.form_row(
                "TRANSCRIPTION SERVICE",
                "Which engine transcribes your voice",
                provider_pill,
                cx,
            ))
            .child(self.form_row("MODEL", "Used for this session", model_pill, cx))
            .child(
                self.form_row(
                    "SESSION TITLE",
                    "Names this take and its default file",
                    Input::new(&self.session_name)
                        .id("input-session-name")
                        .w(px(200.)),
                    cx,
                ),
            )
            .child(self.form_row(
                "OUTPUT",
                "Keep it in the app or write a file",
                h_flex().gap_1().child(memory_pill).child(file_pill),
                cx,
            ))
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        div()
                            .id("btn-cancel-session")
                            .px_3()
                            .py_1()
                            .rounded(px(radius::SM))
                            .bg(cx.theme().secondary)
                            .text_size(px(font::SM))
                            .cursor_pointer()
                            .hover(|this| this.bg(cx.theme().secondary_hover))
                            .on_click(
                                cx.listener(|this, _event, window, cx| {
                                    this.cancel_form(window, cx)
                                }),
                            )
                            .child("Cancel"),
                    )
                    .child(
                        div()
                            .id("btn-create-session")
                            .px_3()
                            .py_1()
                            .rounded(px(radius::SM))
                            .bg(cx.theme().primary)
                            .text_color(cx.theme().primary_foreground)
                            .text_size(px(font::SM))
                            .font_weight(FontWeight::MEDIUM)
                            .when(can_create, |this| this.cursor_pointer())
                            .opacity(if can_create { 1.0 } else { 0.4 })
                            .on_click(
                                cx.listener(|this, _event, _window, cx| this.create_session(cx)),
                            )
                            .child("Create session"),
                    ),
            )
            .into_any_element()
    }

    fn render_ready(&mut self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .size_full()
            .gap_2()
            .child(StatusBadge::tone("READY", BadgeTone::Ready))
            .child(
                v_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap_4()
                    .child(self.primary_circle(
                        "btn-record",
                        IconName::Mic,
                        cx,
                        |this, _window, cx| this.start_recording(cx),
                    ))
                    .child(
                        div()
                            .text_size(px(font::SM))
                            .text_color(cx.theme().muted_foreground)
                            .child("Press to start dictating"),
                    ),
            )
            .into_any_element()
    }

    fn render_recording(&mut self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .size_full()
            .gap_1()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(StatusBadge::tone("RECORDING", BadgeTone::Recording))
                    .child(ElapsedTimer::new(MOCK_ELAPSED_SECONDS)),
            )
            .child(Waveform::new(MOCK_BARS, true))
            .child(CaptionWell::new(MOCK_COMMITTED, MOCK_LIVE))
            // Same two controls as Paused (layout stays put): Pause toggles
            // the take, Stop stays disabled until the take is paused.
            .child(
                h_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap(px(24.))
                    .child(self.primary_circle(
                        "btn-pause",
                        IconName::Pause,
                        cx,
                        |this, _window, cx| this.pause_recording(cx),
                    ))
                    .child(self.stop_control(false, cx)),
            )
            .into_any_element()
    }

    fn render_paused(&mut self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        v_flex()
            .size_full()
            .gap_1()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(StatusBadge::tone("PAUSED", BadgeTone::Paused))
                    .child(ElapsedTimer::new(MOCK_ELAPSED_SECONDS)),
            )
            .child(Waveform::new(MOCK_BARS, false))
            .child(CaptionWell::new(MOCK_COMMITTED, MOCK_LIVE))
            .child(
                h_flex()
                    .flex_1()
                    .items_center()
                    .justify_center()
                    .gap(px(24.))
                    .child(self.circle_action(
                        "btn-resume",
                        IconName::Play,
                        64.0,
                        rgb(color::WARNING).into(),
                        rgb(color::WARNING).into(),
                        rgb(color::ON_COLOR).into(),
                        cx,
                        |this, _window, cx| this.resume_recording(cx),
                    ))
                    .child(self.stop_control(true, cx)),
            )
            .into_any_element()
    }

    /// Square ghost Stop control, shown in both transport states so the
    /// layout does not shift. Disabled (inert, dimmed) while recording.
    fn stop_control(&self, enabled: bool, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let mut control = h_flex()
            .id("btn-stop")
            .size(px(34.))
            .rounded(px(10.))
            .justify_center()
            .items_center()
            .bg(cx.theme().secondary)
            .text_color(cx.theme().muted_foreground)
            .opacity(if enabled { 1.0 } else { 0.35 })
            .child(Icon::new(IconName::Square));

        if enabled {
            let listener = cx.listener(|this, _event, _window, cx| this.finish_take(cx));
            control = control
                .cursor_pointer()
                .hover(|this| this.bg(cx.theme().secondary_hover))
                .on_click(listener);
        }
        control.into_any_element()
    }

    /// The `Stopped` screen. File takes only expose the written path (and a
    /// way to open it); memory takes expose the transcript with copy/save.
    fn render_stopped(&mut self, cx: &mut Context<Self>) -> gpui_kit::AnyElement {
        let Some(result) = self.controller.finished_take() else {
            return StatusBadge::new("STOPPED").into_any_element();
        };
        let is_file = matches!(result.session().output, OutputTarget::File(_));
        let path = match &result.session().output {
            OutputTarget::File(path) => path.clone(),
            OutputTarget::Memory => String::new(),
        };
        let transcript_committed = result.committed().to_string();
        let transcript_live = result.live().to_string();

        let controls = if is_file {
            h_flex().gap_2().child(self.ghost_control(
                "btn-open-file",
                IconName::FileText,
                cx,
                // TODO(Phase 5): write + launch the editor once the file
                // save adapter exists.
                |_this, _cx| {},
            ))
        } else {
            h_flex()
                .gap_2()
                .child(self.ghost_control(
                    "btn-copy",
                    IconName::Copy,
                    cx,
                    // TODO(Phase 5): clipboard adapter.
                    |_this, _cx| {},
                ))
                .child(self.ghost_control(
                    "btn-save",
                    IconName::HardDriveDownload,
                    cx,
                    // TODO(Phase 5): save-to-file flow.
                    |_this, _cx| {},
                ))
        };

        v_flex()
            .id("result-body")
            .size_full()
            .gap_2()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .child(StatusBadge::new("STOPPED"))
                    .child(
                        div()
                            .id("btn-new-take")
                            .px_3()
                            .py_1()
                            .rounded(px(radius::SM))
                            .bg(cx.theme().secondary)
                            .text_size(px(font::SM))
                            .cursor_pointer()
                            .hover(|this| this.bg(cx.theme().secondary_hover))
                            .on_click(cx.listener(|this, _event, _window, cx| this.new_take(cx)))
                            .child("New take"),
                    ),
            )
            .when(is_file, |this| {
                this.child(
                    v_flex()
                        .gap_1()
                        .px(px(14.))
                        .py(px(8.))
                        .rounded(px(radius::MD))
                        .bg(cx.theme().popover)
                        .child(
                            div()
                                .text_size(px(font::XS))
                                .font_weight(FontWeight::BOLD)
                                .text_color(cx.theme().muted_foreground)
                                .child("TRANSCRIPT WRITTEN TO"),
                        )
                        .child(
                            div()
                                .id("result-path")
                                .font_family(cx.theme().mono_font_family.clone())
                                .text_size(px(font::SM))
                                .text_color(cx.theme().foreground)
                                .child(path),
                        ),
                )
            })
            .when(!is_file, |this| {
                this.child(CaptionWell::new(transcript_committed, transcript_live))
            })
            .child(h_flex().flex_1().items_end().justify_end().child(controls))
            .into_any_element()
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
