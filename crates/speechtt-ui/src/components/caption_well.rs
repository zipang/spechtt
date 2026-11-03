//! The rounded caption card: committed text in muted ink, the live words in
//! brand accent (the `audio capture layer` line in the mockups).
//!
//! Both runs are selectable, and they are one document: the pointer drags across
//! the transcript, the range is highlighted behind the glyphs, and the selected
//! range leaves the widget two ways — the application's copy shortcut
//! (`Ctrl-C`, `Cmd-C`) and the transcript's context menu, whose `Copy` item
//! carries the same action. Paste happens in whatever application the user
//! pastes into.
//!
//! # Who renders this
//!
//! `views::recording` (the live take) and `views::session_end` (the finished
//! take), both inside `layout::SurfaceBox`.

use gpui_kit::base::TextSelection;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::prelude::*;
use gpui_kit::{App, Hsla, IntoElement, RenderOnce, SharedString, Window, div};

use crate::app::CopySelection;
use crate::components::Label;
use crate::layout::SurfaceBox;
use crate::tokens::color;

/// Element id of the committed run in the window's text selection.
///
/// It names an element, not a piece of text: the run keeps its selection handle
/// in the element state under this id, so an id derived from the words would
/// recreate the handle on the next partial event and drop the selection the
/// reader is holding.
const COMMITTED_RUN_ID: &str = "caption-committed";

/// Element id of the live tail run, with the same stability requirement.
const LIVE_RUN_ID: &str = "caption-live";

/// Position of the committed run in the transcript document.
const COMMITTED_ORDER: u64 = 0;

/// Position of the live tail in the transcript document.
///
/// The live tail follows the committed text in the document and sits below it on
/// screen, so a drag across the boundary copies the finalized words first.
const LIVE_ORDER: u64 = 1;

/// Opacity of the selection highlight over the transcript.
const SELECTION_ALPHA: f32 = 0.30;

/// The highlight painted behind a selected range of the transcript.
///
/// `brand.primary` — the role mirrored by
/// [`crate::tokens::color::BRAND_PRIMARY`] and documented as the selection
/// highlight in `DESIGN.md` §Transcript well — carrying the approved 30 %
/// opacity. The alpha is what keeps both inks readable over it: the muted grey
/// of the committed text and the brand accent of the live tail stay legible
/// through the wash. No token is introduced and no other component uses it.
#[must_use]
pub fn selection_highlight() -> Hsla {
    gpui_kit::rgb(color::BRAND_PRIMARY)
        .alpha(SELECTION_ALPHA)
        .into()
}

/// Transcript card: committed text, then the live words.
///
/// The two runs stay separate elements because they carry different inks —
/// muted for what is finalized, brand accent for what is still being heard. They
/// are one selectable document through their document order, so a drag that
/// crosses from one into the other copies as one string, committed first.
///
/// # Copying the selection
///
/// The well offers `Copy` twice: the application shortcut handled by the window
/// view, and a context menu attached to the card with a single item. Both carry
/// [`CopySelection`], so they cannot drift apart — the menu item shows the
/// shortcut's keybinding because the menu resolves it from the action, and one
/// handler writes the clipboard for either path.
///
/// The menu exists because a shortcut-only copy is a single point of failure: if
/// the keystroke is intercepted before the window sees it, the user still has a
/// pointer route to the clipboard. It holds `Copy` and nothing else — SpeechTT
/// has no paste path of its own, and the transcript is not an editable field, so
/// neither `Paste` nor `Select All` belongs here. The item is disabled while
/// [`TextSelection::has_selection`] is `false`, so the menu never offers a copy
/// that would leave the clipboard unchanged.
///
/// The menu is the library's own component: its colours, radii, spacing and
/// elevation come from the theme, and this file makes no visual decision about
/// it.
#[derive(IntoElement)]
pub struct CaptionWell {
    committed: SharedString,
    live: SharedString,
}

impl CaptionWell {
    /// A top-anchored well: the reader starts at the beginning of the text.
    pub fn new(committed: impl Into<SharedString>, live: impl Into<SharedString>) -> Self {
        Self {
            committed: committed.into(),
            live: live.into(),
        }
    }
}

impl RenderOnce for CaptionWell {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let highlight = selection_highlight();

        // The right-click route to the clipboard. The menu is attached to the
        // card's wrapper rather than to the card itself: a context menu needs a
        // styled, parent element, which the wrapper is and the themed surface is
        // not. The enabled state is read when the menu is built, which is the
        // only moment it is shown.
        div()
            .flex_1()
            .flex()
            .flex_col()
            .child(
                SurfaceBox::new()
                    .flex_1()
                    .clip()
                    .child(
                        Label::new(self.committed)
                            .mono()
                            .muted()
                            .selectable(COMMITTED_RUN_ID)
                            .selection_order(COMMITTED_ORDER)
                            .selection_color(highlight),
                    )
                    .child(
                        Label::new(self.live)
                            .mono()
                            .tone(gpui_kit::rgb(color::BRAND_ACCENT).into())
                            .selectable(LIVE_RUN_ID)
                            .selection_order(LIVE_ORDER)
                            .selection_color(highlight),
                    ),
            )
            .context_menu(|menu, window, cx| {
                menu.menu_with_disabled(
                    "Copy",
                    Box::new(CopySelection),
                    !TextSelection::has_selection(window, cx),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        COMMITTED_ORDER, COMMITTED_RUN_ID, LIVE_ORDER, LIVE_RUN_ID, SELECTION_ALPHA,
        selection_highlight,
    };
    use crate::tokens::color;
    use gpui_kit::Hsla;

    /// The highlight is the brand role at the approved opacity, not a new
    /// colour: same hue, saturation and lightness as
    /// `--color-brand-primary`, carrying only an alpha.
    #[test]
    fn the_selection_highlight_is_brand_primary_at_thirty_percent() {
        let highlight = selection_highlight();
        let brand: Hsla = gpui_kit::rgb(color::BRAND_PRIMARY).into();

        assert_eq!(highlight.h, brand.h, "same hue as the brand role");
        assert_eq!(highlight.s, brand.s, "same saturation as the brand role");
        assert_eq!(highlight.l, brand.l, "same lightness as the brand role");
        assert_eq!(
            highlight.a, SELECTION_ALPHA,
            "thinned to the approved opacity"
        );
        assert_eq!(
            SELECTION_ALPHA, 0.30,
            "the agreed value is 30 % — a different one is a design change"
        );
    }

    /// Two runs, one document: the finalized words are read first, so a copy
    /// that spans the boundary starts with them.
    #[test]
    fn the_committed_run_comes_before_the_live_run() {
        assert_eq!(COMMITTED_ORDER, 0);
        assert_eq!(LIVE_ORDER, 1);
        const { assert!(COMMITTED_ORDER < LIVE_ORDER) };
    }

    /// The runs keep separate identities. Sharing an element id would make the
    /// second run reuse the first run's selection handle, and the copy would
    /// return one run twice.
    #[test]
    fn the_two_runs_have_their_own_identities() {
        assert_ne!(COMMITTED_RUN_ID, LIVE_RUN_ID);
    }
}
