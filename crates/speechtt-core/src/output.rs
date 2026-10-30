//! Transcript output adapters: clipboard, paste and save-to-file.
//!
//! Every effect is hidden behind a small trait ([`clipboard::ClipboardSink`],
//! [`paste::PasteSink`], [`file::FileOpener`]) so the UI and integration tests
//! can inject deterministic fakes. CI therefore never needs a clipboard, an
//! input-injection daemon, a display server, or an editor: the only real
//! implementations are [`clipboard::SystemClipboard`],
//! [`paste::SystemPaste`] and [`file::SystemOpener`], and they are exercised
//! only in production.

use thiserror::Error;

/// Why an output action failed.
///
/// Messages never contain transcript text or secret material.
#[derive(Debug, Error)]
pub enum OutputError {
    #[error("transcript is empty")]
    EmptyTranscript,
    #[error("clipboard unavailable: {0}")]
    Clipboard(String),
    #[error("could not synthesise paste: {0}")]
    Paste(String),
    #[error("unsupported transcript format: {0}")]
    UnsupportedFormat(String),
    #[error("could not write transcript: {0}")]
    Write(String),
    #[error("could not open file: {0}")]
    OpenFile(String),
}

/// Copying a transcript to the system clipboard.
pub mod clipboard {
    use std::fmt;

    use super::OutputError;

    /// Destination for clipboard writes. Implemented by the UI fake and by
    /// [`SystemClipboard`].
    pub trait ClipboardSink: Send {
        /// Places `text` on the clipboard exactly as given.
        fn copy(&mut self, text: &str) -> Result<(), OutputError>;
    }

    /// Real clipboard backed by `arboard`.
    ///
    /// The platform handle is created on the first copy, so constructing this
    /// type never opens a display connection or touches the clipboard.
    #[derive(Default)]
    pub struct SystemClipboard {
        handle: Option<arboard::Clipboard>,
    }

    impl SystemClipboard {
        /// A clipboard that connects on first use.
        #[must_use]
        pub fn new() -> Self {
            Self::default()
        }
    }

    impl fmt::Debug for SystemClipboard {
        fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter
                .debug_struct("SystemClipboard")
                .field(
                    "handle",
                    &if self.handle.is_some() {
                        "initialised"
                    } else {
                        "uninitialised"
                    },
                )
                .finish()
        }
    }

    impl ClipboardSink for SystemClipboard {
        fn copy(&mut self, text: &str) -> Result<(), OutputError> {
            let handle = match &mut self.handle {
                Some(handle) => handle,
                None => self
                    .handle
                    .insert(arboard::Clipboard::new().map_err(clipboard_error)?),
            };
            handle.set_text(text).map_err(clipboard_error)
        }
    }

    fn clipboard_error(error: arboard::Error) -> OutputError {
        OutputError::Clipboard(error.to_string())
    }

    /// What was copied, so the UI can say "Copied 214 characters" without
    /// measuring the transcript again.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct CopySummary {
        pub characters: usize,
        pub lines: usize,
    }

    /// Joins the committed and live transcript parts and copies the result.
    ///
    /// The two parts are trimmed and joined with a single space when both are
    /// non-empty. An empty transcript is rejected before the sink is touched.
    pub fn copy_transcript(
        sink: &mut dyn ClipboardSink,
        committed: &str,
        live: &str,
    ) -> Result<CopySummary, OutputError> {
        let text = compose(committed, live);
        if text.is_empty() {
            return Err(OutputError::EmptyTranscript);
        }
        sink.copy(&text)?;
        Ok(CopySummary {
            characters: text.chars().count(),
            lines: text.lines().count(),
        })
    }

    /// Trim both parts and join them with exactly one space.
    fn compose(committed: &str, live: &str) -> String {
        let committed = committed.trim();
        let live = live.trim();
        match (committed.is_empty(), live.is_empty()) {
            (true, true) => String::new(),
            (true, false) => live.to_owned(),
            (false, true) => committed.to_owned(),
            (false, false) => format!("{committed} {live}"),
        }
    }

    #[cfg(test)]
    mod tests {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};

        use super::*;

        #[derive(Clone)]
        struct RecordingClipboard {
            copies: Arc<Mutex<Vec<String>>>,
        }

        impl RecordingClipboard {
            fn new() -> (Self, Arc<Mutex<Vec<String>>>) {
                let copies = Arc::new(Mutex::new(Vec::new()));
                (
                    Self {
                        copies: Arc::clone(&copies),
                    },
                    copies,
                )
            }
        }

        impl ClipboardSink for RecordingClipboard {
            fn copy(&mut self, text: &str) -> Result<(), OutputError> {
                self.copies
                    .lock()
                    .expect("clipboard log is not poisoned")
                    .push(text.to_owned());
                Ok(())
            }
        }

        struct FailingClipboard {
            calls: Arc<AtomicUsize>,
        }

        impl ClipboardSink for FailingClipboard {
            fn copy(&mut self, _text: &str) -> Result<(), OutputError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Err(OutputError::Clipboard("no display".to_owned()))
            }
        }

        fn copied(sink: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
            sink.lock().expect("clipboard log is not poisoned").clone()
        }

        #[test]
        fn copies_committed_transcript_exactly() {
            let (mut sink, log) = RecordingClipboard::new();

            let summary = copy_transcript(&mut sink, "Hello world", "").expect("copy succeeds");

            assert_eq!(copied(&log), vec!["Hello world".to_owned()]);
            assert_eq!(
                summary,
                CopySummary {
                    characters: 11,
                    lines: 1
                }
            );
        }

        #[test]
        fn copies_live_transcript_exactly() {
            let (mut sink, log) = RecordingClipboard::new();

            let summary = copy_transcript(&mut sink, "", "Live text").expect("copy succeeds");

            assert_eq!(copied(&log), vec!["Live text".to_owned()]);
            assert_eq!(
                summary,
                CopySummary {
                    characters: 9,
                    lines: 1
                }
            );
        }

        #[test]
        fn joins_both_parts_with_a_single_space() {
            let (mut sink, log) = RecordingClipboard::new();

            copy_transcript(&mut sink, "  Hello  ", "  world  ").expect("copy succeeds");

            assert_eq!(copied(&log), vec!["Hello world".to_owned()]);
        }

        #[test]
        fn preserves_internal_whitespace_and_newlines() {
            let (mut sink, log) = RecordingClipboard::new();

            copy_transcript(&mut sink, "line one\nline two", "tail").expect("copy succeeds");

            assert_eq!(copied(&log), vec!["line one\nline two tail".to_owned()]);
        }

        #[test]
        fn empty_transcript_is_rejected_without_touching_the_sink() {
            let (mut sink, log) = RecordingClipboard::new();

            for (committed, live) in [("", ""), ("   ", "\n\t")] {
                let result = copy_transcript(&mut sink, committed, live);
                assert!(
                    matches!(result, Err(OutputError::EmptyTranscript)),
                    "expected EmptyTranscript for {committed:?}/{live:?}"
                );
            }

            assert!(copied(&log).is_empty());
        }

        #[test]
        fn clipboard_failure_propagates() {
            let calls = Arc::new(AtomicUsize::new(0));
            let mut sink = FailingClipboard {
                calls: Arc::clone(&calls),
            };

            let result = copy_transcript(&mut sink, "Hello", "");

            assert!(matches!(result, Err(OutputError::Clipboard(_))));
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }

        #[test]
        fn summary_counts_unicode_scalars_not_bytes() {
            let (mut sink, log) = RecordingClipboard::new();

            let summary = copy_transcript(&mut sink, "naïve ☕", "").expect("copy succeeds");

            assert_eq!(copied(&log), vec!["naïve ☕".to_owned()]);
            assert_eq!(summary.characters, 7);
            assert_eq!(summary.lines, 1);
        }

        #[test]
        fn summary_counts_lines() {
            let (mut sink, _log) = RecordingClipboard::new();

            let summary = copy_transcript(&mut sink, "a\nb\nc", "").expect("copy succeeds");

            assert_eq!(summary.characters, 5);
            assert_eq!(summary.lines, 3);
        }

        #[test]
        fn system_clipboard_does_not_connect_when_constructed() {
            let sink = SystemClipboard::default();

            assert!(sink.handle.is_none());
            assert!(format!("{sink:?}").contains("uninitialised"));
        }
    }
}

/// Pasting into the previously focused window.
pub mod paste {
    use super::OutputError;
    use super::clipboard::{ClipboardSink, CopySummary, copy_transcript};

    /// Synthesises the paste shortcut into the previously focused window.
    ///
    /// The text travels through the clipboard, so this trait stays primitive
    /// and the injected fake is trivial.
    pub trait PasteSink: Send {
        /// Synthesises Ctrl+V (Cmd+V on macOS) into the focused window.
        fn paste(&mut self) -> Result<(), OutputError>;
    }

    /// Real paste backed by `enigo`.
    ///
    /// The input connection is created per call: it only exists for the
    /// duration of one keystroke and construction never touches the display.
    #[derive(Debug, Default, Clone, Copy)]
    pub struct SystemPaste;

    impl SystemPaste {
        #[must_use]
        pub fn new() -> Self {
            Self
        }
    }

    impl PasteSink for SystemPaste {
        fn paste(&mut self) -> Result<(), OutputError> {
            use enigo::{Direction, Enigo, Key, Keyboard as _, Settings};

            let mut enigo = Enigo::new(&Settings::default())
                .map_err(|error| OutputError::Paste(error.to_string()))?;
            enigo
                .key(paste_modifier(), Direction::Press)
                .map_err(|error| OutputError::Paste(error.to_string()))?;
            let clicked = enigo.key(Key::Unicode('v'), Direction::Click);
            // Release the modifier even when the keystroke itself failed, so a
            // failed paste cannot leave Ctrl (or Cmd) stuck down.
            let released = enigo.key(paste_modifier(), Direction::Release);
            clicked
                .and(released)
                .map_err(|error| OutputError::Paste(error.to_string()))
        }
    }

    /// The platform paste modifier: Cmd on macOS, Ctrl elsewhere.
    fn paste_modifier() -> enigo::Key {
        if cfg!(target_os = "macos") {
            enigo::Key::Meta
        } else {
            enigo::Key::Control
        }
    }

    /// Copies the transcript, then pastes it.
    ///
    /// Paste is attempted only when the copy succeeded. The returned summary
    /// describes what was copied.
    pub fn copy_and_paste(
        clipboard: &mut dyn ClipboardSink,
        paste: &mut dyn PasteSink,
        committed: &str,
        live: &str,
    ) -> Result<CopySummary, OutputError> {
        let summary = copy_transcript(clipboard, committed, live)?;
        paste.paste()?;
        Ok(summary)
    }

    #[cfg(test)]
    mod tests {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};

        use super::*;

        #[derive(Clone)]
        struct LoggingClipboard {
            log: Arc<Mutex<Vec<String>>>,
        }

        impl ClipboardSink for LoggingClipboard {
            fn copy(&mut self, text: &str) -> Result<(), OutputError> {
                self.log
                    .lock()
                    .expect("paste log is not poisoned")
                    .push(format!("copy:{text}"));
                Ok(())
            }
        }

        struct LoggingPaste {
            log: Arc<Mutex<Vec<String>>>,
        }

        impl PasteSink for LoggingPaste {
            fn paste(&mut self) -> Result<(), OutputError> {
                self.log
                    .lock()
                    .expect("paste log is not poisoned")
                    .push("paste".to_owned());
                Ok(())
            }
        }

        struct FailingClipboard {
            calls: Arc<AtomicUsize>,
        }

        impl ClipboardSink for FailingClipboard {
            fn copy(&mut self, _text: &str) -> Result<(), OutputError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Err(OutputError::Clipboard("no display".to_owned()))
            }
        }

        struct FailingPaste {
            calls: Arc<AtomicUsize>,
        }

        impl PasteSink for FailingPaste {
            fn paste(&mut self) -> Result<(), OutputError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Err(OutputError::Paste("no display".to_owned()))
            }
        }

        #[test]
        fn copies_before_pasting() {
            let log = Arc::new(Mutex::new(Vec::new()));
            let mut clipboard = LoggingClipboard {
                log: Arc::clone(&log),
            };
            let mut paste = LoggingPaste {
                log: Arc::clone(&log),
            };

            copy_and_paste(&mut clipboard, &mut paste, "Hello", "world").expect("paste succeeds");

            assert_eq!(
                log.lock().expect("paste log is not poisoned").clone(),
                vec!["copy:Hello world".to_owned(), "paste".to_owned()]
            );
        }

        #[test]
        fn copy_failure_skips_paste() {
            let clipboard_calls = Arc::new(AtomicUsize::new(0));
            let paste_calls = Arc::new(AtomicUsize::new(0));
            let mut clipboard = FailingClipboard {
                calls: Arc::clone(&clipboard_calls),
            };
            let mut paste = FailingPaste {
                calls: Arc::clone(&paste_calls),
            };

            let result = copy_and_paste(&mut clipboard, &mut paste, "Hello", "");

            assert!(matches!(result, Err(OutputError::Clipboard(_))));
            assert_eq!(clipboard_calls.load(Ordering::SeqCst), 1);
            assert_eq!(paste_calls.load(Ordering::SeqCst), 0);
        }

        #[test]
        fn empty_transcript_skips_copy_and_paste() {
            let clipboard_calls = Arc::new(AtomicUsize::new(0));
            let paste_calls = Arc::new(AtomicUsize::new(0));
            let mut clipboard = FailingClipboard {
                calls: Arc::clone(&clipboard_calls),
            };
            let mut paste = FailingPaste {
                calls: Arc::clone(&paste_calls),
            };

            let result = copy_and_paste(&mut clipboard, &mut paste, "  ", "\n");

            assert!(matches!(result, Err(OutputError::EmptyTranscript)));
            assert_eq!(clipboard_calls.load(Ordering::SeqCst), 0);
            assert_eq!(paste_calls.load(Ordering::SeqCst), 0);
        }

        #[test]
        fn paste_failure_propagates_after_copying() {
            let copied = Arc::new(Mutex::new(Vec::new()));
            let mut clipboard = LoggingClipboard {
                log: Arc::clone(&copied),
            };
            let paste_calls = Arc::new(AtomicUsize::new(0));
            let mut paste = FailingPaste {
                calls: Arc::clone(&paste_calls),
            };

            let result = copy_and_paste(&mut clipboard, &mut paste, "Hello", "");

            assert!(matches!(result, Err(OutputError::Paste(_))));
            assert_eq!(
                copied
                    .lock()
                    .expect("clipboard log is not poisoned")
                    .clone(),
                vec!["copy:Hello".to_owned()]
            );
            assert_eq!(paste_calls.load(Ordering::SeqCst), 1);
        }

        #[test]
        fn returns_the_copy_summary() {
            let log = Arc::new(Mutex::new(Vec::new()));
            let mut clipboard = LoggingClipboard {
                log: Arc::clone(&log),
            };
            let mut paste = LoggingPaste { log };

            let summary =
                copy_and_paste(&mut clipboard, &mut paste, "a\nb", "c").expect("paste succeeds");

            assert_eq!(
                summary,
                CopySummary {
                    characters: 5,
                    lines: 2
                }
            );
        }

        #[test]
        fn system_paste_is_zero_sized_and_does_not_connect() {
            let paste = SystemPaste::new();

            assert_eq!(std::mem::size_of_val(&paste), 0);
        }
    }
}

/// Saving transcripts as Markdown or plain text.
pub mod file {
    use std::ffi::OsString;
    use std::fs;
    use std::io::{self, Write as _};
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::OutputError;

    /// Session name length cap for the generated file stem.
    const MAX_STEM_CHARS: usize = 64;

    /// Prefix used when the session name sanitises to nothing.
    const FALLBACK_STEM: &str = "speechtt";

    /// Heading used when the title is blank.
    const FALLBACK_TITLE: &str = "SpeechTT";

    /// On-disk transcript format.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum TranscriptFormat {
        Markdown,
        Text,
    }

    impl TranscriptFormat {
        /// Maps a file extension (case-insensitive, without the dot).
        #[must_use]
        pub fn from_extension(extension: &str) -> Option<Self> {
            match extension.to_ascii_lowercase().as_str() {
                "md" => Some(Self::Markdown),
                "txt" => Some(Self::Text),
                _ => None,
            }
        }

        /// The canonical extension, without the dot.
        #[must_use]
        pub fn extension(self) -> &'static str {
            match self {
                Self::Markdown => "md",
                Self::Text => "txt",
            }
        }
    }

    /// What a successful save produced.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct SavedFile {
        pub path: PathBuf,
        pub bytes_written: usize,
    }

    /// Opens a saved transcript in the user's editor/viewer.
    pub trait FileOpener: Send {
        fn open(&mut self, path: &Path) -> Result<(), OutputError>;
    }

    /// Real opener: `xdg-open` on Linux, `open` on macOS, `start` on Windows.
    #[derive(Debug, Default, Clone, Copy)]
    pub struct SystemOpener;

    impl SystemOpener {
        #[must_use]
        pub fn new() -> Self {
            Self
        }
    }

    impl FileOpener for SystemOpener {
        fn open(&mut self, path: &Path) -> Result<(), OutputError> {
            let mut command = opener_command();
            command
                .arg(path)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            let mut child = command
                .spawn()
                .map_err(|error| OutputError::OpenFile(error.to_string()))?;
            // Reap the child without blocking the caller.
            std::thread::spawn(move || {
                let _ = child.wait();
            });
            Ok(())
        }
    }

    fn opener_command() -> Command {
        #[cfg(target_os = "windows")]
        {
            let mut command = Command::new("cmd");
            command.args(["/C", "start", ""]);
            command
        }
        #[cfg(target_os = "macos")]
        {
            Command::new("open")
        }
        #[cfg(all(unix, not(target_os = "macos")))]
        {
            Command::new("xdg-open")
        }
    }

    /// Writes `text` to exactly `path`, replacing whatever is there.
    ///
    /// The native save dialog already let the user confirm the destination
    /// (including replacing an existing file), so no timestamp and no
    /// collision suffix are invented here. `title` heads a Markdown document.
    /// Missing parent directories are created.
    ///
    /// # Errors
    ///
    /// [`OutputError::EmptyTranscript`] for blank text and [`OutputError::Write`]
    /// when the file cannot be written.
    pub fn save_transcript_to(
        path: &Path,
        title: &str,
        format: TranscriptFormat,
        text: &str,
    ) -> Result<SavedFile, OutputError> {
        let body = text.trim();
        if body.is_empty() {
            return Err(OutputError::EmptyTranscript);
        }

        let contents = render(title, format, body);
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|error| OutputError::Write(error.to_string()))?;
        }

        let bytes_written = write_replacing(path, contents.as_bytes())
            .map_err(|error| OutputError::Write(error.to_string()))?;

        Ok(SavedFile {
            path: path.to_path_buf(),
            bytes_written,
        })
    }

    /// Suggested file name for the native save dialog:
    /// `<sanitised-stem>-<YYYYMMDD-HHMMSS>.md`.
    ///
    /// A stem that sanitises to nothing falls back to `speechtt`.
    #[must_use]
    pub fn suggested_transcript_name(stem: &str, now: SystemTime) -> String {
        format!("{}.md", file_stem(stem, now))
    }

    /// `<stem>-<YYYYMMDD-HHMMSS>`, falling back to `speechtt-…`.
    fn file_stem(session_name: &str, now: SystemTime) -> String {
        let stem = sanitise_stem(session_name);
        let stem = if stem.is_empty() {
            FALLBACK_STEM.to_owned()
        } else {
            stem
        };
        format!("{stem}-{}", format_timestamp(now))
    }

    /// Trim, lower-case, collapse whitespace to `-`, keep `[a-z0-9._-]`,
    /// strip edge separators, then cap at [`MAX_STEM_CHARS`].
    fn sanitise_stem(session_name: &str) -> String {
        let mut out = String::with_capacity(session_name.len());
        for character in session_name.trim().to_lowercase().chars() {
            if character.is_whitespace() {
                push_separator(&mut out, '-');
            } else if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                if character == '-' {
                    push_separator(&mut out, '-');
                } else {
                    out.push(character);
                }
            }
        }

        let trimmed = out.trim_matches(|character| matches!(character, '-' | '.'));
        let mut stem: String = trimmed.chars().take(MAX_STEM_CHARS).collect();
        while stem.ends_with(['-', '.']) {
            stem.pop();
        }
        stem
    }

    /// Push one separator unless the previous character is already one.
    fn push_separator(out: &mut String, separator: char) {
        if !out.is_empty() && !out.ends_with(separator) {
            out.push(separator);
        }
    }

    /// UTC `YYYYMMDD-HHMMSS`; timestamps before the epoch clamp to 1970.
    fn format_timestamp(now: SystemTime) -> String {
        let seconds = now
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs() as i64)
            .unwrap_or_default();
        let days = seconds.div_euclid(86_400);
        let seconds_of_day = seconds.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        let hour = seconds_of_day / 3_600;
        let minute = (seconds_of_day % 3_600) / 60;
        let second = seconds_of_day % 60;
        format!("{year:04}{month:02}{day:02}-{hour:02}{minute:02}{second:02}")
    }

    /// Howard Hinnant's `civil_from_days`: days since 1970-01-01 to (y, m, d).
    fn civil_from_days(days: i64) -> (i64, u32, u32) {
        let shifted = days + 719_468;
        let era = if shifted >= 0 {
            shifted
        } else {
            shifted - 146_096
        } / 146_097;
        let day_of_era = shifted - era * 146_097; // [0, 146096]
        let year_of_era =
            (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
        let year = year_of_era + era * 400;
        let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
        let mp = (5 * day_of_year + 2) / 153;
        let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        (if month <= 2 { year + 1 } else { year }, month, day)
    }

    /// Markdown gets a heading and a blank line; both formats get exactly one
    /// trailing newline.
    fn render(title: &str, format: TranscriptFormat, body: &str) -> String {
        match format {
            TranscriptFormat::Markdown => format!("# {}\n\n{body}\n", display_title(title)),
            TranscriptFormat::Text => format!("{body}\n"),
        }
    }

    /// Title as a single-line heading, or the product name if blank.
    fn display_title(title: &str) -> String {
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        if title.is_empty() {
            FALLBACK_TITLE.to_owned()
        } else {
            title
        }
    }

    /// Writes `contents` through a sibling temp file, then moves it into place,
    /// replacing `target` when it exists.
    fn write_replacing(target: &Path, contents: &[u8]) -> io::Result<usize> {
        let temporary = temporary_path(target);
        let mut file = fs::File::create(&temporary)?;
        let written = file.write_all(contents).and_then(|()| file.sync_all());
        drop(file);
        if let Err(error) = written {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        // `rename` replaces on Unix but fails on Windows; removing first keeps
        // the dialog's "Replace" working on every platform.
        if target.exists()
            && let Err(error) = fs::remove_file(target)
        {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        if let Err(error) = fs::rename(&temporary, target) {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        Ok(contents.len())
    }

    /// `<target>.tmp`, preserving the target's full file name.
    fn temporary_path(target: &Path) -> PathBuf {
        let mut name: OsString = target
            .file_name()
            .map_or_else(OsString::new, |name| name.to_owned());
        name.push(".tmp");
        target.with_file_name(name)
    }

    #[cfg(test)]
    mod tests {
        use std::env;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Mutex};
        use std::time::Duration;

        use super::*;

        /// `2026-10-27T14:03:05Z`.
        const FIXED_SECONDS: u64 = 1_793_109_785;
        const FIXED_STAMP: &str = "20261027-140305";

        fn fixed_now() -> SystemTime {
            UNIX_EPOCH + Duration::from_secs(FIXED_SECONDS)
        }

        /// Unique directory under the system temp dir, removed on drop.
        struct TempDir(PathBuf);

        impl TempDir {
            fn new(label: &str) -> Self {
                static COUNTER: AtomicUsize = AtomicUsize::new(0);
                let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
                let path = env::temp_dir().join(format!(
                    "speechtt-output-{}-{unique}-{label}",
                    std::process::id()
                ));
                fs::create_dir_all(&path).expect("temp dir can be created");
                Self(path)
            }

            fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        fn file_names(directory: &Path) -> Vec<String> {
            let mut names: Vec<String> = fs::read_dir(directory)
                .expect("directory is readable")
                .map(|entry| {
                    entry
                        .expect("entry is readable")
                        .file_name()
                        .to_string_lossy()
                        .into_owned()
                })
                .collect();
            names.sort();
            names
        }

        #[test]
        fn markdown_has_heading_blank_line_and_one_trailing_newline() {
            let directory = TempDir::new("markdown");
            let target = directory.path().join("my-talk.md");

            let saved = save_transcript_to(
                &target,
                "My Talk",
                TranscriptFormat::Markdown,
                "  hello\nworld\n\n",
            )
            .expect("markdown is saved");

            assert_eq!(saved.path, target);
            let contents = fs::read_to_string(&saved.path).expect("saved file is readable");
            assert_eq!(contents, "# My Talk\n\nhello\nworld\n");
            assert_eq!(saved.bytes_written, contents.len());
        }

        #[test]
        fn text_format_is_the_trimmed_body_only() {
            let directory = TempDir::new("text");
            let target = directory.path().join("notes.txt");

            let saved = save_transcript_to(
                &target,
                "My Talk",
                TranscriptFormat::Text,
                "\n  hello\nworld  \n",
            )
            .expect("text is saved");

            assert_eq!(saved.path, target);
            let contents = fs::read_to_string(&saved.path).expect("saved file is readable");
            assert_eq!(contents, "hello\nworld\n");
        }

        #[test]
        fn empty_transcript_is_rejected_and_nothing_is_written() {
            let directory = TempDir::new("empty");
            let target = directory.path().join("notes.md");

            let result = save_transcript_to(&target, "Notes", TranscriptFormat::Markdown, " \n\t ");

            assert!(matches!(result, Err(OutputError::EmptyTranscript)));
            assert!(!target.exists());
            assert!(file_names(directory.path()).is_empty());
        }

        #[test]
        fn the_exact_path_is_written_without_timestamps_or_suffixes() {
            let directory = TempDir::new("exact");
            let target = directory.path().join("weekly sync.md");

            let first =
                save_transcript_to(&target, "Weekly Sync", TranscriptFormat::Markdown, "one")
                    .expect("first save");
            let second =
                save_transcript_to(&target, "Weekly Sync", TranscriptFormat::Markdown, "two")
                    .expect("second save");

            assert_eq!(first.path, target);
            assert_eq!(second.path, target);
            assert_eq!(file_names(directory.path()), vec!["weekly sync.md"]);
            assert_eq!(
                fs::read_to_string(&target).expect("saved file"),
                "# Weekly Sync\n\ntwo\n"
            );
        }

        #[test]
        fn an_existing_file_is_replaced() {
            let directory = TempDir::new("replace");
            let target = directory.path().join("notes.md");
            fs::write(&target, "original").expect("fixture is written");

            let saved = save_transcript_to(&target, "Notes", TranscriptFormat::Markdown, "hello")
                .expect("replace succeeds");

            assert_eq!(saved.path, target);
            assert_eq!(
                fs::read_to_string(&target).expect("saved file"),
                "# Notes\n\nhello\n"
            );
            assert_eq!(file_names(directory.path()), vec!["notes.md"]);
        }

        #[test]
        fn parent_directories_are_created() {
            let directory = TempDir::new("parents");
            let target = directory
                .path()
                .join("a")
                .join("b")
                .join("c")
                .join("deep.txt");

            let saved = save_transcript_to(&target, "Deep", TranscriptFormat::Text, "hello")
                .expect("nested save succeeds");

            assert_eq!(saved.path, target);
            assert!(target.exists());
        }

        #[test]
        fn temp_files_are_removed_after_success() {
            let directory = TempDir::new("tmp-cleanup");
            let target = directory.path().join("clean.txt");

            save_transcript_to(&target, "Clean", TranscriptFormat::Text, "hello")
                .expect("save succeeds");

            assert_eq!(file_names(directory.path()), vec!["clean.txt"]);
        }

        #[test]
        fn write_failure_keeps_the_previous_file_and_leaves_no_artefacts() {
            let directory = TempDir::new("write-failure");
            let target = directory.path().join("notes.md");
            fs::write(&target, "original").expect("fixture is written");
            // Occupy the temp path with a directory so `File::create` fails.
            let blocker = directory.path().join("notes.md.tmp");
            fs::create_dir_all(&blocker).expect("blocking directory is created");

            let result = save_transcript_to(&target, "Notes", TranscriptFormat::Markdown, "hello");

            assert!(matches!(result, Err(OutputError::Write(_))));
            assert_eq!(fs::read_to_string(&target).expect("fixture"), "original");
            assert!(blocker.exists(), "the blocker is not ours to remove");
        }

        #[test]
        fn file_stem_sanitises_spaces_and_case() {
            assert_eq!(
                sanitise_stem("  My   Great  Talk "),
                "my-great-talk".to_owned()
            );
        }

        #[test]
        fn file_stem_strips_edge_separators() {
            assert_eq!(sanitise_stem("--Hello.. "), "hello".to_owned());
        }

        #[test]
        fn file_stem_drops_path_separators_and_traversal() {
            let stem = sanitise_stem("Notes/../../etc/passwd");

            assert!(!stem.contains('/'), "stem must not contain a separator");
            assert!(!stem.contains('\\'), "stem must not contain a separator");
            assert!(!stem.starts_with('.'), "stem must not start with a dot");
            assert!(stem.starts_with("notes"));
        }

        #[test]
        fn file_stem_falls_back_for_reserved_names() {
            for name in ["..", ".", "../../", "   ", "\0"] {
                assert_eq!(sanitise_stem(name), String::new(), "name {name:?}");
            }
        }

        #[test]
        fn file_stem_removes_nul_and_control_characters() {
            assert_eq!(sanitise_stem("a\0b\nc"), "ab-c".to_owned());
        }

        #[test]
        fn file_stem_keeps_ascii_from_unicode_names() {
            assert_eq!(sanitise_stem("Café Notes"), "caf-notes".to_owned());
        }

        #[test]
        fn file_stem_caps_the_length() {
            let stem = sanitise_stem(&"a".repeat(100));

            assert_eq!(stem, "a".repeat(64));
        }

        #[test]
        fn file_stem_cap_does_not_leave_a_trailing_separator() {
            let name = format!("{} b", "a".repeat(63));
            let stem = sanitise_stem(&name);

            assert_eq!(stem, "a".repeat(63));
        }

        #[test]
        fn suggested_name_uses_the_stem_and_timestamp() {
            assert_eq!(
                suggested_transcript_name("Weekly  Sync", fixed_now()),
                format!("weekly-sync-{FIXED_STAMP}.md")
            );
            assert_eq!(
                suggested_transcript_name("meeting", fixed_now()),
                format!("meeting-{FIXED_STAMP}.md")
            );
        }

        #[test]
        fn suggested_name_falls_back_when_the_stem_sanitises_to_nothing() {
            for stem in ["  ", "../../", "..", ".", "\0"] {
                assert_eq!(
                    suggested_transcript_name(stem, fixed_now()),
                    format!("speechtt-{FIXED_STAMP}.md"),
                    "stem {stem:?}"
                );
            }
        }

        #[test]
        fn suggested_name_strips_path_separators_and_traversal() {
            let name = suggested_transcript_name("Notes/../../etc/passwd", fixed_now());

            assert!(!name.contains('/'), "name must not contain a separator");
            assert!(!name.contains('\\'), "name must not contain a separator");
            assert!(!name.starts_with('.'), "name must not start with a dot");
            assert!(name.starts_with("notes"), "unexpected name {name}");
            assert!(name.ends_with(&format!("-{FIXED_STAMP}.md")));
        }

        #[test]
        fn suggested_name_keeps_ascii_from_unicode_stems() {
            assert_eq!(
                suggested_transcript_name("Café Notes", fixed_now()),
                format!("caf-notes-{FIXED_STAMP}.md")
            );
        }

        #[test]
        fn suggested_name_caps_the_stem_length() {
            let name = suggested_transcript_name(&"a".repeat(100), fixed_now());

            assert_eq!(name, format!("{}-{FIXED_STAMP}.md", "a".repeat(64)));
        }

        #[test]
        fn timestamps_are_utc_zero_padded_and_handle_leap_days() {
            let leap_day = UNIX_EPOCH + Duration::from_secs(1_709_164_800);

            assert_eq!(
                suggested_transcript_name("Leap", leap_day),
                "leap-20240229-000000.md"
            );
        }

        #[test]
        fn epoch_timestamp_is_formatted() {
            assert_eq!(
                suggested_transcript_name("Epoch", UNIX_EPOCH),
                "epoch-19700101-000000.md"
            );
        }

        #[test]
        fn blank_title_falls_back_to_the_product_name() {
            let directory = TempDir::new("blank-title");
            let target = directory.path().join("blank.md");

            save_transcript_to(&target, "   ", TranscriptFormat::Markdown, "hello")
                .expect("save succeeds");

            assert_eq!(
                fs::read_to_string(&target).expect("saved file"),
                "# SpeechTT\n\nhello\n"
            );
        }

        #[test]
        fn markdown_title_is_collapsed_to_one_line() {
            let directory = TempDir::new("one-line-title");
            let target = directory.path().join("notes.md");

            save_transcript_to(
                &target,
                "Weekly\n  Sync",
                TranscriptFormat::Markdown,
                "hello",
            )
            .expect("save succeeds");

            assert_eq!(
                fs::read_to_string(&target).expect("saved file"),
                "# Weekly Sync\n\nhello\n"
            );
        }

        #[test]
        fn from_extension_is_case_insensitive_and_rejects_others() {
            assert_eq!(
                TranscriptFormat::from_extension("md"),
                Some(TranscriptFormat::Markdown)
            );
            assert_eq!(
                TranscriptFormat::from_extension("MD"),
                Some(TranscriptFormat::Markdown)
            );
            assert_eq!(
                TranscriptFormat::from_extension("txt"),
                Some(TranscriptFormat::Text)
            );
            assert_eq!(
                TranscriptFormat::from_extension("TXT"),
                Some(TranscriptFormat::Text)
            );
            for extension in ["pdf", "", "mdx", "markdown"] {
                assert_eq!(TranscriptFormat::from_extension(extension), None);
            }
        }

        #[test]
        fn format_extensions_round_trip() {
            for format in [TranscriptFormat::Markdown, TranscriptFormat::Text] {
                assert_eq!(
                    TranscriptFormat::from_extension(format.extension()),
                    Some(format)
                );
            }
            assert_eq!(TranscriptFormat::Markdown.extension(), "md");
            assert_eq!(TranscriptFormat::Text.extension(), "txt");
        }

        struct MockOpener {
            opened: Arc<Mutex<Vec<PathBuf>>>,
            fail: bool,
        }

        impl FileOpener for MockOpener {
            fn open(&mut self, path: &Path) -> Result<(), OutputError> {
                self.opened
                    .lock()
                    .expect("opener log is not poisoned")
                    .push(path.to_path_buf());
                if self.fail {
                    Err(OutputError::OpenFile("mock opener failed".to_owned()))
                } else {
                    Ok(())
                }
            }
        }

        #[test]
        fn file_opener_receives_the_exact_path() {
            let opened = Arc::new(Mutex::new(Vec::new()));
            let mut opener = MockOpener {
                opened: Arc::clone(&opened),
                fail: false,
            };
            let path = Path::new("/tmp/transcript.md");

            opener.open(path).expect("mock opener succeeds");

            assert_eq!(
                opened.lock().expect("opener log is not poisoned").clone(),
                vec![path.to_path_buf()]
            );
        }

        #[test]
        fn file_opener_failure_is_typed() {
            let opened = Arc::new(Mutex::new(Vec::new()));
            let mut opener = MockOpener { opened, fail: true };

            let result = opener.open(Path::new("/tmp/transcript.md"));

            assert!(matches!(result, Err(OutputError::OpenFile(_))));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::OutputError;

    #[test]
    fn error_messages_are_stable_and_typed() {
        assert_eq!(
            OutputError::EmptyTranscript.to_string(),
            "transcript is empty"
        );
        assert_eq!(
            OutputError::Clipboard("no display".to_owned()).to_string(),
            "clipboard unavailable: no display"
        );
        assert_eq!(
            OutputError::Paste("no display".to_owned()).to_string(),
            "could not synthesise paste: no display"
        );
        assert_eq!(
            OutputError::UnsupportedFormat("pdf".to_owned()).to_string(),
            "unsupported transcript format: pdf"
        );
        assert_eq!(
            OutputError::Write("disk full".to_owned()).to_string(),
            "could not write transcript: disk full"
        );
        assert_eq!(
            OutputError::OpenFile("missing xdg-open".to_owned()).to_string(),
            "could not open file: missing xdg-open"
        );
    }

    #[test]
    fn errors_never_carry_transcript_text() {
        let transcript = "secret dictation";
        let error = OutputError::EmptyTranscript;
        let message = error.to_string();

        assert!(!message.contains(transcript));
    }
}
