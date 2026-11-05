//! Trailing header read-out for a take screen: the model, then the clock.
//!
//! Rendered by `views::recording` and `views::session_transcription` on the
//! header's trailing edge. The elapsed `mm:ss` is how long the take has been
//! running — its capture for a live take, its decode for a file take — and the
//! model sits to its left, because knowing which engine is listening changes what
//! the user expects from the text.
//!
//! [`TransportModel`] carries the three presentations: a picker while the session
//! can still change its model, a plain name when it cannot, and nothing when no
//! catalogue has arrived.

use gpui_kit::base::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{App, IntoElement, RenderOnce, SharedString, Window};

use crate::components::{ElapsedTimer, Label, LiveModelSelect};

/// How the trailing header edge presents the take's model.
///
/// The header never offers a control that cannot act: a frozen take or a
/// catalogue that has not arrived both read as text.
pub enum TransportModel {
    /// A live dropdown the user can change.
    Picker(LiveModelSelect),
    /// A read-only name: the take is active, or nothing can be picked.
    Label(SharedString),
    /// Nothing to show: no catalogue and no resolved model.
    Absent,
}

#[derive(IntoElement)]
pub struct TransportHeader {
    elapsed: u32,
    model: TransportModel,
}

impl TransportHeader {
    /// Trailing read-out: the model on the left, `mm:ss` on the right.
    pub fn new(elapsed: u32, model: TransportModel) -> Self {
        Self { elapsed, model }
    }
}

impl RenderOnce for TransportHeader {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut row = h_flex().gap_2().items_center();
        row = match self.model {
            TransportModel::Picker(select) => row.child(select),
            TransportModel::Label(name) => row.child(Label::new(name).muted()),
            TransportModel::Absent => row,
        };
        row.child(ElapsedTimer::new(self.elapsed))
    }
}
