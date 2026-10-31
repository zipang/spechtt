//! A recessed note row: small info glyph plus one line of muted copy.
//!
//! Used where a screen has to explain a fallback or a disabled state without
//! competing with the controls around it.

use gpui_kit::assets::IconName;
use gpui_kit::base::h_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::{Icon, Sizable as _};
use gpui_kit::prelude::*;
use gpui_kit::{App, IntoElement, RenderOnce, SharedString, Window, div, px};

use crate::components::Label;
use crate::tokens::{color, radius};

#[derive(IntoElement)]
pub struct InfoNote {
    text: SharedString,
    tone: Option<gpui_kit::Hsla>,
}

impl InfoNote {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            tone: None,
        }
    }

    /// Overrides the glyph colour (defaults to the info role, or the warning
    /// role when the note explains a problem).
    pub fn tone(mut self, tone: gpui_kit::Hsla) -> Self {
        self.tone = Some(tone);
        self
    }
}

impl RenderOnce for InfoNote {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let tone = self
            .tone
            .unwrap_or_else(|| gpui_kit::rgb(color::INFO).into());

        h_flex()
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
            )
    }
}
