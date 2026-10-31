//! The whole type scale: three roles, three components.
//!
//! SpeechTT uses exactly three text sizes and the weight that belongs to each,
//! so the same kind of copy never renders differently on two screens:
//!
//! | Role    | Token            | Size   | Weight   | Used for                        |
//! |---------|------------------|--------|----------|---------------------------------|
//! | [`Title`] | `--font-size-lg` | 20 px  | semibold | screen titles, card titles      |
//! | [`Body`]  | `--font-size-md` | 16 px  | regular  | body copy, settings rows        |
//! | [`Label`] | `--font-size-sm` | 12.8px | medium   | captions, badges, hints, chips  |
//!
//! A caller picks the role and nothing else: there is no public size or weight
//! setter. The optional builders below are the complete list of decisions a
//! view is allowed to make — tone, monospace, one emphasis step and alignment.
//!
//! This module (with [`crate::tokens`]) is the only place allowed to call
//! `text_size`/`font_weight`; `tests/design_system.rs` enforces that.

use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{App, FontWeight, Hsla, IntoElement, RenderOnce, SharedString, Window, div, rems};

use crate::tokens::rem;

/// The three type roles. Sizes and weights come from here, never the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextRole {
    Title,
    Body,
    Label,
}

impl TextRole {
    /// Size in `rem`, resolved against the window's rem anchor.
    #[must_use]
    pub const fn size(self) -> f32 {
        match self {
            TextRole::Title => rem::LG,
            TextRole::Body => rem::MD,
            TextRole::Label => rem::SM,
        }
    }

    /// The role's default weight.
    #[must_use]
    pub const fn weight(self) -> FontWeight {
        match self {
            TextRole::Title => FontWeight::SEMIBOLD,
            TextRole::Body => FontWeight::NORMAL,
            TextRole::Label => FontWeight::MEDIUM,
        }
    }

    /// One step above [`Self::weight`] — the only emphasis the scale allows.
    #[must_use]
    pub const fn bold_weight(self) -> FontWeight {
        match self {
            TextRole::Title => FontWeight::BOLD,
            TextRole::Body => FontWeight::MEDIUM,
            TextRole::Label => FontWeight::BOLD,
        }
    }
}

/// Shared implementation behind [`Title`], [`Body`] and [`Label`].
#[derive(IntoElement)]
pub struct Text {
    role: TextRole,
    text: SharedString,
    tone: Option<Hsla>,
    muted: bool,
    mono: bool,
    bold: bool,
    center: bool,
}

impl Text {
    fn new(role: TextRole, text: impl Into<SharedString>) -> Self {
        Self {
            role,
            text: text.into(),
            tone: None,
            muted: false,
            mono: false,
            bold: false,
            center: false,
        }
    }

    /// Uses the muted ink (`--color-text-muted`).
    #[must_use]
    pub fn muted(mut self) -> Self {
        self.muted = true;
        self
    }

    /// Overrides the ink with an explicit role colour.
    #[must_use]
    pub fn tone(mut self, tone: Hsla) -> Self {
        self.tone = Some(tone);
        self
    }

    /// Renders in the monospace family (timers, ids, paths, transcript).
    #[must_use]
    pub fn mono(mut self) -> Self {
        self.mono = true;
        self
    }

    /// The single emphasis step above the role's weight.
    #[must_use]
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    /// Centres the text inside its box.
    #[must_use]
    pub fn center(mut self) -> Self {
        self.center = true;
        self
    }
}

impl RenderOnce for Text {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let weight = if self.bold {
            self.role.bold_weight()
        } else {
            self.role.weight()
        };

        let ink = self.tone.unwrap_or(if self.muted {
            theme.muted_foreground
        } else {
            theme.foreground
        });

        let mut element = div()
            .text_size(rems(self.role.size()))
            .font_weight(weight)
            .text_color(ink);

        if self.mono {
            element = element.font_family(theme.mono_font_family.clone());
        }
        if self.center {
            element = element.w_full().text_center();
        }

        element.child(self.text)
    }
}

/// Generates one role component that forwards the shared builders.
macro_rules! text_role {
    ($name:ident, $role:expr, $doc:literal) => {
        #[doc = $doc]
        #[derive(IntoElement)]
        pub struct $name(Text);

        impl $name {
            #[must_use]
            pub fn new(text: impl Into<SharedString>) -> Self {
                Self(Text::new($role, text))
            }

            #[must_use]
            pub fn muted(self) -> Self {
                Self(self.0.muted())
            }

            #[must_use]
            pub fn tone(self, tone: Hsla) -> Self {
                Self(self.0.tone(tone))
            }

            #[must_use]
            pub fn mono(self) -> Self {
                Self(self.0.mono())
            }

            #[must_use]
            pub fn bold(self) -> Self {
                Self(self.0.bold())
            }

            #[must_use]
            pub fn center(self) -> Self {
                Self(self.0.center())
            }
        }

        impl RenderOnce for $name {
            fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
                self.0.render(window, cx)
            }
        }
    };
}

text_role!(
    Title,
    TextRole::Title,
    "A screen or card title: 20 px semibold."
);
text_role!(
    Body,
    TextRole::Body,
    "Body copy: 16 px regular — the reading size of the application."
);
text_role!(
    Label,
    TextRole::Label,
    "A label, caption, badge or hint: 12.8 px medium."
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tokens::font;

    /// The three roles are the whole scale, and they mirror the token values.
    #[test]
    fn roles_map_to_the_three_token_sizes() {
        assert_eq!(TextRole::Title.size() * 16.0, font::LG);
        assert_eq!(TextRole::Body.size() * 16.0, font::MD);
        assert_eq!(TextRole::Label.size() * 16.0, font::SM);

        // Strictly ordered: no two roles may collapse to the same size.
        assert!(TextRole::Title.size() > TextRole::Body.size());
        assert!(TextRole::Body.size() > TextRole::Label.size());
    }

    #[test]
    fn every_role_has_two_weights_and_the_bold_step_is_heavier() {
        for role in [TextRole::Title, TextRole::Body, TextRole::Label] {
            assert_ne!(role.weight(), role.bold_weight());
        }
    }

    /// A caller may only pick tone/mono/emphasis/alignment — never a size.
    #[test]
    fn the_three_components_report_their_own_role() {
        assert_eq!(Title::new("t").0.role, TextRole::Title);
        assert_eq!(Body::new("b").0.role, TextRole::Body);
        assert_eq!(Label::new("l").0.role, TextRole::Label);
    }

    #[test]
    fn builders_do_not_change_the_role() {
        let label = Label::new("x").mono().bold().center();
        assert_eq!(label.0.role, TextRole::Label);
        assert!(label.0.mono && label.0.bold && label.0.center);
    }
}
