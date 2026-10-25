//! Streaming transcript reducer: committed clauses plus the live tail.
//!
//! `CaptionWell` renders two visually distinct parts: finalized text
//! ([`TranscriptBuffer::committed`], muted ink) and the words currently being
//! spoken ([`TranscriptBuffer::live`], brand accent). This type is the single
//! source of truth for both, and it is a pure reducer over [`SttEvent`]s so it
//! can be unit-tested without a window, a backend or a clock.

use crate::protocol::SttEvent;

/// Committed transcript text plus the in-progress live tail.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TranscriptBuffer {
    committed: String,
    live: String,
}

impl TranscriptBuffer {
    /// Fold one backend event into the buffer. Events that carry no transcript
    /// text (`ready`, `started`, `error`) leave both parts untouched.
    pub fn apply(&mut self, event: &SttEvent) {
        match event {
            SttEvent::Partial { text, .. } => self.live = text.trim().to_string(),
            SttEvent::Delta { delta, .. } => self.live.push_str(delta),
            SttEvent::Committed { text, .. } => {
                let text = text.trim();
                if !text.is_empty() {
                    self.push_committed(text);
                }
                self.live.clear();
            }
            SttEvent::Stopped { full_text, .. } => {
                let full_text = full_text.trim();
                if full_text.is_empty() {
                    // Final flush with no authoritative text: promote the tail.
                    self.flush_live();
                } else {
                    self.committed = full_text.to_string();
                    self.live.clear();
                }
            }
            SttEvent::Ready | SttEvent::Started { .. } | SttEvent::Error { .. } => {}
        }
    }

    /// Finalized text. Never includes the live tail.
    #[must_use]
    pub fn committed(&self) -> &str {
        &self.committed
    }

    /// The in-progress hypothesis, empty when nothing is being spoken.
    #[must_use]
    pub fn live(&self) -> &str {
        &self.live
    }

    /// Committed and live text joined for copy/save actions.
    #[must_use]
    pub fn full_text(&self) -> String {
        match (self.committed.is_empty(), self.live.is_empty()) {
            (true, _) => self.live.clone(),
            (false, true) => self.committed.clone(),
            (false, false) => format!("{} {}", self.committed, self.live),
        }
    }

    /// Discard both parts, e.g. at the start of a new take.
    pub fn clear(&mut self) {
        self.committed.clear();
        self.live.clear();
    }

    fn push_committed(&mut self, text: &str) {
        if !self.committed.is_empty() && !self.committed.ends_with(char::is_whitespace) {
            self.committed.push(' ');
        }
        self.committed.push_str(text);
    }

    fn flush_live(&mut self) {
        if self.live.is_empty() {
            return;
        }
        let live = std::mem::take(&mut self.live);
        self.push_committed(live.trim());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::SttEvent;

    fn partial(session: &str, text: &str) -> SttEvent {
        SttEvent::Partial {
            session_id: session.into(),
            text: text.into(),
        }
    }

    fn delta(session: &str, text: &str) -> SttEvent {
        SttEvent::Delta {
            session_id: session.into(),
            delta: text.into(),
        }
    }

    fn committed(session: &str, text: &str) -> SttEvent {
        SttEvent::Committed {
            session_id: session.into(),
            text: text.into(),
        }
    }

    fn stopped(session: &str, full_text: &str) -> SttEvent {
        SttEvent::Stopped {
            session_id: session.into(),
            full_text: full_text.into(),
        }
    }

    #[test]
    fn streams_partial_delta_committed_then_stopped() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&partial("take-1", "audio"));
        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.live(), "audio");

        buffer.apply(&delta("take-1", " capture"));
        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.live(), "audio capture");

        buffer.apply(&delta("take-1", " layer"));
        buffer.apply(&committed("take-1", "audio capture layer."));
        assert_eq!(buffer.committed(), "audio capture layer.");
        assert_eq!(buffer.live(), "");

        buffer.apply(&partial("take-1", "next words"));
        assert_eq!(buffer.committed(), "audio capture layer.");
        assert_eq!(buffer.live(), "next words");

        // A final `stopped` with no new text flushes the live tail.
        buffer.apply(&stopped("take-1", ""));
        assert_eq!(buffer.committed(), "audio capture layer. next words");
        assert_eq!(buffer.live(), "");
    }

    #[test]
    fn partial_replaces_the_live_tail_and_trims_it() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&partial("take-1", "  audio cap  "));
        assert_eq!(buffer.live(), "audio cap");

        buffer.apply(&partial("take-1", "audio capture"));
        assert_eq!(buffer.live(), "audio capture");
    }

    #[test]
    fn deltas_append_to_the_live_tail() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&delta("take-1", "audio"));
        buffer.apply(&delta("take-1", " capture"));
        buffer.apply(&delta("take-1", " layer"));

        assert_eq!(buffer.live(), "audio capture layer");
        assert_eq!(buffer.committed(), "");
    }

    #[test]
    fn committed_segments_join_with_a_single_space_and_clear_the_tail() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&committed("take-1", "First sentence."));
        buffer.apply(&partial("take-1", "second"));
        buffer.apply(&committed("take-1", "Second sentence."));

        assert_eq!(buffer.committed(), "First sentence. Second sentence.");
        assert_eq!(buffer.live(), "");
    }

    #[test]
    fn committed_preserves_existing_trailing_whitespace() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&committed("take-1", "First."));
        buffer.apply(&committed("take-1", ""));

        assert_eq!(buffer.committed(), "First.");
        assert_eq!(buffer.full_text(), "First.");
    }

    #[test]
    fn stopped_with_full_text_replaces_the_whole_transcript() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&committed("take-1", "draft"));
        buffer.apply(&partial("take-1", "unfinished tail"));
        buffer.apply(&stopped("take-1", "The final transcript."));

        assert_eq!(buffer.committed(), "The final transcript.");
        assert_eq!(buffer.live(), "");
        assert_eq!(buffer.full_text(), "The final transcript.");
    }

    #[test]
    fn stopped_without_text_flushes_a_previous_committed_tail() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&committed("take-1", "Committed."));
        buffer.apply(&partial("take-1", " live tail "));
        buffer.apply(&stopped("take-1", ""));

        assert_eq!(buffer.committed(), "Committed. live tail");
        assert_eq!(buffer.full_text(), "Committed. live tail");
    }

    #[test]
    fn events_that_do_not_carry_text_leave_the_buffer_untouched() {
        let mut buffer = TranscriptBuffer::default();
        buffer.apply(&partial("take-1", "audio"));

        buffer.apply(&SttEvent::Ready);
        buffer.apply(&SttEvent::Started {
            session_id: "take-1".into(),
        });
        buffer.apply(&SttEvent::Error {
            session_id: Some("take-1".into()),
            code: "stream_failed".into(),
            message: "boom".into(),
        });

        assert_eq!(buffer.live(), "audio");
        assert_eq!(buffer.committed(), "");
    }

    #[test]
    fn full_text_combines_both_parts_without_stray_separators() {
        let mut buffer = TranscriptBuffer::default();
        assert_eq!(buffer.full_text(), "");

        buffer.apply(&partial("take-1", "live only"));
        assert_eq!(buffer.full_text(), "live only");

        buffer.apply(&committed("take-1", "committed"));
        buffer.apply(&partial("take-1", "live"));
        assert_eq!(buffer.full_text(), "committed live");
    }

    #[test]
    fn clear_resets_both_parts() {
        let mut buffer = TranscriptBuffer::default();
        buffer.apply(&committed("take-1", "committed"));
        buffer.apply(&partial("take-1", "live"));

        buffer.clear();

        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.live(), "");
        assert_eq!(buffer.full_text(), "");
        assert_eq!(buffer, TranscriptBuffer::default());
    }
}
