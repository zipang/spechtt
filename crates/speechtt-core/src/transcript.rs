//! Streaming transcript reducer: committed clauses plus the live tail.
//!
//! `CaptionWell` renders two visually distinct parts: finalized text
//! ([`TranscriptBuffer::committed`], muted ink) and the words currently being
//! spoken ([`TranscriptBuffer::live`], brand accent). This type is the single
//! source of truth for both, and it is a pure reducer over [`SttEvent`]s so it
//! can be unit-tested without a window, a backend or a clock.
//!
//! A text event may name the utterance it belongs to (`segment`). Revisions of
//! one utterance replace the live tail; the first event of a *new* utterance
//! settles the previous tail into `committed` first, so a boundary neither
//! drops words nor duplicates them.

use crate::protocol::SttEvent;

/// Committed transcript text plus the in-progress live tail.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TranscriptBuffer {
    committed: String,
    live: String,
    /// Identity of the utterance the live tail belongs to, when the backend
    /// publishes one. A tail with no identity is replaced by any revision, as
    /// a backend that sends no `segment` behaves like a single continuous
    /// hypothesis.
    live_segment: Option<String>,
}

impl TranscriptBuffer {
    /// Fold one backend event into the buffer. Events that carry no transcript
    /// text (`ready`, `started`, `error`) leave both parts untouched.
    pub fn apply(&mut self, event: &SttEvent) {
        match event {
            SttEvent::Partial { text, segment, .. } => {
                let text = text.trim();
                if text == self.live.trim() {
                    // The same hypothesis re-sent: nothing to settle and nothing
                    // to replace, only its identity may have moved.
                    if let Some(segment) = segment {
                        self.live_segment = Some(segment.clone());
                    }
                    return;
                }
                if !self.revises_the_tail(text) {
                    // A provider may transcribe window by window and restart its
                    // hypothesis for each one. The words of the previous window
                    // are real text, so they are settled before the new window
                    // replaces the tail: a take never loses what was already
                    // transcribed.
                    self.settle_live();
                }
                self.begin_utterance(segment.as_deref());
                self.live = text.to_string();
            }
            SttEvent::Delta { delta, segment, .. } => {
                self.begin_utterance(segment.as_deref());
                self.push_delta(delta);
            }
            SttEvent::Committed { text, segment, .. } => {
                // A final names the utterance it closes. A tail of another
                // utterance is real text the backend already produced: settle
                // it before the final lands, so it is neither dropped nor
                // repeated. A tail with no identity cannot be matched to the
                // final, so it is settled too: a visible duplicate is
                // recoverable, a silently dropped utterance is not. Only a
                // final for the tail's own utterance, or one without an
                // identity, supersedes the hypothesis.
                let supersedes = match (self.live_segment.as_deref(), segment.as_deref()) {
                    (Some(live), Some(segment)) => live == segment,
                    (None, Some(_)) => false,
                    (_, None) => true,
                };
                if !supersedes {
                    self.settle_live();
                }
                let text = text.trim();
                if !text.is_empty() {
                    self.push_committed(text);
                }
                self.live.clear();
                self.live_segment = None;
            }
            SttEvent::Stopped { full_text, .. } => {
                let full_text = full_text.trim();
                if full_text.is_empty() {
                    // Final flush with no authoritative text: promote the tail.
                    self.settle_live();
                } else if self.authoritative_text_is_a_fragment(full_text) {
                    // The backend's final text is only a piece of what it has
                    // already produced — a provider that flush an early stream
                    // sends the tail it has, not the take. Adopting it would
                    // erase the words already on screen, so the take is kept and
                    // the tail promoted instead.
                    self.settle_live();
                } else {
                    self.committed = full_text.to_string();
                    self.live.clear();
                    self.live_segment = None;
                }
            }
            SttEvent::Ready
            | SttEvent::Started { .. }
            | SttEvent::Progress { .. }
            | SttEvent::Error { .. }
            | SttEvent::Cleaned { .. }
            | SttEvent::Capabilities { .. } => {}
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
        self.live_segment = None;
    }

    fn push_committed(&mut self, text: &str) {
        if !self.committed.is_empty() && !self.committed.ends_with(char::is_whitespace) {
            self.committed.push(' ');
        }
        self.committed.push_str(text);
    }

    /// Folds one delta into the live tail.
    ///
    /// A backend may stream incremental tokens, or a cumulative hypothesis that
    /// repeats the tail plus what it added, or a duplicate of what it just sent.
    /// The delta is classified against the tail at character boundaries, in
    /// this order:
    ///
    /// - whitespace-only: appended as the separator between two words when a
    ///   tail is already there and does not end with whitespace yet, and
    ///   ignored otherwise, so a take never opens with a blank;
    /// - the delta starts with the whole tail and is longer: cumulative, so it
    ///   replaces the tail. Word boundaries play no part — a provider that
    ///   extends the tail inside a word is still resending it, and appending
    ///   would duplicate every character it repeats;
    /// - the delta is exactly the tail: a duplicate, so it is ignored;
    /// - anything else: new text, so it is appended — including a shorter word
    ///   the tail already ends with. A repeated word is more plausible than a
    ///   repeated hypothesis, and dropping it would lose words the backend
    ///   produced.
    ///
    /// An empty tail takes the delta as is. Every comparison takes place on
    /// char boundaries, so multi-byte text is never split.
    fn push_delta(&mut self, delta: &str) {
        if delta.trim().is_empty() {
            // A provider may stream the separator between two words as a delta
            // of its own: dropping it would glue the words together, and a blank
            // with nothing to separate would open the tail with a space.
            if !self.live.is_empty() && !self.live.ends_with(char::is_whitespace) {
                self.live.push_str(delta);
            }
            return;
        }

        let tail = self.live.as_str();
        if tail.is_empty() {
            self.live.push_str(delta);
            return;
        }

        if let Some(rest) = delta.strip_prefix(tail) {
            if !rest.is_empty() {
                self.live.clear();
                self.live.push_str(delta);
            }
            return;
        }

        self.live.push_str(delta);
    }

    /// Whether `text` revises the live tail instead of restarting the window.
    ///
    /// A window hypothesis grows by prefix and may shrink back when the
    /// provider re-punctuates it, so an extension in either direction is the
    /// same window. A fragment that only matches the opening of a much longer
    /// tail is not a revision: it is the first words of the next window, and
    /// replacing the tail with it would drop everything it does not repeat.
    #[must_use]
    fn revises_the_tail(&self, text: &str) -> bool {
        let tail = self.live.trim();
        if tail.is_empty() {
            return true;
        }
        if text.starts_with(tail) {
            return true;
        }
        if !tail.starts_with(text) {
            return false;
        }
        text.chars().count() * 2 >= tail.chars().count()
    }

    /// Whether a `stopped` text is only a piece of the take already assembled.
    ///
    /// A provider may close a stream early and report the last window it
    /// transcribed as its final text. That text is a strict substring of what is
    /// already on screen, and adopting it would delete the words before it. A
    /// genuinely authoritative final is the whole take: it is longer than the
    /// assembled text, or it is a rewrite that no longer contains it.
    #[must_use]
    fn authoritative_text_is_a_fragment(&self, full_text: &str) -> bool {
        let assembled = self.full_text();
        let assembled = assembled.trim();
        let shorter = full_text.chars().count() < assembled.chars().count();
        shorter && assembled.contains(full_text)
    }

    /// Records the identity of a text event's utterance, settling the tail
    /// first when the event belongs to a different one. An event without an
    /// identity leaves the tracking untouched.
    fn begin_utterance(&mut self, segment: Option<&str>) {
        let Some(segment) = segment else {
            return;
        };
        if self
            .live_segment
            .as_deref()
            .is_some_and(|live| live != segment)
        {
            self.settle_live();
        }
        self.live_segment = Some(segment.to_string());
    }

    /// Moves the live tail into `committed` and forgets its utterance.
    ///
    /// A whitespace-only tail settles nothing: it must not leave a stray
    /// separator in the committed text.
    fn settle_live(&mut self) {
        let live = std::mem::take(&mut self.live);
        let live = live.trim();
        if !live.is_empty() {
            self.push_committed(live);
        }
        self.live_segment = None;
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
            segment: None,
        }
    }

    fn delta(session: &str, text: &str) -> SttEvent {
        SttEvent::Delta {
            session_id: session.into(),
            delta: text.into(),
            segment: None,
        }
    }

    fn committed(session: &str, text: &str) -> SttEvent {
        SttEvent::Committed {
            session_id: session.into(),
            text: text.into(),
            segment: None,
        }
    }

    fn segment_partial(session: &str, segment: &str, text: &str) -> SttEvent {
        SttEvent::Partial {
            session_id: session.into(),
            text: text.into(),
            segment: Some(segment.into()),
        }
    }

    fn segment_delta(session: &str, segment: &str, text: &str) -> SttEvent {
        SttEvent::Delta {
            session_id: session.into(),
            delta: text.into(),
            segment: Some(segment.into()),
        }
    }

    fn segment_committed(session: &str, segment: &str, text: &str) -> SttEvent {
        SttEvent::Committed {
            session_id: session.into(),
            text: text.into(),
            segment: Some(segment.into()),
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
            request_id: None,
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

    // ----- Delta classification ---------------------------------------

    #[test]
    fn a_partial_followed_by_the_same_delta_keeps_one_copy() {
        let mut buffer = TranscriptBuffer::default();
        let text = "d'une transcription quelconque";

        buffer.apply(&segment_partial("take-1", "seg-1", text));
        buffer.apply(&segment_delta("take-1", "seg-1", text));

        assert_eq!(buffer.live(), text);
        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.full_text(), text);
    }

    #[test]
    fn a_cumulative_delta_replaces_the_tail() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_delta("take-1", "seg-1", "d'une"));
        buffer.apply(&segment_delta("take-1", "seg-1", "d'une transcription"));

        assert_eq!(buffer.live(), "d'une transcription");
    }

    #[test]
    fn an_incremental_delta_still_appends() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_delta("take-1", "seg-1", "d'une"));
        buffer.apply(&segment_delta("take-1", "seg-1", " transcription"));

        assert_eq!(buffer.live(), "d'une transcription");
    }

    /// A provider that streams the space between two words as its own delta
    /// must still produce two words, not one glued word.
    #[test]
    fn a_whitespace_delta_keeps_the_separator_between_two_words() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_delta("take-1", "seg-1", "le"));
        buffer.apply(&segment_delta("take-1", "seg-1", " "));
        buffer.apply(&segment_delta("take-1", "seg-1", "monde"));

        assert_eq!(buffer.live(), "le monde");
        assert_eq!(buffer.full_text(), "le monde");
    }

    #[test]
    fn a_whitespace_delta_never_opens_the_tail_with_a_blank() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&delta("take-1", " "));
        buffer.apply(&delta("take-1", "   "));
        assert_eq!(buffer.live(), "");

        // A separator already at the end of the tail is not doubled.
        buffer.apply(&delta("take-1", "le"));
        buffer.apply(&delta("take-1", " "));
        buffer.apply(&delta("take-1", " "));
        assert_eq!(buffer.live(), "le ");
    }

    #[test]
    fn a_delta_already_at_the_end_of_the_tail_is_ignored() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_delta("take-1", "seg-1", "oui."));
        buffer.apply(&segment_delta("take-1", "seg-1", "oui."));
        buffer.apply(&segment_delta("take-1", "seg-1", "oui."));

        assert_eq!(buffer.live(), "oui.");
    }

    #[test]
    fn a_partial_then_a_delta_that_adds_words_keeps_the_new_words() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial("take-1", "seg-1", "d'une transcription"));
        buffer.apply(&segment_delta("take-1", "seg-1", " quelconque"));

        assert_eq!(buffer.live(), "d'une transcription quelconque");
    }

    /// A word the backend really says twice is not a duplicate: the delta is
    /// kept, so the tail holds three occurrences.
    #[test]
    fn a_repeated_word_is_not_swallowed_as_a_duplicate() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_delta("take-1", "seg-1", "très"));
        buffer.apply(&segment_delta("take-1", "seg-1", " très"));
        buffer.apply(&segment_delta("take-1", "seg-1", " très"));

        assert_eq!(buffer.live(), "très très très");
    }

    /// A delta that repeats a whole word already at the end of the tail is new
    /// text, not a duplicate. The reducer appends deltas verbatim, so the word
    /// lands without a separator; the invariant that matters is that no word
    /// the backend produced is dropped.
    #[test]
    fn a_delta_that_repeats_a_whole_word_is_kept() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_delta("take-1", "seg-1", "le chat"));
        buffer.apply(&segment_delta("take-1", "seg-1", "chat"));

        assert_eq!(buffer.live(), "le chatchat");
        assert_eq!(
            buffer.live().matches("chat").count(),
            2,
            "both occurrences of the word survive"
        );
    }

    #[test]
    fn a_new_segment_still_settles_the_previous_tail() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_delta("take-1", "seg-1", "hello"));
        // The same words in a new utterance are new text, not a duplicate: the
        // settle runs before the delta is classified against the tail.
        buffer.apply(&segment_delta("take-1", "seg-2", "hello"));

        assert_eq!(buffer.committed(), "hello");
        assert_eq!(buffer.live(), "hello");
        assert_eq!(buffer.full_text(), "hello hello");
    }

    // ----- Utterance identity (`segment`) ------------------------------

    #[test]
    fn a_new_segment_settles_the_previous_hypothesis() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial("take-1", "seg-1", "hello"));
        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.live(), "hello");

        buffer.apply(&segment_partial("take-1", "seg-2", "world"));
        assert_eq!(buffer.committed(), "hello");
        assert_eq!(buffer.live(), "world");
    }

    #[test]
    fn a_revision_inside_one_segment_replaces_only_the_tail() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial("take-1", "seg-1", "hel"));
        buffer.apply(&segment_partial("take-1", "seg-1", "hello"));
        buffer.apply(&segment_delta("take-1", "seg-1", " there"));

        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.live(), "hello there");
    }

    #[test]
    fn a_delta_of_a_new_segment_settles_the_previous_tail() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_delta("take-1", "seg-1", "hello"));
        buffer.apply(&segment_delta("take-1", "seg-2", "world"));

        assert_eq!(buffer.committed(), "hello");
        assert_eq!(buffer.live(), "world");
        assert_eq!(buffer.full_text(), "hello world");
    }

    #[test]
    fn a_final_for_the_live_segment_supersedes_it_without_duplication() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial("take-1", "seg-1", "hello"));
        buffer.apply(&segment_committed("take-1", "seg-1", "Hello."));

        assert_eq!(buffer.committed(), "Hello.");
        assert_eq!(buffer.live(), "");
        assert_eq!(buffer.full_text(), "Hello.");
    }

    #[test]
    fn a_final_without_a_segment_supersedes_the_live_tail() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial("take-1", "seg-1", "hello"));
        buffer.apply(&committed("take-1", "Hello."));

        assert_eq!(buffer.committed(), "Hello.");
        assert_eq!(buffer.live(), "");
    }

    /// A final that names its utterance cannot match a tail with no identity.
    /// The tail is real text, so it is settled before the final lands instead
    /// of being replaced: a visible duplicate is recoverable, a dropped
    /// utterance is not.
    #[test]
    fn a_final_naming_a_segment_settles_an_unidentified_tail() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&partial("take-1", "hello"));
        buffer.apply(&segment_committed("take-1", "seg-1", "Hello."));

        assert_eq!(buffer.committed(), "hello Hello.");
        assert_eq!(buffer.live(), "");
        assert_eq!(buffer.full_text(), "hello Hello.");
    }

    #[test]
    fn a_final_for_the_next_segment_settles_an_unsettled_tail() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial("take-1", "seg-1", "hello"));
        buffer.apply(&segment_committed("take-1", "seg-2", "World."));

        assert_eq!(buffer.committed(), "hello World.");
        assert_eq!(buffer.live(), "");
        assert_eq!(buffer.full_text(), "hello World.");
    }

    #[test]
    fn a_partial_without_a_segment_keeps_replacing_the_tail() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&partial("take-1", "audio"));
        buffer.apply(&partial("take-1", "audio capture"));

        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.live(), "audio capture");
    }

    #[test]
    fn whitespace_only_text_settles_and_appends_nothing() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial("take-1", "seg-1", "hello"));
        buffer.apply(&segment_partial("take-1", "seg-2", "   "));
        assert_eq!(buffer.committed(), "hello");
        assert_eq!(buffer.live(), "");

        buffer.apply(&segment_committed("take-1", "seg-2", "  "));
        assert_eq!(buffer.committed(), "hello");
        assert_eq!(buffer.full_text(), "hello");

        // A whitespace-only tail is not committed as a stray separator.
        let mut tail_only = TranscriptBuffer::default();
        tail_only.apply(&delta("take-1", "   "));
        tail_only.apply(&segment_partial("take-1", "seg-2", "next"));
        assert_eq!(tail_only.committed(), "");
        assert_eq!(tail_only.live(), "next");
        assert_eq!(tail_only.full_text(), "next");
    }

    /// A provider that closes its stream early reports the last window it
    /// transcribed as the final text. That text is the tail of the take, not the
    /// take: adopting it erases everything before it, and the result screen then
    /// shows the loss for good.
    #[test]
    fn a_stopped_text_that_is_only_the_tail_does_not_erase_the_take() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial("take-1", "seg-1", "un deux trois quatre"));
        buffer.apply(&stopped("take-1", "trois quatre"));

        assert_eq!(buffer.committed(), "un deux trois quatre");
        assert_eq!(buffer.full_text(), "un deux trois quatre");
    }

    /// The same window re-sent under a new utterance identity is not a new
    /// utterance: settling it would append the tail a second time, which is how
    /// the first words of a sentence appeared twice.
    #[test]
    fn the_same_hypothesis_under_a_new_segment_is_not_settled_twice() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial("take-1", "seg-0", "Dans le cas"));
        buffer.apply(&segment_partial("take-1", "seg-1", "Dans le cas"));

        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.live(), "Dans le cas");
        assert_eq!(buffer.full_text(), "Dans le cas");
    }

    /// The live screen is not the only reader of the buffer: the result screen
    /// shows what was stored, so a window the provider restarted must be settled
    /// rather than dropped with the tail it replaced.
    #[test]
    fn a_restarted_window_settles_the_previous_one() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial("take-1", "seg-0", "Dans le cas"));
        buffer.apply(&segment_partial("take-1", "seg-0", "d'un blueprint"));

        assert_eq!(buffer.committed(), "Dans le cas");
        assert_eq!(buffer.live(), "d'un blueprint");
        assert_eq!(buffer.full_text(), "Dans le cas d'un blueprint");
    }

    /// A cumulative delta that extends the tail inside a word is still a
    /// cumulative delta. Appending it instead of replacing it duplicates every
    /// character the tail already held.
    #[test]
    fn a_cumulative_delta_that_extends_a_word_replaces_the_tail() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial("take-1", "seg-0", "un blueprint mesur"));
        buffer.apply(&segment_delta("take-1", "seg-0", "un blueprint mesuré"));

        assert_eq!(buffer.live(), "un blueprint mesuré");
        assert_eq!(buffer.full_text(), "un blueprint mesuré");
    }

    /// A provider re-punctuating its window may resend a shorter draft of it.
    /// That is a revision of the same window, not a new one: settling it would
    /// commit text the smaller draft still carries.
    #[test]
    fn a_shorter_draft_of_the_same_window_replaces_the_tail() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial(
            "take-1",
            "seg-0",
            "le workflow va consister à créer un blueprint",
        ));
        buffer.apply(&segment_partial(
            "take-1",
            "seg-0",
            "le workflow va consister à créer un",
        ));

        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.live(), "le workflow va consister à créer un");
    }

    /// A short fragment that happens to repeat the opening of a long tail is
    /// the first words of the next window, not a revision of the last one.
    #[test]
    fn a_short_fragment_matching_the_opening_starts_a_new_window() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial(
            "take-1",
            "seg-0",
            "Et ensuite le workflow va consister",
        ));
        buffer.apply(&segment_partial("take-1", "seg-0", "Et"));

        assert_eq!(buffer.committed(), "Et ensuite le workflow va consister");
        assert_eq!(buffer.live(), "Et");
        assert_eq!(buffer.full_text(), "Et ensuite le workflow va consister Et");
    }

    /// The sequence a live provider produced: one utterance identity, a window
    /// that restarts for each portion of the take, then the whole take as the
    /// final text. No window may be lost on the way, and the final text wins.
    #[test]
    fn a_restarting_window_take_keeps_every_word_until_the_authoritative_final() {
        let mut buffer = TranscriptBuffer::default();
        let windows = [
            "Dans le cas d'un projet",
            "et ensuite le workflow consiste",
            "à créer un blueprint",
        ];
        for window in windows {
            buffer.apply(&segment_partial("take-1", "google-segment-0", window));
        }

        assert_eq!(
            buffer.committed(),
            "Dans le cas d'un projet et ensuite le workflow consiste"
        );
        assert_eq!(buffer.live(), "à créer un blueprint");

        let full = "Dans le cas d'un projet et ensuite le workflow consiste à créer un blueprint";
        buffer.apply(&stopped("take-1", full));

        assert_eq!(buffer.committed(), full);
        assert_eq!(buffer.live(), "");
        assert_eq!(buffer.full_text(), full);
    }

    #[test]
    fn committed_never_shrinks_and_keeps_every_word_across_four_segments() {
        let mut buffer = TranscriptBuffer::default();
        let events = [
            segment_partial("take-1", "seg-1", "one"),
            segment_delta("take-1", "seg-1", " two"),
            segment_partial("take-1", "seg-2", "three"),
            segment_committed("take-1", "seg-2", "three four."),
            segment_partial("take-1", "seg-3", "five"),
            segment_delta("take-1", "seg-3", " six"),
            segment_committed("take-1", "seg-4", "seven."),
        ];

        let mut previous = String::new();
        for event in &events {
            buffer.apply(event);
            let committed = buffer.committed().to_owned();
            assert!(
                committed.starts_with(&previous),
                "committed shrank after {event:?}: {previous:?} -> {committed:?}"
            );
            previous = committed;
        }

        assert_eq!(buffer.committed(), "one two three four. five six seven.");
        assert_eq!(buffer.live(), "");
        assert_eq!(
            buffer.full_text().split_whitespace().collect::<Vec<_>>(),
            ["one", "two", "three", "four.", "five", "six", "seven."]
        );
    }
}
