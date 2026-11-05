//! Replays three measured provider captures: a live take must stay readable.
//!
//! The `.tsv` fixtures under `fixtures/` are verbatim dumps of real dictations,
//! one event per line, exactly as the worker received them from ElevenLabs Scribe
//! v2 Realtime, Microsoft MAI Transcribe 2 Streaming and Google Gemini 3.5
//! Transcribe Live. They are the only record of how those three providers
//! behave, and they disagree with each other: one re-sends the whole take on
//! every partial, one sends sliding fragments plus deltas that stop in the middle
//! of a word, one sends the whole take twice, once on each channel.
//!
//! Every test here replays one capture through
//! [`TranscriptBuffer::apply`](speechtt_core::transcript::TranscriptBuffer::apply)
//! and checks the display after **each** event rather than only at the end: the
//! live transcript is what the user reads while dictating, so a smear that the
//! provider's final text happens to repair has already cost the take. The shared
//! invariants are the ones that hold for every provider shape:
//!
//! - the displayed text never runs longer than the take the provider stands
//!   behind (`full_text()`);
//! - no two-word run appears twice back to back (`le modèle le modèle`);
//! - the committed half never rewinds (`committed()`).
//!
//! Per-capture tests pin what makes each take different, because the shared
//! invariants alone can be satisfied by throwing a channel away: MAI's deltas must
//! rebuild its take byte for byte, Gemini's two whole-take channels must not
//! double, and an empty `stopped` must leave ElevenLabs' last hypothesis on screen
//! instead of a blank.
//!
//! The captures are compiled in with `include_str!` and replayed synchronously, so
//! `cargo test -p speechtt-core --test transcript_captures` needs no microphone,
//! no worker, no display server and no clock.

use speechtt_core::protocol::SttEvent;
use speechtt_core::transcript::TranscriptBuffer;

/// The session id every capture was recorded under. The reducer keys nothing on
/// it; the event order is the whole input.
const SESSION: &str = "take-1";

/// Cumulative partials, no delta, and a `stopped` that arrived empty.
const ELEVENLABS: &str = include_str!("fixtures/elevenlabs-scribe-v2-realtime.tsv");
/// Sliding partial fragments preceded by incremental deltas that split words.
const MAI: &str = include_str!("fixtures/mai-transcribe-2-streaming.tsv");
/// Cumulative partials and one delta that carries the whole take again.
const GEMINI: &str = include_str!("fixtures/gemini-3.5-transcribe-live.tsv");

/// The transcript-bearing event types a capture records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Partial,
    Delta,
    Committed,
    Stopped,
}

impl Kind {
    /// The type name the dump writes for this event, or `None` for a line the
    /// loader must reject.
    fn parse(name: &str) -> Option<Self> {
        match name {
            "partial" => Some(Self::Partial),
            "delta" => Some(Self::Delta),
            "committed" => Some(Self::Committed),
            "stopped" => Some(Self::Stopped),
            _ => None,
        }
    }
}

/// One recorded event: its type and the provider's own text, untrimmed.
#[derive(Debug, Clone)]
struct Recorded {
    kind: Kind,
    text: String,
}

impl Recorded {
    /// The wire event this line stands for, replayed under one session.
    ///
    /// No fixture carries a segment: each take is one continuous utterance, so
    /// the reducer has to rebuild it from the event order alone and must not
    /// depend on an utterance identity arriving.
    fn event(&self) -> SttEvent {
        match self.kind {
            Kind::Partial => SttEvent::Partial {
                session_id: SESSION.to_owned(),
                text: self.text.clone(),
                segment: None,
            },
            Kind::Delta => SttEvent::Delta {
                session_id: SESSION.to_owned(),
                delta: self.text.clone(),
                segment: None,
            },
            Kind::Committed => SttEvent::Committed {
                session_id: SESSION.to_owned(),
                text: self.text.clone(),
                segment: None,
            },
            Kind::Stopped => SttEvent::Stopped {
                session_id: SESSION.to_owned(),
                full_text: self.text.clone(),
            },
        }
    }
}

/// One take: the model that produced it, the text the provider stands behind, and
/// its event stream in emission order.
struct Capture {
    model: String,
    authoritative: String,
    events: Vec<Recorded>,
}

/// Parses one dump: the `model` and `authoritative` headers, then every
/// `<type>\t<text>` line.
///
/// Text is kept byte for byte. A delta's leading space, and a delta that stops in
/// the middle of a word, are the provider's own; trimming or re-spacing them
/// would destroy the very evidence the take is rebuilt from.
fn capture(source: &str) -> Capture {
    let mut model = String::new();
    let mut authoritative = String::new();
    let mut events: Vec<Recorded> = Vec::new();

    for line in source.lines() {
        if let Some(value) = line.strip_prefix("# model=") {
            model = value.trim().to_owned();
            continue;
        }
        if let Some(value) = line.strip_prefix("# authoritative=") {
            authoritative = value.trim().to_owned();
            continue;
        }
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let Some((name, text)) = line.split_once('\t') else {
            panic!("capture line carries no type/text separator: {line:?}");
        };
        let Some(kind) = Kind::parse(name) else {
            panic!("capture line carries an unknown event type: {line:?}");
        };
        events.push(Recorded {
            kind,
            text: text.to_owned(),
        });
    }

    assert!(!model.is_empty(), "capture has no `# model=` header");
    assert!(
        !authoritative.is_empty(),
        "capture has no `# authoritative=` header"
    );
    assert!(!events.is_empty(), "capture has no events");
    Capture {
        model,
        authoritative,
        events,
    }
}

/// The buffer state a reader would see after one event of the replay.
///
/// Both strings are trimmed: text at the very edge of the screen is not displayed
/// content, and no assertion here is about it.
#[derive(Debug)]
struct Frame {
    kind: Kind,
    text: String,
    displayed: String,
    committed: String,
}

/// Replays `capture` from an empty buffer and snapshots the display after every
/// event.
fn replay(capture: &Capture) -> Vec<Frame> {
    let mut buffer = TranscriptBuffer::default();
    let mut frames = Vec::with_capacity(capture.events.len());
    for record in &capture.events {
        buffer.apply(&record.event());
        frames.push(Frame {
            kind: record.kind,
            text: record.text.clone(),
            displayed: buffer.full_text().trim().to_owned(),
            committed: buffer.committed().trim().to_owned(),
        });
    }
    frames
}

/// The text of every `kind` event of `capture`, concatenated verbatim.
///
/// Concatenation without a separator is what a delta channel means: the provider
/// supplies its own spacing, and ` Trans` followed by `cribe` is one word.
fn concatenated_text(capture: &Capture, kind: Kind) -> String {
    capture
        .events
        .iter()
        .filter(|record| record.kind == kind)
        .map(|record| record.text.as_str())
        .collect()
}

/// The trimmed text of the last `kind` event of `capture`.
fn last_text_of(capture: &Capture, kind: Kind) -> String {
    capture
        .events
        .iter()
        .rev()
        .find(|record| record.kind == kind)
        .map(|record| record.text.trim().to_owned())
        .unwrap_or_default()
}

/// Index of the last frame produced by an event of `kind`.
fn last_frame_of(frames: &[Frame], kind: Kind) -> Option<usize> {
    frames.iter().rposition(|frame| frame.kind == kind)
}

/// A word as duplication is compared: lower-cased, with the punctuation that can
/// only sit at its edge removed.
///
/// Casing and edge punctuation are what a provider rewrites freely (Gemini sends
/// `Live.` on one channel and `live.` on the other), while the characters an
/// apostrophe or a hyphen join stay inside the word, so `d'une` and `A-t-il`
/// remain single words.
fn comparison_word(word: &str) -> String {
    word.to_lowercase()
        .trim_matches(|c: char| !c.is_alphanumeric())
        .to_owned()
}

/// The first two-word run that appears twice back to back in `text`.
///
/// The window is two words wide on purpose: a speaker really does say `très très`,
/// and collapsing that would drop a word the provider transcribed. Two words
/// repeated verbatim is a seam joined twice.
fn repeated_word_run(text: &str) -> Option<(String, String)> {
    let words: Vec<String> = text
        .split_whitespace()
        .map(comparison_word)
        .filter(|word| !word.is_empty())
        .collect();
    words
        .windows(4)
        .find(|window| window[0] == window[2] && window[1] == window[3])
        .map(|window| (window[0].clone(), window[1].clone()))
}

/// Asserts the invariants every capture holds after every single event.
///
/// The limit is the length of the take the provider stands behind: the display may
/// lag behind it, never run past it. A transient overshoot is the defect, because
/// the screen shows it for as long as the take lasts.
fn assert_stays_within_the_take(capture: &Capture, frames: &[Frame]) {
    let model = capture.model.as_str();
    let limit = capture.authoritative.chars().count();
    let mut previous_committed = String::new();

    for (index, frame) in frames.iter().enumerate() {
        let kind = frame.kind;
        let text = frame.text.as_str();
        let displayed = frame.displayed.as_str();
        let shown = displayed.chars().count();

        assert!(
            shown <= limit,
            "{model} displayed {shown} characters against a {limit}-character take \
             after event {index} ({kind:?} {text:?}): {displayed:?}"
        );
        if let Some((first, second)) = repeated_word_run(displayed) {
            panic!(
                "{model} displayed {first:?} {second:?} twice in a row after event \
                 {index} ({kind:?} {text:?}): {displayed:?}"
            );
        }
        assert!(
            frame.committed.starts_with(previous_committed.as_str()),
            "{model} lost committed text after event {index} ({kind:?} {text:?}): \
             {previous_committed:?} -> {:?}",
            frame.committed
        );
        previous_committed.clone_from(&frame.committed);
    }
}

/// Asserts the replay ends on the text the provider stands behind.
///
/// This is the assertion the end of a take lives or dies by: it catches a take
/// that drops its tail where no non-empty `stopped` arrives to repair it, and a
/// take left showing a preview the provider has already superseded.
fn assert_ends_on_the_take(capture: &Capture, frames: &[Frame]) {
    let model = capture.model.as_str();
    let last = frames.last().expect("the capture has events");
    assert_eq!(
        last.displayed.as_str(),
        capture.authoritative.as_str(),
        "{model} ended on the wrong text"
    );
}

/// ElevenLabs sends the whole take on every partial and no delta at all.
///
/// Its `stopped` arrived empty because the stream failed, so there is no
/// authoritative final: the last hypothesis is the take, and it is 241 characters
/// long. A reducer that settles each re-sent hypothesis leaves the screen smeared,
/// and one that reads an empty `stopped` as "nothing was said" leaves it blank.
#[test]
fn elevenlabs_ends_on_its_last_hypothesis() {
    let capture = capture(ELEVENLABS);
    let frames = replay(&capture);
    assert_stays_within_the_take(&capture, &frames);
    assert_ends_on_the_take(&capture, &frames);

    let model = capture.model.as_str();
    let last = frames.last().expect("the capture has events");
    let shown = last.displayed.chars().count();
    assert!(
        shown > 200,
        "{model} ended on {shown} characters: {:?}",
        last.displayed
    );
    assert_eq!(
        last.displayed.as_str(),
        last_text_of(&capture, Kind::Partial).as_str(),
        "{model} did not end on its last hypothesis"
    );
}

/// MAI sends sliding partial fragments and rebuilds the take only on its delta
/// channel: the deltas concatenate to the 185-character `stopped` text byte for
/// byte, while the last fragment is nine characters long.
///
/// So the display has to grow with the deltas instead of tracking the fragment,
/// and the delta channel has to be kept verbatim — no separator inserted, no delta
/// folded away as a duplicate of the one before it.
#[test]
fn mai_rebuilds_its_take_from_the_delta_channel() {
    let capture = capture(MAI);
    let frames = replay(&capture);
    assert_stays_within_the_take(&capture, &frames);
    assert_ends_on_the_take(&capture, &frames);

    let model = capture.model.as_str();
    let deltas = concatenated_text(&capture, Kind::Delta);
    let stopped = last_text_of(&capture, Kind::Stopped);
    assert_eq!(
        deltas.as_str(),
        stopped.as_str(),
        "{model} deltas do not rebuild its final text"
    );

    // After the last delta lands and before `stopped` overwrites anything, the
    // committed half is the take itself: every delta appended, none compared away.
    let last_delta = last_frame_of(&frames, Kind::Delta).expect("the capture has deltas");
    assert_eq!(
        frames[last_delta].committed.as_str(),
        deltas.as_str(),
        "{model} did not append its deltas verbatim"
    );

    let grown = frames
        .iter()
        .filter(|frame| frame.displayed.chars().count() > 150)
        .count();
    assert!(
        grown > 0,
        "{model} never grew past 150 characters, so its display stayed on the \
         sliding fragment instead of its deltas"
    );
}

/// Gemini sends the whole take twice: once as its single delta, once as the
/// preview that follows it.
///
/// The two are not byte-identical — the preview spells `3.5` without the full
/// stop and `Live` with a capital — so nothing but a word-level comparison sees
/// that the preview adds nothing. The take is 117 characters; joining the two
/// channels naively shows 233 and ends there, because this capture has no
/// `stopped` to repair it.
#[test]
fn gemini_joins_two_whole_take_channels_without_doubling() {
    let capture = capture(GEMINI);
    let frames = replay(&capture);
    assert_stays_within_the_take(&capture, &frames);
    assert_ends_on_the_take(&capture, &frames);

    let model = capture.model.as_str();
    let delta = last_text_of(&capture, Kind::Delta);
    let last_partial = last_text_of(&capture, Kind::Partial);
    let take = capture.authoritative.chars().count();
    assert_ne!(
        delta, last_partial,
        "{model} capture must keep two differently spelled whole takes"
    );
    assert_eq!(take, 117, "{model} take is not the 117 characters measured");

    // The last frame holds both channels in the buffer, so it is the frame where a
    // naive join doubles the take: it must still show the take once.
    let last = frames.last().expect("the capture has events");
    let shown = last.displayed.chars().count();
    assert_eq!(
        shown, take,
        "{model} displayed {shown} characters on a {take}-character take"
    );
    let delta_frame = last_frame_of(&frames, Kind::Delta).expect("the capture has a delta");
    assert_eq!(
        frames[delta_frame].displayed.chars().count(),
        take,
        "{model} doubled its delta against the preview it repeated"
    );
}

/// A delta may stop in the middle of a word: ` Trans` then `cribe` is one word.
///
/// Deltas carry the provider's own spacing, so they concatenate without a
/// separator and the committed half holds `MAI Transcribe to Streaming`. Inserting
/// a space between deltas, or settling a sliding preview between them, spells
/// `Trans cribe` — a word the speaker never said.
///
/// The assertion runs on `committed()` rather than on the display because the
/// preview at that moment is the window being spoken (`cribe to Speech.`), which
/// legitimately starts mid-word; the committed channel is where the word has to be
/// whole.
#[test]
fn mai_mid_word_deltas_join_into_one_word() {
    let capture = capture(MAI);
    let frames = replay(&capture);

    let model = capture.model.as_str();
    let deltas = concatenated_text(&capture, Kind::Delta);
    assert!(
        deltas.contains("MAI Transcribe to Streaming"),
        "{model} deltas do not spell the word once: {deltas:?}"
    );
    assert!(
        !deltas.contains("Trans cribe"),
        "{model} deltas spell the word split: {deltas:?}"
    );

    let mut whole_word = false;
    for frame in &frames {
        let kind = frame.kind;
        let text = frame.text.as_str();
        let committed = frame.committed.as_str();
        assert!(
            !committed.contains("Trans cribe"),
            "{model} split a word across two deltas after {kind:?} {text:?}: {committed:?}"
        );
        whole_word |= committed.contains("Transcribe");
    }
    assert!(
        whole_word,
        "{model} never held the whole word in its committed text"
    );
}
