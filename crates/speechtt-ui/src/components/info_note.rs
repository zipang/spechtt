//! A recessed note row: small info glyph plus one line of muted copy.
//!
//! Adds one line of context under the control it describes, without competing
//! with the controls around it. Two screens use it: `dialogs::settings`
//! explains why a provider's API key cannot be used yet, and
//! `views::session_setup` says why the model menu lists non-streaming models
//! only.

use gpui_kit::assets::IconName;
use gpui_kit::base::h_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ElementId, IntoElement, RenderOnce, SharedString, TestSupportExt as _, Window, div, px,
};

use crate::components::Label;
use crate::tokens::{color, radius};

#[derive(IntoElement)]
pub struct InfoNote {
    text: SharedString,
    tone: Option<gpui_kit::Hsla>,
    id: Option<ElementId>,
}

impl InfoNote {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            tone: None,
            id: None,
        }
    }

    /// Overrides the glyph colour (defaults to the info role, or the warning
    /// role when the note explains a problem).
    pub fn tone(mut self, tone: gpui_kit::Hsla) -> Self {
        self.tone = Some(tone);
        self
    }

    /// Registers the note under `id` so a headless test can prove the note was
    /// drawn.
    ///
    /// Optional: a note no test queries needs no id. The id must be unique
    /// within its window scope and stable across frames, like every other
    /// element id.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = Some(id.into());
        self
    }
}

impl RenderOnce for InfoNote {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tone = self
            .tone
            .unwrap_or_else(|| gpui_kit::rgb(color::INFO).into());

        let note = h_flex()
            .w_full()
            .items_start()
            .gap_2()
            .p_2()
            .rounded(px(radius::MD))
            .bg(cx.theme().secondary)
            .child(Icon::new(IconName::Info).small().text_color(tone))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(Label::new(self.text).muted()),
            );

        match self.id {
            Some(id) => note.id(id).test_support().into_any_element(),
            None => note.into_any_element(),
        }
    }
}
