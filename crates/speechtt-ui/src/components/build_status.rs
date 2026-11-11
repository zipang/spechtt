//! The build line every dialog footer carries: the compiled version and the
//! gateway state.
//!
//! Rendered by `dialogs::help` and `dialogs::settings` as
//! [`crate::components::AppFooter::leading`]. A dialog window never puts a
//! connection badge beside a form row: the footer is the one place that reports
//! the build and the connection, so both windows say it identically and a
//! screen change cannot move it.

use gpui_kit::base::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{App, Hsla, IntoElement, RenderOnce, Window};

use crate::components::{Label, StatusDot};
use crate::controller::ConnectionState;
use crate::tokens::color;

/// Version and gateway state, as a dialog footer's leading slot draws them.
#[derive(IntoElement)]
pub struct BuildStatus {
    state: ConnectionState,
}

impl BuildStatus {
    /// Builds the line for a connection state.
    pub fn new(state: ConnectionState) -> Self {
        Self { state }
    }

    /// The version text: the value compiled into the binary, never a literal
    /// kept beside it.
    #[must_use]
    pub fn version_label() -> String {
        format!("SPEECHTT V{}", env!("CARGO_PKG_VERSION"))
    }

    /// What the line claims about the gateway for a connection state.
    ///
    /// The three states are distinct on purpose. `NOT VERIFIED` is not a
    /// failure: no catalogue has arrived, so nothing has been checked yet, and
    /// saying `NOT CONNECTED` there would report an outcome nobody observed.
    pub fn gateway_label(state: ConnectionState) -> &'static str {
        match state {
            ConnectionState::Connected => "GATEWAY CONNECTED",
            ConnectionState::NotConnected => "GATEWAY NOT CONNECTED",
            ConnectionState::Unknown => "GATEWAY NOT VERIFIED",
        }
    }

    /// The role colour of the dot and the gateway label.
    fn role_color(state: ConnectionState) -> Hsla {
        match state {
            ConnectionState::Connected => gpui_kit::rgb(color::SUCCESS).into(),
            ConnectionState::NotConnected => gpui_kit::rgb(color::DANGER).into(),
            ConnectionState::Unknown => gpui_kit::rgb(color::WARNING).into(),
        }
    }
}

impl RenderOnce for BuildStatus {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let tone = Self::role_color(self.state);
        let label = Self::gateway_label(self.state);
        h_flex()
            .gap_2()
            .items_center()
            .child(Label::new(Self::version_label()).mono().muted())
            .child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(StatusDot::new(tone))
                    .child(Label::new(label).bold().tone(tone)),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The version comes from the build, so a footer cannot advertise a number
    /// the binary does not report.
    #[test]
    fn the_version_is_the_compiled_one() {
        assert_eq!(
            BuildStatus::version_label(),
            format!("SPEECHTT V{}", env!("CARGO_PKG_VERSION"))
        );
    }

    /// An unverified gateway never reads as a failure: the catalogue has not
    /// spoken yet, so the line reports that nothing was checked.
    #[test]
    fn an_unchecked_gateway_is_not_reported_as_disconnected() {
        assert_eq!(
            BuildStatus::gateway_label(ConnectionState::Unknown),
            "GATEWAY NOT VERIFIED"
        );
        assert_eq!(
            BuildStatus::gateway_label(ConnectionState::Connected),
            "GATEWAY CONNECTED"
        );
        assert_eq!(
            BuildStatus::gateway_label(ConnectionState::NotConnected),
            "GATEWAY NOT CONNECTED"
        );
        assert_ne!(
            BuildStatus::gateway_label(ConnectionState::Unknown),
            BuildStatus::gateway_label(ConnectionState::NotConnected)
        );
    }
}
