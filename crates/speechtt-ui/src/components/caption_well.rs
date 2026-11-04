//! The rounded caption card: committed text in muted ink, the live words in
//! brand accent (the `audio capture layer` line in the mockups).
//!
//! The text sits in a vertical scroll area that fills the panel, which itself
//! keeps the height the screen gives it: the visible window is the
//! screen-adapted one, and the full transcript stays in the buffer for
//! copy/save. There is no scrollbar — the wheel and the keyboard reach the
//! earlier words.
//!
//! A live take passes a [`gpui_kit::ScrollHandle`] through [`CaptionWell::follow`]
//! to keep the newest line in view while the reader is at the bottom.
//!
//! Both runs are selectable, and they are one document: the pointer drags across
//! the transcript, the range is highlighted behind the glyphs, and the selected
//! range leaves the widget two ways — the application's copy shortcut
//! (`Ctrl-C`, `Cmd-C`) and the transcript's context menu, whose `Copy` item
//! carries the same action. The menu dispatches that action to the window's
//! focused handle, which is the widget root: the same element the shortcut
//! reaches. Paste happens in whatever application the user pastes into.
//!
//! # Who renders this
//!
//! `views::recording` (the live take, following the newest line) and
//! `views::session_end` (the finished take, anchored at the start), both inside
//! `layout::SurfaceBox`.

use gpui_kit::base::{TextSelection, v_flex};
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::prelude::*;
use gpui_kit::{App, Hsla, IntoElement, RenderOnce, ScrollHandle, SharedString, Window, div};

use crate::app::CopySelection;
use crate::components::Label;
use crate::layout::SurfaceBox;
use crate::tokens::color;

/// How close to the scroll end, in logical pixels, still counts as "at the
/// bottom". Absorbs the sub-pixel difference between the laid-out content and
/// the reported scroll range.
pub const FOLLOW_EPSILON: f32 = 0.5;

/// Stable identity of the live take's scroll area, so its offset survives a
/// repaint.
///
/// It differs from [`RESULT_SCROLL_ID`] because element state is keyed by id: a
/// shared id would let the finished take open at the live take's bottom offset.
const LIVE_SCROLL_ID: &str = "caption-well-live-text";

/// Stable identity of the finished take's scroll area, which stays top-anchored
/// and must not inherit the live take's element state.
const RESULT_SCROLL_ID: &str = "caption-well-result-text";

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

/// Whether a scroll view is at its end, and should therefore follow new text.
///
/// GPUI scroll offsets are non-positive and `max_offset_y` is the positive
/// scroll range, so the bottom is reached when the whole range is consumed. A
/// view that cannot scroll (`(0.0, 0.0)`) counts as at the bottom, which keeps
/// a short transcript following until it overflows.
#[must_use]
pub fn follows_bottom(offset_y: f32, max_offset_y: f32) -> bool {
    -offset_y >= max_offset_y - FOLLOW_EPSILON
}

/// Whether the transcript's `Copy` item must be offered as disabled.
///
/// The entry is disabled while nothing is selected: the menu never offers a
/// `Copy` that would leave the clipboard unchanged. The second state that makes
/// it unusable is a menu with no focus handle to dispatch the action to
/// (`has_target` is `false`). Neither state is a visible one — the right-click
/// that opens the menu focuses the widget root — but the menu is built from
/// whatever the window reports, so the item follows it.
fn copy_item_disabled(has_selection: bool, has_target: bool) -> bool {
    !(has_selection && has_target)
}

/// Scrollable transcript card: committed text, then the live words.
///
/// The two runs stay separate elements because they carry different inks —
/// muted for what is finalized, brand accent for what is still being heard. They
/// are one selectable document through their document order, so a drag that
/// crosses from one into the other copies as one string, committed first.
///
/// The card fills the space its screen gives it and scrolls; it never truncates
/// the transcript.
///
/// # Copying the selection
///
/// The well offers `Copy` twice: the application shortcut handled by the window
/// view, and a context menu attached to the card's outer envelope with a single
/// item. Both carry [`CopySelection`], so they cannot drift apart — the menu item
/// shows the shortcut's keybinding because the menu resolves it from the action,
/// and one handler writes the clipboard for either path.
///
/// The menu hangs off the envelope that fills the card, outside the
/// [`SurfaceBox`] padding, so a right-click anywhere over the painted card —
/// including its inner margin — opens it. The scroll column sits inside the
/// envelope and keeps its own offset; the menu never wraps the scroll state.
///
/// A menu action is not sent to the element that was right-clicked: the menu
/// returns focus to the handle it was given and dispatches the action there. The
/// handle passed is the window's focused one, which is the widget root carrying
/// the `SpeechTT` key context and the copy handler, so the item and the shortcut
/// resolve to the same element by construction rather than by a second pairing.
///
/// The item is disabled when there is no selection — the menu never offers a copy
/// that would leave the clipboard unchanged — and equally when the window reports
/// no focused handle, because an action with no dispatch target would close the
/// menu and write nothing. Neither state is a visible one: the right-click that
/// opens the menu focuses the root.
///
/// The menu exists because a shortcut-only copy is a single point of failure: if
/// the keystroke is intercepted before the window sees it, the user still has a
/// pointer route to the clipboard. It holds `Copy` and nothing else — SpeechTT
/// has no paste path of its own, and the transcript is not an editable field, so
/// neither `Paste` nor `Select All` belongs here.
///
/// The menu is the library's own component: its colours, radii, spacing and
/// elevation come from the theme, and this file makes no visual decision about
/// it.
#[derive(IntoElement)]
pub struct CaptionWell {
    committed: SharedString,
    live: SharedString,
    follow: Option<ScrollHandle>,
}

impl CaptionWell {
    /// A top-anchored well: the reader starts at the beginning of the text and
    /// scrolls for the rest. This is the result screen's behaviour.
    pub fn new(committed: impl Into<SharedString>, live: impl Into<SharedString>) -> Self {
        Self {
            committed: committed.into(),
            live: live.into(),
            follow: None,
        }
    }

    /// Keeps the newest line in view without fighting the reader: the well
    /// asks for the bottom on every frame only while the view is already
    /// there. Scrolling up stops following; scrolling back to the bottom
    /// resumes it. The selection never drives the offset, so a partial that
    /// arrives while the reader is scrolled up does not snap the view.
    ///
    /// The predicate reads the offset and the scroll range the handle stored
    /// during the previous frame's layout, so the request for the bottom is one
    /// frame behind the text that just arrived: a take that grows past the
    /// panel can hide the newest line for that frame.
    pub fn follow(mut self, handle: &ScrollHandle) -> Self {
        self.follow = Some(handle.clone());
        self
    }
}

impl RenderOnce for CaptionWell {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let scroll_id = if self.follow.is_some() {
            LIVE_SCROLL_ID
        } else {
            RESULT_SCROLL_ID
        };
        let mut text = v_flex()
            .id(scroll_id)
            .w_full()
            .flex_1()
            .min_h_0()
            .gap_1()
            .overflow_y_scroll();

        if let Some(handle) = &self.follow {
            if follows_bottom(
                f32::from(handle.offset().y),
                f32::from(handle.max_offset().y),
            ) {
                handle.scroll_to_bottom();
            }
            text = text.track_scroll(handle);
        }

        let highlight = selection_highlight();

        let column = text
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
            );

        // The right-click route to the clipboard, attached to the envelope that
        // fills the card and wraps the `SurfaceBox`, so a right-click anywhere
        // over the painted card — its inner padding included — opens it. The
        // enabled state is read when the menu is built, which is the only
        // moment it is shown.
        div()
            .flex_1()
            .flex()
            .flex_col()
            .min_h_0()
            .child(SurfaceBox::new().flex_1().child(column))
            .context_menu(|menu, window, cx| {
                let target = window.focused(cx);
                let disabled =
                    copy_item_disabled(TextSelection::has_selection(window, cx), target.is_some());
                let menu = match target {
                    Some(handle) => menu.action_context(handle),
                    None => menu,
                };

                menu.menu_with_disabled("Copy", Box::new(CopySelection), disabled)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        COMMITTED_ORDER, COMMITTED_RUN_ID, FOLLOW_EPSILON, LIVE_ORDER, LIVE_RUN_ID, LIVE_SCROLL_ID,
        RESULT_SCROLL_ID, SELECTION_ALPHA, copy_item_disabled, follows_bottom, selection_highlight,
    };
    use crate::tokens::color;
    use gpui_kit::Hsla;

    #[test]
    fn a_view_that_cannot_scroll_counts_as_the_bottom() {
        assert!(follows_bottom(0.0, 0.0));
    }

    #[test]
    fn a_view_at_the_scroll_end_follows() {
        assert!(follows_bottom(-300.0, 300.0));
        assert!(follows_bottom(-10.0, 10.0));
    }

    #[test]
    fn a_view_scrolled_away_from_the_end_stops_following() {
        assert!(!follows_bottom(0.0, 300.0));
        assert!(!follows_bottom(-100.0, 300.0));
        assert!(!follows_bottom(-299.0, 300.0));
    }

    #[test]
    fn the_tolerance_absorbs_sub_pixel_drift() {
        assert!(follows_bottom(-(300.0 - FOLLOW_EPSILON), 300.0));
        assert!(follows_bottom(-(300.0 - FOLLOW_EPSILON / 2.0), 300.0));
        assert!(!follows_bottom(-(300.0 - FOLLOW_EPSILON * 3.0), 300.0));
    }

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

    /// The live take and the finished take keep separate scroll state. The
    /// element state of a scroll area is keyed by its id, so one shared id
    /// would let the result screen open at the live take's bottom offset.
    #[test]
    fn the_live_and_result_screens_keep_separate_scroll_state() {
        assert_ne!(LIVE_SCROLL_ID, RESULT_SCROLL_ID);
    }

    /// A `Copy` entry is usable only when both halves are there: something is
    /// selected, and the menu has a handle to dispatch the action to. Either
    /// missing half leaves the clipboard untouched, so the entry says so.
    #[test]
    fn the_copy_entry_needs_a_selection_and_a_dispatch_target() {
        assert!(!copy_item_disabled(true, true), "both halves present");
        assert!(copy_item_disabled(false, true), "nothing selected");
        assert!(copy_item_disabled(true, false), "nowhere to dispatch");
        assert!(copy_item_disabled(false, false), "neither half");
    }
}
