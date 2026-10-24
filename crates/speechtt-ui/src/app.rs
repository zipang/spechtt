//! The SpeechTT widget window view.
//!
//! Phase 2 shell: header + body views derived from the session machine.
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
//! ElementIds here are the test contract — they must stay stable; labels,
//! copy and layout are free to change (`tests/session_flow.rs`).

use gpui_kit::assets::IconName;
use gpui_kit::base::{h_flex, v_flex};
use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    Context, Entity, FontWeight, Hsla, Subscription, TestSupportExt as _, Window, div, px, rgb,
};
use speechtt_core::session::{FinishedSession, OutputTarget, SessionMachine, SessionState};

use crate::components::{BadgeTone, CaptionWell, ElapsedTimer, StatusBadge, Waveform};

use crate::tokens::{color, font, radius};

/// Mock feed until the core (Phase 3/8) is wired: elapsed time, FFT-shaped
/// bars and transcript lines mirroring `recording.png` / `paused.png`.
pub const MOCK_ELAPSED_SECONDS: u32 = 42;
const MOCK_BARS: [f32; 36] = [
    0.28, 0.55, 0.34, 0.60, 0.78, 0.41, 0.52, 0.33, 0.72, 0.95, 0.85, 0.48, 0.30, 0.42,
    0.66, 0.87, 0.94, 0.58, 0.36, 0.26, 0.47, 0.70, 0.89, 0.81, 0.44, 0.32, 0.56, 0.75,
    0.90, 0.63, 0.39, 0.29, 0.50, 0.68, 0.46, 0.24,
];
const MOCK_COMMITTED: &str = "...so I think the best approach here is to split the work into two phases, starting with the";
const MOCK_LIVE: &str = "audio capture layer";

/// The finished take shown in the `Stopped` screen. The machine hands the
/// session identity back on `stop`; the transcript itself is owned here
/// (mock text until the core streams it in Phase 8).
struct FinishedTake {
    session: FinishedSession,
    committed: String,
    live: String,
}

/// Application title shown in the header strip.
pub const APP_TITLE: &str = "SpeechTT";

/// Mock audio source until device enumeration lands (Phase 3).
pub const DEFAULT_AUDIO_SOURCE: &str = "System default microphone";

/// Mock provider registry until the TypeScript/provider layer is wired
/// (Phases 6/8). The form cycles through these; the real Select control and
/// capability detection replace it later.
struct Provider {
    name: &'static str,
    models: &'static [&'static str],
}

const PROVIDERS: &[Provider] = &[
    Provider {
        name: "Vercel AI Gateway",
        models: &["voxtral-mini-transcribe-3", "whisper-large-v3"],
    },
    Provider {
        name: "Mistral AI",
        models: &["voxtral-mini-latest", "voxtral-small-latest"],
    },
];

/// Which setup screen the body shows while the machine is `Blank`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Prompt,
    Form,
}

/// Form values not yet committed to the machine.
#[derive(Default)]
struct SessionDraft {
    provider: Option<usize>,
    model: Option<usize>,
    output: Option<OutputTarget>,
}

/// Root view of the widget window.
pub struct SpeechTtApp {
    machine: SessionMachine,
    screen: Screen,
    draft: SessionDraft,
    session_name: Entity<InputState>,
    /// Set when a take finishes: drives the `Stopped` screen even though the
    /// machine has already reset to `Blank` for the next take.
    result: Option<FinishedTake>,
    /// Keeps the input subscription alive; dropped with the view.
    _subscriptions: Vec<Subscription>,
}

impl SpeechTtApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut machine = SessionMachine::new();
        // The mock "idle" frame assumes a usable default mic: no device
        // row on the setup form.
        machine
            .set_audio_source(DEFAULT_AUDIO_SOURCE)
            .expect("blank machine accepts setup");

        let session_name =
            cx.new(|cx| InputState::new(window, cx).placeholder("e.g. meeting-notes"));
        // Re-render as the user types so `Create session` enables live.
        let subscription = cx.subscribe_in(
            &session_name,
            window,
            |_this, _input, event: &InputEvent, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            },
        );

        Self {
            machine,
            screen: Screen::Prompt,
            draft: SessionDraft::default(),
            session_name,
            result: None,
            _subscriptions: vec![subscription],
        }
    }

    /// Test surface: the machine owns the truth, the view just reflects it.
    pub fn session_state(&self) -> SessionState {
        self.machine.state()
    }

    pub fn session_missing(&self) -> Vec<speechtt_core::session::SessionField> {
        self.machine.missing()
    }

    fn draft_complete(&self, cx: &Context<Self>) -> bool {
        self.draft.provider.is_some()
            && self.draft.model.is_some()
            && self.draft.output.is_some()
            && !self.session_name.read(cx).value().trim().is_empty()
    }

    fn open_form(&mut self, cx: &mut Context<Self>) {
        self.screen = Screen::Form;
        self.draft = SessionDraft::default();
        cx.notify();
    }

    fn cancel_form(&mut self, cx: &mut Context<Self>) {
        self.screen = Screen::Prompt;
        self.draft = SessionDraft::default();
        cx.notify();
    }

    fn cycle_provider(&mut self, cx: &mut Context<Self>) {
        let next = self
            .draft
            .provider
            .map_or(0, |index| (index + 1) % PROVIDERS.len());
        self.draft.provider = Some(next);
        // A model belongs to its provider: reset the dependent choice.
        self.draft.model = None;
        cx.notify();
    }

    fn cycle_model(&mut self, cx: &mut Context<Self>) {
        let Some(provider) = self.draft.provider else {
            return;
        };
        let models = PROVIDERS[provider].models;
        self.draft.model = Some(self.draft.model.map_or(0, |index| (index + 1) % models.len()));
        cx.notify();
    }

    fn create_session(&mut self, cx: &mut Context<Self>) {
        if !self.draft_complete(cx) {
            return;
        }
        let provider = &PROVIDERS[self.draft.provider.expect("checked by draft_complete")];
        let model = provider.models[self.draft.model.expect("checked by draft_complete")];
        let name = self.session_name.read(cx).value().trim().to_string();

        // All five parameters present → the machine derives `Ready`.
        self.machine.set_provider(provider.name).ok();
        self.machine.set_model(model).ok();
        let output = match self.draft.output.clone().expect("checked by draft_complete") {
            OutputTarget::Memory => OutputTarget::Memory,
            // No file picker yet: the default location follows the title.
            OutputTarget::File(_) => OutputTarget::File(format!("~/Documents/{name}.md")),
        };
        self.machine.set_session_name(name).ok();
        self.machine.set_output(output).ok();

        self.screen = Screen::Prompt;
        self.draft = SessionDraft::default();
        cx.notify();
    }

    fn start_recording(&mut self, cx: &mut Context<Self>) {
        self.machine.start().ok();
        cx.notify();
    }

    fn pause_recording(&mut self, cx: &mut Context<Self>) {
        self.machine.pause().ok();
        cx.notify();
    }

    fn resume_recording(&mut self, cx: &mut Context<Self>) {
        self.machine.resume().ok();
        cx.notify();
    }

    fn finish_take(&mut self, cx: &mut Context<Self>) {
        if let Ok(session) = self.machine.stop() {
            self.result = Some(FinishedTake {
                session,
                committed: MOCK_COMMITTED.to_string(),
                live: MOCK_LIVE.to_string(),
            });
        }
        cx.notify();
    }

    /// Leave the `Stopped` screen and start over from `Blank`.
    fn new_take(&mut self, cx: &mut Context<Self>) {
        self.result = None;
        self.screen = Screen::Prompt;
        cx.notify();
    }
}

impl gpui_kit::Render for SpeechTtApp {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl gpui_kit::IntoElement {
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
                            .test_support()
                            .font_weight(FontWeight::BOLD)
                            .child(APP_TITLE),
                    )
                    .child(h_flex().gap_1().child(help_button()).child(settings_button())),
            )
            .child(
                v_flex()
                    .flex_1()
                    .px(px(20.))
                    .py(px(10.))
                    .justify_between()
                    .child(match (self.result.as_ref(), self.machine.state()) {
                        (Some(_), _) => self.render_stopped(cx).into_any_element(),
                        (None, SessionState::Blank) => match self.screen {
                            Screen::Prompt => self.render_prompt(cx).into_any_element(),
                            Screen::Form => self.render_form(cx).into_any_element(),
                        },
                        (None, SessionState::Ready) => self.render_ready(cx).into_any_element(),
                        (None, SessionState::Recording) => {
                            self.render_recording(cx).into_any_element()
                        }
                        (None, SessionState::Paused) => self.render_paused(cx).into_any_element(),
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
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> gpui_kit::AnyElement {
        let listener = cx.listener(move |this, _event, _window, cx| on_click(this, cx));
        h_flex()
            .id(id)
            .test_support()
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
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
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
            .test_support()
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
            .test_support()
            .items_center()
            .gap_4()
            .child(StatusBadge::new("NO SESSION"))
            .child(self.primary_circle("btn-new-session", IconName::Plus, cx, |this, cx| {
                this.open_form(cx)
            }))
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
                    .child(
                        div()
                            .text_size(px(font::SM))
                            .child(help),
                    ),
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
            .test_support()
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
            .draft
            .provider
            .map_or("Choose…".to_string(), |index| PROVIDERS[index].name.to_string());
        let model_label = match (self.draft.provider, self.draft.model) {
            (Some(provider), Some(model)) => PROVIDERS[provider].models[model].to_string(),
            _ => "Choose…".to_string(),
        };
        let can_create = self.draft_complete(cx);
        let has_provider = self.draft.provider.is_some();
        let has_model = self.draft.model.is_some();
        let selected_output = self.draft.output.clone();

        // Build interactive controls first: they need `&mut Context`, while
        // the row helper below only reads it.
        let provider_pill = self.value_pill(
            "field-provider",
            provider_label,
            has_provider,
            cx,
            |this, cx| this.cycle_provider(cx),
        );
        let model_pill = self.value_pill(
            "field-model",
            model_label,
            has_model,
            cx,
            |this, cx| this.cycle_model(cx),
        );
        let memory_pill = self.value_pill(
            "field-output-memory",
            "In memory".to_string(),
            selected_output == Some(OutputTarget::Memory),
            cx,
            |this, cx| {
                this.draft.output = Some(OutputTarget::Memory);
                cx.notify();
            },
        );
        let file_selected = matches!(selected_output, Some(OutputTarget::File(_)));
        let file_pill = self.value_pill(
            "field-output-file",
            "To file".to_string(),
            file_selected,
            cx,
            |this, cx| {
                this.draft.output = Some(OutputTarget::File(String::new()));
                cx.notify();
            },
        );

        v_flex()
            .id("session-form")
            .test_support()
            .size_full()
            // Compact: the 480×280 widget body must fit pill + 4 rows +
            // footer without clipping.
            .gap_1()
            .child(StatusBadge::accent("NEW SESSION", rgb(color::BRAND_ACCENT).into()))
            .child(self.form_row(
                "TRANSCRIPTION SERVICE",
                "Which engine transcribes your voice",
                provider_pill,
                cx,
            ))
            .child(self.form_row(
                "MODEL",
                "Used for this session",
                model_pill,
                cx,
            ))
            .child(self.form_row(
                "SESSION TITLE",
                "Names this take and its default file",
                Input::new(&self.session_name)
                    .id("input-session-name")
                    .w(px(200.)),
                cx,
            ))
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
                            .test_support()
                            .px_3()
                            .py_1()
                            .rounded(px(radius::SM))
                            .bg(cx.theme().secondary)
                            .text_size(px(font::SM))
                            .cursor_pointer()
                            .hover(|this| this.bg(cx.theme().secondary_hover))
                            .on_click(cx.listener(|this, _event, _window, cx| this.cancel_form(cx)))
                            .child("Cancel"),
                    )
                    .child(
                        div()
                            .id("btn-create-session")
                            .test_support()
                            .px_3()
                            .py_1()
                            .rounded(px(radius::SM))
                            .bg(cx.theme().primary)
                            .text_color(cx.theme().primary_foreground)
                            .text_size(px(font::SM))
                            .font_weight(FontWeight::MEDIUM)
                            .when(can_create, |this| this.cursor_pointer())
                            .opacity(if can_create { 1.0 } else { 0.4 })
                            .on_click(cx.listener(|this, _event, _window, cx| this.create_session(cx)))
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
                    .child(self.primary_circle("btn-record", IconName::Mic, cx, |this, cx| {
                        this.start_recording(cx)
                    }))
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
                    .child(self.primary_circle("btn-pause", IconName::Pause, cx, |this, cx| {
                        this.pause_recording(cx)
                    }))
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
                        |this, cx| this.resume_recording(cx),
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
            .test_support()
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
        let Some(result) = self.result.as_ref() else {
            return StatusBadge::new("STOPPED").into_any_element();
        };
        let is_file = matches!(result.session.output, OutputTarget::File(_));
        let path = match &result.session.output {
            OutputTarget::File(path) => path.clone(),
            OutputTarget::Memory => String::new(),
        };
        let transcript_committed = result.committed.clone();
        let transcript_live = result.live.clone();

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
            .test_support()
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
                            .test_support()
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
                                .test_support()
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
            .child(
                h_flex()
                    .flex_1()
                    .items_end()
                    .justify_end()
                    .child(controls),
            )
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
