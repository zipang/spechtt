//! The whole type scale: three roles, plus the opt-in that makes one of them
//! selectable.
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
//! view is allowed to make — tone, monospace, one emphasis step, alignment, and
//! whether the copy joins the window's text selection.
//!
//! This module (with [`crate::tokens`]) is the only place allowed to call
//! `text_size`/`font_weight`; `tests/design_system.rs` enforces that.
//!
//! # Who renders this
//!
//! Every screen: `views::welcome`, `views::session_setup`, `views::recording`,
//! `views::session_end`, `views::session_transcription`, and the dialogs under
//! `dialogs`. Every component that carries copy of its own:
//! [`crate::components::AppHeader`], [`crate::components::StatusBadge`],
//! [`crate::components::InfoNote`], [`crate::components::KeyCap`],
//! [`crate::components::ProgressBar`], [`crate::components::SettingRow`],
//! [`crate::components::ElapsedTimer`],
//! [`crate::components::TranscriptionModeBadge`], and
//! [`crate::components::CaptionWell`] — the only caller that makes its runs
//! selectable.

use gpui_kit::base::SelectableText;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ElementId, FontWeight, Hsla, IntoElement, RenderOnce, SharedString, Window, div, rems,
};

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
///
/// The role owns the size, the weight and (when asked) the family. The
/// builders refine the ink and the emphasis, and optionally hand the copy to
/// the window's text selection.
#[derive(IntoElement)]
pub struct Text {
    role: TextRole,
    text: SharedString,
    tone: Option<Hsla>,
    muted: bool,
    mono: bool,
    bold: bool,
    center: bool,
    selectable: Option<ElementId>,
    selection_order: u64,
    selection_color: Option<Hsla>,
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
            selectable: None,
            selection_order: 0,
            selection_color: None,
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

    /// Joins the window's text selection under the element id `id`.
    ///
    /// A selectable run is selectable with the pointer, and the copy shortcut
    /// the window root owns (`Ctrl-C` on Linux, `Cmd-C` on macOS) puts the
    /// selected range on the system clipboard. Only
    /// [`crate::components::CaptionWell`] asks for it: the transcript is the one
    /// piece of copy a reader lifts out by hand, and every other string stays
    /// inert so a drag on a button remains a click.
    ///
    /// `id` is the caller's responsibility. It must be unique per rendered
    /// element and **stable across frames**: the run keeps its selection handle
    /// in the element state under that id, so a changing id recreates the handle
    /// and silently drops the selection the reader is holding. An empty string
    /// is a valid run — it renders nothing and contributes nothing to the
    /// document.
    #[must_use]
    pub fn selectable(mut self, id: impl Into<ElementId>) -> Self {
        self.selectable = Some(id.into());
        self
    }

    /// Places this run in reading order among the other runs of one document.
    ///
    /// Defaults to `0`, which is right for a lone selectable label. A copy that
    /// spans several runs reads them in this order:
    /// [`crate::components::CaptionWell`] passes `0` for the committed text and
    /// `1` for the live tail, so a drag across both copies the finalized words
    /// first. Ignored when the text is not selectable.
    #[must_use]
    pub fn selection_order(mut self, order: u64) -> Self {
        self.selection_order = order;
        self
    }

    /// Overrides the highlight painted behind a selectable run's selected range.
    ///
    /// Defaults to the theme's own selection role. The transcript passes
    /// `brand.primary` at 30 % opacity, which stays readable under both the muted
    /// committed ink and the brand-accent live tail (`DESIGN.md` §Transcript
    /// well). Ignored when the text is not selectable.
    #[must_use]
    pub fn selection_color(mut self, color: Hsla) -> Self {
        self.selection_color = Some(color);
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

        // A selectable run keeps every type decision on this wrapping `div`.
        // `Div` lays its children out and pre-paints them inside its own ambient
        // text style, and `SelectableText` builds the runs it selects from that
        // ambient style, so the size, the weight, the family and the ink arrive
        // without being restated. Restating them on the run would also move them
        // out of this module, which `tests/design_system.rs` forbids.
        if let Some(id) = self.selectable {
            let mut run = SelectableText::new(id, self.text).document_order(self.selection_order);
            if let Some(color) = self.selection_color {
                run = run.selection_color(color);
            }
            return element.child(run);
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

            /// Joins the window's text selection under a stable element id.
            ///
            /// The id must be unique per rendered element and stable across
            /// frames: the run keeps its selection handle under it, so a
            /// changing id drops the selection the reader is holding.
            #[must_use]
            pub fn selectable(self, id: impl Into<ElementId>) -> Self {
                Self(self.0.selectable(id))
            }

            /// Places this run in reading order among the runs of one document.
            /// Defaults to `0`.
            #[must_use]
            pub fn selection_order(self, order: u64) -> Self {
                Self(self.0.selection_order(order))
            }

            /// Overrides the selection highlight. Defaults to the theme's own
            /// selection role.
            #[must_use]
            pub fn selection_color(self, color: Hsla) -> Self {
                Self(self.0.selection_color(color))
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

    /// Selection is opt-in per role. A role that never asks for it stays out of
    /// the window's text selection, which is what keeps a drag on a button a
    /// click and the whole widget one selectable surface: the transcript.
    #[test]
    fn only_the_roles_asked_to_be_selectable_join_the_selection() {
        assert!(Title::new("t").0.selectable.is_none());
        assert!(Body::new("b").0.selectable.is_none());
        assert!(Label::new("l").0.selectable.is_none());

        let id = ElementId::from("caption-live");
        assert_eq!(
            Label::new("l").selectable(id.clone()).0.selectable,
            Some(id)
        );
    }

    /// The document order is the caller's argument and defaults to the first
    /// run, so a single selectable label needs no order at all.
    #[test]
    fn the_selection_order_defaults_to_the_first_run() {
        assert_eq!(Label::new("l").0.selection_order, 0);
        assert_eq!(
            Label::new("l")
                .selectable("caption-live")
                .selection_order(1)
                .0
                .selection_order,
            1
        );
        // Recorded even before a selection is asked for: the builder chain has
        // no required order.
        assert_eq!(Body::new("b").selection_order(7).0.selection_order, 7);
    }

    /// Selecting a role is not a fourth role: the copy keeps the size, weight
    /// and ink of the role it was asked for.
    #[test]
    fn the_selection_builders_do_not_change_the_role() {
        let title = Title::new("t").selectable("title").selection_order(2);
        let body = Body::new("b").selectable("body").selection_order(2);
        let label = Label::new("l").selectable("label").selection_order(2);

        for (role, text) in [
            (TextRole::Title, title.0),
            (TextRole::Body, body.0),
            (TextRole::Label, label.0),
        ] {
            assert_eq!(text.role, role);
            assert!(text.selectable.is_some(), "{role:?}");
            assert_eq!(text.selection_order, 2, "{role:?}");
        }
    }

    /// The highlight is opt-in as well: without a colour a selectable run keeps
    /// the theme's own selection role.
    #[test]
    fn the_highlight_colour_is_recorded_only_when_given() {
        assert!(Label::new("l").0.selection_color.is_none());

        let highlight: Hsla = gpui_kit::rgb(crate::tokens::color::BRAND_PRIMARY)
            .alpha(0.3)
            .into();
        assert_eq!(
            Label::new("l")
                .selectable("caption-committed")
                .selection_color(highlight)
                .0
                .selection_color,
            Some(highlight)
        );
    }
}
