//! Keycap row used by the Help dialog: one bordered box per key, joined by a
//! muted `+` (e.g. `Ctrl + Shift + R`).

use gpui_kit::base::h_flex;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{App, IntoElement, RenderOnce, SharedString, Window, div, px};

use crate::components::{Body, Label};
use crate::tokens::radius;

#[derive(IntoElement)]
pub struct KeyCap {
    keys: Vec<SharedString>,
}

impl KeyCap {
    pub fn new(keys: impl IntoIterator<Item = impl Into<SharedString>>) -> Self {
        Self {
            keys: keys.into_iter().map(Into::into).collect(),
        }
    }
}

impl RenderOnce for KeyCap {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut row = h_flex().items_center().gap_2();
        for (index, key) in self.keys.into_iter().enumerate() {
            if index > 0 {
                row = row.child(Label::new("+").muted());
            }
            row = row.child(
                div()
                    .px_2()
                    .py_1()
                    .rounded(px(radius::SM))
                    .border_1()
                    .border_color(cx.theme().border)
                    .bg(cx.theme().secondary)
                    .child(Body::new(key).bold()),
            );
        }
        row
    }
}
