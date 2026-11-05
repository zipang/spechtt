//! Streaming transcript reducer: the committed text plus the live preview.
//!
//! `views::recording` renders the two parts through `components::CaptionWell`:
//! the committed text in muted ink and the words being spoken in brand accent,
//! both read from `SessionController::transcript_committed` and
//! `SessionController::transcript_live`. The copy, paste and save actions read
//! [`TranscriptBuffer::full_text`]. The buffer is a pure reducer over
//! [`SttEvent`]s, so it is unit-tested without a window, a backend or a clock.
//!
//! The worker publishes text on two channels that mean two different things, and
//! the buffer keeps them apart:
//!
//! - `Delta` is text the provider has **already committed**. It is appended
//!   verbatim, because a provider may split one word across two deltas
//!   (` Trans` + `cribe`).
//! - `Partial` is a **preview** of the utterance being spoken. It replaces the
//!   previous preview and is never promoted into the committed text: a provider
//!   slides its window, re-punctuates words it already showed, and may replace
//!   the preview wholesale after a language switch.
//!
//! Display reconciles the two through [`TranscriptBuffer::full_text`], which
//! hides a preview the committed text already ends with and removes a shared
//! word run at the joint, so a provider that reports the same text on both
//! channels is shown once.

use crate::protocol::SttEvent;

/// Committed transcript text plus the in-progress preview.
///
/// `committed` is what the provider committed — every delta, every `committed`
/// payload and a non-empty `stopped` text; `live` is the newest partial. The
/// two are rendered side by side by `components::CaptionWell` and joined by
/// [`TranscriptBuffer::full_text`] for the copy, paste and save actions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TranscriptBuffer {
    /// Text the provider has committed, in the order it published it.
    committed: String,
    /// The newest partial: a preview of the utterance being spoken, replaced by
    /// the next partial and never promoted into `committed`.
    live: String,
    /// Identity of the utterance the preview belongs to, recorded from the
    /// newest text event. The reducer does not gate the preview on it — every
    /// partial replaces the preview, whoever it belongs to — but it is part of
    /// the buffer's value, so previews of different utterances never compare
    /// equal and the UI can tell two takes apart.
    live_segment: Option<String>,
}

impl TranscriptBuffer {
    /// Folds one backend event into the buffer.
    ///
    /// Transcript-bearing events follow the channel split: `Delta` accumulates
    /// into the committed text, `Partial` replaces the preview, `Committed`
    /// seam-joins its text and clears the preview, and `Stopped` either replaces
    /// everything (non-empty text) or folds the preview in. Events that carry no
    /// transcript text (`ready`, `started`, `progress`, `error`, `cleaned`,
    /// `capabilities`) leave both parts untouched.
    pub fn apply(&mut self, event: &SttEvent) {
        match event {
            SttEvent::Partial { text, segment, .. } => self.apply_partial(text, segment.as_deref()),
            SttEvent::Delta { delta, .. } => self.apply_delta(delta),
            SttEvent::Committed { text, .. } => {
                self.committed = Self::seam_join(&self.committed, text);
                self.live.clear();
                self.live_segment = None;
            }
            SttEvent::Stopped { full_text, .. } => {
                let full_text = full_text.trim();
                if full_text.is_empty() {
                    // A take that died without an authoritative text: the
                    // preview is the only text there is, so it is folded in
                    // instead of blanking the transcript.
                    self.committed = Self::seam_join(&self.committed, &self.live);
                } else {
                    self.committed = full_text.to_string();
                }
                self.live.clear();
                self.live_segment = None;
            }
            SttEvent::Ready
            | SttEvent::Started { .. }
            | SttEvent::Progress { .. }
            | SttEvent::Error { .. }
            | SttEvent::Cleaned { .. }
            | SttEvent::Capabilities { .. } => {}
        }
    }

    /// Finalized text. Never includes the preview.
    #[must_use]
    pub fn committed(&self) -> &str {
        &self.committed
    }

    /// The in-progress preview, empty when nothing is being spoken.
    #[must_use]
    pub fn live(&self) -> &str {
        &self.live
    }

    /// Committed text and preview as one string, for the copy, paste and save
    /// actions.
    ///
    /// The two parts are reconciled by [`Self::seam_join`], so text a provider
    /// publishes on both channels is shown once.
    #[must_use]
    pub fn full_text(&self) -> String {
        Self::seam_join(&self.committed, &self.live)
    }

    /// Discard both parts, e.g. at the start of a new take.
    pub fn clear(&mut self) {
        self.committed.clear();
        self.live.clear();
        self.live_segment = None;
    }

    /// Folds one `Delta` into the committed text.
    ///
    /// A delta is text the provider has already committed, in one of three
    /// measured shapes: an increment (` Trans`, `cribe`), an increment that
    /// continues the word the previous delta ended with, or the whole take
    /// re-sent. It is therefore appended **verbatim** — nothing is inserted
    /// between two deltas, because a separator would turn a split word into two
    /// — and the whole-take shape replaces the committed text instead of
    /// doubling it. A delta the committed text already ends with is the same
    /// text re-sent and is ignored.
    ///
    /// Whitespace is the one thing that is never content: a separator-only delta
    /// keeps one separator between two words, and is dropped when there is
    /// nothing to separate, so a take never opens with a blank and a blank never
    /// doubles.
    fn apply_delta(&mut self, delta: &str) {
        let text = delta.trim_start();

        if text.is_empty() {
            if !self.committed.is_empty() && !self.committed.ends_with(char::is_whitespace) {
                self.committed.push_str(delta);
            }
            return;
        }

        if text.len() > self.committed.len() && text.starts_with(self.committed.as_str()) {
            self.committed = text.to_string();
        } else if !self.committed.ends_with(delta) {
            // A take opens on its first word, not on the separator in front of
            // it; between two deltas the provider's own separator is the text.
            let addition = if self.committed.is_empty() {
                text
            } else {
                delta
            };
            self.committed.push_str(addition);
        }
    }

    /// Replaces the preview with one `Partial`.
    ///
    /// A partial is a preview of the window being spoken, so every shape a
    /// provider emits for it is the same thing here: the current best text for
    /// right now. It may grow, shrink, re-punctuate a word it already showed,
    /// slide forward, or be replaced wholesale after a language switch, and each
    /// of those replaces the previous preview rather than being compared with
    /// it. The preview is never folded into the committed text: the provider's
    /// committed text arrives on the `Delta` channel.
    fn apply_partial(&mut self, text: &str, segment: Option<&str>) {
        self.live = text.trim().to_string();
        self.live_segment = segment.map(str::to_string);
    }

    /// Joins committed text and the preview for display.
    ///
    /// Three rules, in order:
    ///
    /// 1. a preview the committed text already ends with — compared word by
    ///    word, so case and edge punctuation do not matter — adds nothing: the
    ///    take already shows those words. This is what keeps a provider that
    ///    publishes its whole take on both channels from doubling it;
    /// 2. otherwise the longest word run shared at the joint is removed. Two
    ///    words are a duplicated fragment. One word is removed only when the
    ///    preview keeps going after it: a preview that restates the last
    ///    committed word and then continues is a continuation of that word, not
    ///    a repetition of it. A preview that *ends* on the shared word is rule 1
    ///    and adds nothing;
    /// 3. otherwise the two are joined by exactly one space.
    ///
    /// A word repeated inside one hypothesis never reaches this seam, so the
    /// rules may drop a shared word without swallowing real speech (`très très`
    /// is one partial or one delta).
    ///
    /// Words are compared case-insensitively and without the punctuation at
    /// their edges, because a provider re-punctuates the same window (`3.5`
    /// against `3.5.`, `live` against `Live.`). A token that is punctuation only
    /// compares as itself. The text returned is always the provider's own
    /// spelling and spacing: only whole words are dropped from the joint.
    #[must_use]
    fn seam_join(committed: &str, live: &str) -> String {
        let committed = committed.trim();
        let live = live.trim();

        if committed.is_empty() {
            return live.to_string();
        }
        if live.is_empty() {
            return committed.to_string();
        }

        let committed_words = word_spans(committed);
        let live_words = word_spans(live);

        let shares_run = |run: usize| {
            same_words(
                committed,
                &committed_words[committed_words.len() - run..],
                live,
                &live_words[..run],
            )
        };

        // Rule 1: every word of the preview is already the end of the take.
        if live_words.len() <= committed_words.len() && shares_run(live_words.len()) {
            return committed.to_string();
        }

        // Rule 2: the longest run shared at the joint.
        let longest_shared = (1..=live_words.len().min(committed_words.len()))
            .rev()
            .find(|&run| shares_run(run));

        if let Some(run) = longest_shared
            && (run >= MIN_SEAM_RUN || run < live_words.len())
        {
            let remainder = &live[live_words[run].start..];
            return format!("{committed} {remainder}");
        }

        format!("{committed} {live}")
    }
}

/// Length of a shared word run at the joint that is a duplicate on its own.
///
/// Two words are a duplicated fragment whatever the preview does next. A single
/// shared word is only dropped when the preview continues past it; a preview
/// that ends on it is caught by the suffix rule first.
const MIN_SEAM_RUN: usize = 2;

/// Byte span of one whitespace-delimited word inside the text it came from.
#[derive(Debug, Clone, Copy)]
struct WordSpan {
    start: usize,
    end: usize,
}

/// Splits `text` into its whitespace-delimited words, keeping their byte spans.
///
/// Byte offsets rather than character indices, so the seam can cut the preview
/// at a word start and return the provider's own slice of it.
fn word_spans(text: &str) -> Vec<WordSpan> {
    let mut spans = Vec::new();
    let mut start = None;

    for (offset, character) in text.char_indices() {
        if character.is_whitespace() {
            if let Some(begin) = start.take() {
                spans.push(WordSpan {
                    start: begin,
                    end: offset,
                });
            }
        } else if start.is_none() {
            start = Some(offset);
        }
    }

    if let Some(begin) = start {
        spans.push(WordSpan {
            start: begin,
            end: text.len(),
        });
    }

    spans
}

/// Comparison key of one word: lower-cased, with the punctuation at its edges
/// removed.
///
/// Interior punctuation is kept, so `3.5` stays one word and is not equal to
/// `3` or `5`. A token that is punctuation only (`?`, `«`, `»`) keeps that
/// punctuation as its key, so the same mark on both sides of the joint still
/// matches while two different marks do not.
fn word_key(word: &str) -> String {
    let trimmed = word.trim_matches(|character: char| !character.is_alphanumeric());
    if trimmed.is_empty() {
        word.to_lowercase()
    } else {
        trimmed.to_lowercase()
    }
}

/// Whether two equal-length runs of words are the same words, in order.
fn same_words(left: &str, left_spans: &[WordSpan], right: &str, right_spans: &[WordSpan]) -> bool {
    left_spans
        .iter()
        .zip(right_spans)
        .all(|(left_span, right_span)| {
            word_key(&left[left_span.start..left_span.end])
                == word_key(&right[right_span.start..right_span.end])
        })
}

#[cfg(test)]
mod tests {
    use super::*;

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

    // ----- One take through both channels ------------------------------

    #[test]
    fn streams_delta_committed_then_stopped() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&delta("take-1", "audio"));
        assert_eq!(buffer.committed(), "audio");
        assert_eq!(buffer.live(), "");
        assert_eq!(buffer.full_text(), "audio");

        buffer.apply(&delta("take-1", " capture"));
        assert_eq!(buffer.committed(), "audio capture");

        buffer.apply(&committed("take-1", "audio capture layer."));
        assert_eq!(buffer.committed(), "audio capture layer.");
        assert_eq!(buffer.live(), "");

        buffer.apply(&partial("take-1", "next words"));
        assert_eq!(buffer.committed(), "audio capture layer.");
        assert_eq!(buffer.live(), "next words");

        // A final `stopped` with no new text folds the preview in.
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
    fn deltas_append_to_the_committed_text() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&delta("take-1", "audio"));
        buffer.apply(&delta("take-1", " capture"));
        buffer.apply(&delta("take-1", " layer"));

        assert_eq!(buffer.committed(), "audio capture layer");
        assert_eq!(buffer.live(), "");
        assert_eq!(buffer.full_text(), "audio capture layer");
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

        buffer.apply(&delta("take-1", "draft"));
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

    /// ElevenLabs ended its take with an empty `stopped`: the preview is the
    /// only text there is, so folding it in is what keeps the take.
    #[test]
    fn stopped_without_text_keeps_the_preview_of_a_take_with_no_committed_text() {
        let mut buffer = TranscriptBuffer::default();
        let last = "Et voici le début d'une nouvelle dictée. Nous allons essayer \
                    d'observer la fenêtre glissante.";

        buffer.apply(&partial("take-1", last));
        buffer.apply(&stopped("take-1", ""));

        assert_eq!(buffer.committed(), last);
        assert_eq!(buffer.live(), "");
        assert_eq!(buffer.full_text(), last);
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
        assert_eq!(buffer.full_text(), "committed");

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

    // ----- The seam between the two channels ---------------------------

    /// A preview the committed text already ends with is the same sentence: it
    /// adds nothing, even when the provider re-punctuated it on the way.
    #[test]
    fn a_preview_the_committed_text_already_ends_with_adds_nothing() {
        let committed = "Et voici le dernier test de Gemini 3.5. Transcribe live. A-t-il";
        let live = "Et voici le dernier test de Gemini 3.5 Transcribe Live. A-t-il";

        assert_eq!(TranscriptBuffer::seam_join(committed, live), committed);
    }

    /// The whole preview is the tail of the committed text: no continuation is
    /// left, so there is nothing to add.
    #[test]
    fn a_one_word_preview_the_take_already_ends_with_adds_nothing() {
        assert_eq!(
            TranscriptBuffer::seam_join("à la fin de cette session.", "session."),
            "à la fin de cette session."
        );
    }

    /// A preview that restates the last committed word and then keeps going is
    /// a continuation of that word, so the take must not show it twice.
    #[test]
    fn a_one_word_run_the_preview_continues_past_is_removed() {
        assert_eq!(
            TranscriptBuffer::seam_join("nous allons le découvrir", "découvrir à la fin"),
            "nous allons le découvrir à la fin"
        );
        assert_eq!(
            TranscriptBuffer::seam_join("à la fin de cette", "cette session."),
            "à la fin de cette session."
        );
    }

    #[test]
    fn a_two_word_run_at_the_joint_is_removed() {
        assert_eq!(
            TranscriptBuffer::seam_join("Nous allons le découvrir", "le découvrir à la fin"),
            "Nous allons le découvrir à la fin"
        );
    }

    /// A repetition inside one hypothesis is one partial or one delta, so it
    /// never reaches the seam and no rule can swallow it.
    #[test]
    fn a_repeated_word_inside_one_hypothesis_survives() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&delta("take-1", "c'est très très bon"));

        assert_eq!(buffer.committed(), "c'est très très bon");
        assert_eq!(buffer.full_text(), "c'est très très bon");
        assert_eq!(TranscriptBuffer::seam_join("", "très très"), "très très");
    }

    #[test]
    fn disjoint_text_is_joined_by_exactly_one_space() {
        assert_eq!(
            TranscriptBuffer::seam_join("Dans le cas", "d'un blueprint"),
            "Dans le cas d'un blueprint"
        );
        assert_eq!(
            TranscriptBuffer::seam_join("Dans le cas ", " d'un blueprint "),
            "Dans le cas d'un blueprint"
        );
        assert_eq!(TranscriptBuffer::seam_join("", "seul"), "seul");
        assert_eq!(TranscriptBuffer::seam_join("seul", ""), "seul");
        assert_eq!(TranscriptBuffer::seam_join("  ", "  "), "");
    }

    /// Gemini sends its whole take as one delta *and* re-sends it as a preview,
    /// with the punctuation fixed differently. The two are shown once.
    #[test]
    fn a_whole_take_sent_on_both_channels_is_shown_once() {
        let mut buffer = TranscriptBuffer::default();
        let take = "Et voici le dernier test de Gemini 3.5. Transcribe live.";
        let preview = "Et voici le dernier test de Gemini 3.5 Transcribe Live.";

        buffer.apply(&partial("take-1", preview));
        buffer.apply(&delta("take-1", take));

        assert_eq!(buffer.committed(), take);
        assert_eq!(buffer.live(), preview);
        assert_eq!(buffer.full_text(), take);
    }

    /// The preview opens on the words the committed text ends with: one
    /// sentence, not two, so the shared run is shown once.
    #[test]
    fn full_text_does_not_double_a_two_word_seam() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&delta("take-1", "Nous allons le découvrir"));
        buffer.apply(&partial("take-1", "le découvrir à la fin"));

        assert_eq!(buffer.full_text(), "Nous allons le découvrir à la fin");
        assert_eq!(buffer.committed(), "Nous allons le découvrir");
    }

    // ----- The delta channel -------------------------------------------

    /// MAI splits `Transcribe` across two deltas and puts the leading spaces in
    /// itself. Anything inserted between them turns one word into two.
    #[test]
    fn deltas_concatenate_verbatim_so_a_word_split_across_them_stays_one_word() {
        let mut buffer = TranscriptBuffer::default();

        for chunk in ["MAI", " Trans", "cribe", " to"] {
            buffer.apply(&delta("take-1", chunk));
        }

        assert_eq!(buffer.committed(), "MAI Transcribe to");
        assert_eq!(buffer.live(), "");
        assert_eq!(buffer.full_text(), "MAI Transcribe to");
    }

    #[test]
    fn a_delta_that_opens_a_take_is_taken_as_it_is() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&delta("take-1", "Nouveau test"));
        assert_eq!(buffer.committed(), "Nouveau test");
        assert_eq!(buffer.live(), "");
        assert_eq!(buffer.full_text(), "Nouveau test");

        // The separator in front of the provider's first word is not text: the
        // take still opens on the word.
        let mut leading = TranscriptBuffer::default();
        leading.apply(&delta("take-1", "  Nouveau test"));
        assert_eq!(leading.committed(), "Nouveau test");
    }

    #[test]
    fn a_partial_followed_by_the_same_delta_keeps_one_copy() {
        let mut buffer = TranscriptBuffer::default();
        let text = "d'une transcription quelconque";

        buffer.apply(&segment_partial("take-1", "seg-1", text));
        buffer.apply(&segment_delta("take-1", "seg-1", text));

        assert_eq!(buffer.committed(), text);
        assert_eq!(buffer.full_text(), text);
        assert_eq!(
            buffer.full_text().matches("quelconque").count(),
            1,
            "the take shows one copy of the hypothesis"
        );
    }

    #[test]
    fn a_cumulative_delta_replaces_the_committed_text() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_delta("take-1", "seg-1", "d'une"));
        buffer.apply(&segment_delta("take-1", "seg-1", "d'une transcription"));

        assert_eq!(buffer.committed(), "d'une transcription");
        assert_eq!(buffer.full_text(), "d'une transcription");
    }

    /// A cumulative delta carries the whole take again. Appending it instead of
    /// replacing would duplicate every character it repeats, including the ones
    /// it extends inside a word.
    #[test]
    fn a_cumulative_delta_that_extends_a_word_replaces_the_committed_text() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_delta("take-1", "seg-0", "un blueprint mesur"));
        buffer.apply(&segment_delta("take-1", "seg-0", "un blueprint mesuré"));

        assert_eq!(buffer.committed(), "un blueprint mesuré");
        assert_eq!(buffer.full_text(), "un blueprint mesuré");
    }

    #[test]
    fn an_incremental_delta_still_appends() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_delta("take-1", "seg-1", "d'une"));
        buffer.apply(&segment_delta("take-1", "seg-1", " transcription"));

        assert_eq!(buffer.committed(), "d'une transcription");
    }

    /// A provider that streams the space between two words as its own delta
    /// must still produce two words, not one glued word.
    #[test]
    fn a_whitespace_delta_keeps_the_separator_between_two_words() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_delta("take-1", "seg-1", "le"));
        buffer.apply(&segment_delta("take-1", "seg-1", " "));
        buffer.apply(&segment_delta("take-1", "seg-1", "monde"));

        assert_eq!(buffer.committed(), "le monde");
        assert_eq!(buffer.full_text(), "le monde");
    }

    #[test]
    fn a_whitespace_delta_never_opens_the_committed_text_with_a_blank() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&delta("take-1", " "));
        buffer.apply(&delta("take-1", "   "));
        assert_eq!(buffer.committed(), "");

        // A separator already at the end of the committed text is not doubled.
        buffer.apply(&delta("take-1", "le"));
        buffer.apply(&delta("take-1", " "));
        buffer.apply(&delta("take-1", " "));
        assert_eq!(buffer.committed(), "le ");
        assert_eq!(buffer.full_text(), "le");
    }

    #[test]
    fn a_delta_already_at_the_end_of_the_committed_text_is_ignored() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_delta("take-1", "seg-1", "oui."));
        buffer.apply(&segment_delta("take-1", "seg-1", "oui."));
        buffer.apply(&segment_delta("take-1", "seg-1", "oui."));

        assert_eq!(buffer.committed(), "oui.");
    }

    /// A word the backend really says twice is not a duplicate: both deltas are
    /// kept, because the second one carries its own separator. A third identical
    /// delta is indistinguishable from that text being re-sent, so it is
    /// ignored.
    #[test]
    fn a_repeated_word_is_not_swallowed_as_a_duplicate() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_delta("take-1", "seg-1", "très"));
        buffer.apply(&segment_delta("take-1", "seg-1", " très"));

        assert_eq!(buffer.committed(), "très très");
        assert_eq!(buffer.full_text(), "très très");
    }

    /// A bare word the committed text already ends with carries no separator of
    /// its own, so it is the same text re-sent rather than new speech.
    #[test]
    fn a_delta_that_repeats_a_whole_word_is_a_re_send() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_delta("take-1", "seg-1", "le chat"));
        buffer.apply(&segment_delta("take-1", "seg-1", "chat"));

        assert_eq!(buffer.committed(), "le chat");
        assert_eq!(buffer.full_text(), "le chat");
    }

    /// The delta channel is the source of truth for committed text, so a
    /// preview that trails it joins the take without repeating the words.
    #[test]
    fn a_partial_then_a_delta_that_adds_words_keeps_the_new_words() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_delta("take-1", "seg-1", "d'une transcription"));
        buffer.apply(&segment_partial(
            "take-1",
            "seg-1",
            "d'une transcription quelconque",
        ));

        assert_eq!(buffer.committed(), "d'une transcription");
        assert_eq!(buffer.full_text(), "d'une transcription quelconque");
    }

    /// A delta from a later utterance continues the same take: the buffer holds
    /// one transcript, and a segment identity never splits it.
    #[test]
    fn a_delta_of_a_new_segment_continues_the_same_take() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_delta("take-1", "seg-1", "hello"));
        buffer.apply(&segment_delta("take-1", "seg-2", " world"));

        assert_eq!(buffer.committed(), "hello world");
        assert_eq!(buffer.live(), "");
        assert_eq!(buffer.full_text(), "hello world");
    }

    /// The same delta re-sent under a new utterance identity is still the same
    /// text: it is not appended a second time.
    #[test]
    fn a_repeated_delta_under_a_new_segment_is_still_a_duplicate() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_delta("take-1", "seg-1", "hello"));
        buffer.apply(&segment_delta("take-1", "seg-2", "hello"));

        assert_eq!(buffer.committed(), "hello");
        assert_eq!(buffer.full_text(), "hello");
    }

    // ----- The preview channel -----------------------------------------

    /// A partial is a fresh preview, not a delta to append to: a provider that
    /// rewrites the window it is speaking (re-punctuates, corrects a word, or
    /// switches language) replaces the preview in place.
    #[test]
    fn a_retroactive_rewrite_replaces_the_preview_in_place() {
        let mut buffer = TranscriptBuffer::default();
        let before = "Et voici le dernier test. A-t-il une fenêtre coulissante ou";
        let after = "Et voici le dernier test. A-t-il une fenêtre glissante ou";

        buffer.apply(&partial("take-1", before));
        buffer.apply(&partial("take-1", after));

        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.live(), after);
        assert_eq!(buffer.full_text(), after);
    }

    /// Gemini answered the French take with `It was` before switching language.
    /// The replacement replaces the preview; the two are never concatenated.
    #[test]
    fn a_language_switch_replaces_the_preview() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&partial("take-1", "It was"));
        buffer.apply(&partial("take-1", "Et voici le"));

        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.live(), "Et voici le");
        assert_eq!(buffer.full_text(), "Et voici le");
    }

    /// MAI slides its window: the newest hypothesis starts at the *suffix* of
    /// the previous one. The preview follows it; nothing is committed.
    #[test]
    fn a_sliding_fragment_replaces_the_preview_without_settling_it() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&partial("take-1", "ée avec le modèle MAI."));
        buffer.apply(&partial("take-1", "le modèle MAI."));
        buffer.apply(&partial("take-1", "modèle MAI."));

        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.live(), "modèle MAI.");
        assert_eq!(buffer.full_text(), "modèle MAI.");
    }

    #[test]
    fn an_identical_partial_leaves_the_buffer_unchanged() {
        let mut buffer = TranscriptBuffer::default();
        buffer.apply(&segment_partial("take-1", "seg-1", "Dans le cas"));
        let once = buffer.clone();

        buffer.apply(&segment_partial("take-1", "seg-1", "Dans le cas"));

        assert_eq!(buffer, once);
        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.full_text(), "Dans le cas");
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
    fn partials_inside_one_segment_replace_only_the_preview() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial("take-1", "seg-1", "hel"));
        buffer.apply(&segment_partial("take-1", "seg-1", "hello"));

        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.live(), "hello");
    }

    /// A new utterance identity does not settle the preview it replaces: the
    /// words of the closed utterance arrive on the delta channel or in the
    /// authoritative final, never by promoting a preview.
    #[test]
    fn a_new_segment_replaces_the_previous_hypothesis() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial("take-1", "seg-1", "hello"));
        assert_eq!(buffer.live(), "hello");

        buffer.apply(&segment_partial("take-1", "seg-2", "world"));
        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.live(), "world");
    }

    /// A provider re-punctuating its window may resend a shorter draft of it.
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

    /// A short fragment that only repeats the opening of a long preview is the
    /// next window, not a revision of the last one.
    #[test]
    fn a_short_fragment_matching_the_opening_replaces_the_preview() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial(
            "take-1",
            "seg-0",
            "Et ensuite le workflow va consister",
        ));
        buffer.apply(&segment_partial("take-1", "seg-0", "Et"));

        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.live(), "Et");
        assert_eq!(buffer.full_text(), "Et");
    }

    /// A windowing provider restarts its hypothesis for every window: the
    /// preview follows the newest window and nothing is promoted.
    #[test]
    fn a_restarting_window_replaces_the_preview() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial("take-1", "seg-0", "Dans le cas"));
        buffer.apply(&segment_partial("take-1", "seg-0", "d'un blueprint"));

        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.live(), "d'un blueprint");
        assert_eq!(buffer.full_text(), "d'un blueprint");
    }

    #[test]
    fn whitespace_only_text_appends_nothing() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial("take-1", "seg-1", "hello"));
        buffer.apply(&segment_partial("take-1", "seg-2", "   "));
        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.live(), "");
        assert_eq!(buffer.full_text(), "");

        buffer.apply(&segment_committed("take-1", "seg-2", "  "));
        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.full_text(), "");

        // A whitespace-only delta is not committed as a stray separator.
        let mut take = TranscriptBuffer::default();
        take.apply(&delta("take-1", "   "));
        assert_eq!(take.committed(), "");
    }

    // ----- The final of an utterance -----------------------------------

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

    /// A final is the authoritative text of its utterance, so it replaces the
    /// preview even when no segment identity pairs the two.
    #[test]
    fn a_final_naming_a_segment_replaces_an_unidentified_preview() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&partial("take-1", "hello"));
        buffer.apply(&segment_committed("take-1", "seg-1", "Hello."));

        assert_eq!(buffer.committed(), "Hello.");
        assert_eq!(buffer.live(), "");
        assert_eq!(buffer.full_text(), "Hello.");
    }

    #[test]
    fn a_final_for_the_next_segment_replaces_an_unsettled_preview() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial("take-1", "seg-1", "hello"));
        buffer.apply(&segment_committed("take-1", "seg-2", "World."));

        assert_eq!(buffer.committed(), "World.");
        assert_eq!(buffer.live(), "");
        assert_eq!(buffer.full_text(), "World.");
    }

    /// A final adds text to the committed transcript; it never drops a word the
    /// transcript already held.
    #[test]
    fn a_committed_event_keeps_every_word_already_committed() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&delta("take-1", "un deux"));
        buffer.apply(&committed("take-1", "trois quatre."));

        assert_eq!(buffer.committed(), "un deux trois quatre.");
    }

    #[test]
    fn a_committed_event_with_no_text_still_clears_the_preview() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&delta("take-1", "un deux"));
        buffer.apply(&partial("take-1", "troi"));
        buffer.apply(&committed("take-1", ""));

        assert_eq!(buffer.committed(), "un deux");
        assert_eq!(buffer.live(), "");
    }

    // ----- End of take -------------------------------------------------

    /// A non-empty `stopped` is the authoritative take: it replaces what the
    /// live view assembled, because the provider's final text is the only
    /// ground truth of the take.
    #[test]
    fn a_stopped_text_replaces_the_take_even_when_it_only_repeats_the_tail() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&delta("take-1", "un deux trois quatre"));
        buffer.apply(&stopped("take-1", "trois quatre"));

        assert_eq!(buffer.committed(), "trois quatre");
        assert_eq!(buffer.full_text(), "trois quatre");
    }

    // ----- Invariants ---------------------------------------------------

    #[test]
    fn the_same_hypothesis_under_a_new_segment_is_not_settled_twice() {
        let mut buffer = TranscriptBuffer::default();

        buffer.apply(&segment_partial("take-1", "seg-0", "Dans le cas"));
        buffer.apply(&segment_partial("take-1", "seg-1", "Dans le cas"));

        assert_eq!(buffer.committed(), "");
        assert_eq!(buffer.live(), "Dans le cas");
        assert_eq!(buffer.full_text(), "Dans le cas");
    }

    /// A windowing provider restarts its hypothesis for every window. Every
    /// window is a preview; the authoritative final is what makes the take, and
    /// no window may survive into it twice.
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

        assert_eq!(buffer.committed(), "");
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
            delta("take-1", "one"),
            delta("take-1", " two"),
            partial("take-1", "one two"),
            segment_partial("take-1", "seg-2", "three"),
            segment_committed("take-1", "seg-2", "three four."),
            delta("take-1", " five six"),
            partial("take-1", "five six"),
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
