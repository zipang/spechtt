# T0007 — Core: Clipboard, Paste and Save-to-File Adapters

- **Track**: Rust core (`crates/speechtt-core`)
- **Wave**: 1 (independent)
- **Depends on**: `session::{OutputTarget, FinishedSession}` (T0001)
- **Blocks**: T0009 (end-to-end wiring)
- **PRODUCT.md**: Phase 5

## Context & Goal

The Stopped screen offers **Copy**, **Save** and **Open File**, but all three
handlers are stubs. This ticket adds the real output adapters to
`speechtt-core`, each behind a small trait so UI and integration tests can use
deterministic fakes and so CI never needs a clipboard, an input-injection
daemon, or an editor.

## Scope Boundary

Only touch:

```text
crates/speechtt-core/Cargo.toml      # arboard, enigo (append-only)
crates/speechtt-core/src/lib.rs      # `pub mod output;` (append-only)
crates/speechtt-core/src/output.rs   # new (single module, submodules inside)
```

Do **not** touch `crates/speechtt-ui/` or `src/stt/`.

> `Cargo.toml` / `lib.rs` are shared with T0005/T0006; append only.

## 1. Module layout (`output.rs`)

```rust
pub mod clipboard;
pub mod paste;
pub mod file;
```

(All three in one file with private inner modules is also acceptable; keep
the public paths above.)

## 2. Clipboard (`output::clipboard`)

```rust
pub trait ClipboardSink: Send {
    fn copy(&mut self, text: &str) -> Result<(), OutputError>;
}

pub struct SystemClipboard; // arboard-backed, lazily initialised

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopySummary { pub characters: usize, pub lines: usize }

pub fn copy_transcript(
    sink: &mut dyn ClipboardSink,
    committed: &str,
    live: &str,
) -> Result<CopySummary, OutputError>;
```

- `copy_transcript` joins `committed` and `live` with a single space when both
  are non-empty, trims the result, and copies it **exactly**.
- Empty transcript → `OutputError::EmptyTranscript`; the clipboard is not
  touched.
- `CopySummary` lets the UI show "Copied 214 characters" without re-measuring.
- `SystemClipboard` maps `arboard` failures to `OutputError::Clipboard`.

## 3. Paste (`output::paste`)

```rust
pub trait PasteSink: Send {
    /// Synthesises Ctrl+V into the previously focused window.
    fn paste(&mut self) -> Result<(), OutputError>;
}

pub struct SystemPaste; // enigo-backed

pub fn copy_and_paste(
    clipboard: &mut dyn ClipboardSink,
    paste: &mut dyn PasteSink,
    committed: &str,
    live: &str,
) -> Result<CopySummary, OutputError>;
```

- `copy_and_paste` copies first; paste is attempted only when the copy
  succeeded.
- `PasteSink` is intentionally primitive (no text argument) because the text
  travels through the clipboard. This keeps the injected fake trivial and
  mirrors real auto-paste behaviour.
- Errors are typed: `OutputError::Paste`, `OutputError::UnsupportedPlatform`.
- Auto-paste is opt-in (`Settings::auto_paste`, T0006); this ticket only
  provides the capability and does not call it from the UI.

## 4. File save (`output::file`)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TranscriptFormat { Markdown, Text }

impl TranscriptFormat {
    pub fn from_extension(ext: &str) -> Option<Self>;
    pub fn extension(self) -> &'static str;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedFile { pub path: PathBuf, pub bytes_written: usize }

pub trait FileOpener: Send {
    fn open(&mut self, path: &Path) -> Result<(), OutputError>;
}

pub struct SystemOpener; // xdg-open on Linux, `open` elsewhere

pub fn save_transcript(
    directory: &Path,
    session_name: &str,
    format: TranscriptFormat,
    text: &str,
) -> Result<SavedFile, OutputError>;

pub fn save_for_session(
    session: &FinishedSession,
    text: &str,
) -> Result<SavedFile, OutputError>; // maps OutputTarget::File
```

Rules:

- **Filename**: `<sanitised-session-name>-<YYYYMMDD-HHMMSS>.<ext>` for
  explicit session names; when the name sanitises to empty, fall back to
  `speechtt-<YYYYMMDD-HHMMSS>.<ext>`.
- **Sanitisation**: trim, collapse whitespace to `-`, lower-case, keep
  `[a-z0-9._-]`, strip leading/trailing `-`/`.`, cap the stem at 64 chars.
  Reject path separators, `..`, NUL and reserved names (`.`, `..`) — never
  allow escaping `directory`.
- **Content**: Markdown gets a `# <session name>` heading and a blank line,
  then the trimmed transcript; Text gets the trimmed transcript only. Both
  end with exactly one trailing newline.
- **Atomicity**: write to `<target>.tmp`, `sync_all`, rename. Existing files
  are **never** silently overwritten: append `-2`, `-3`, … until free.
- **Validation**: unsupported extension → `OutputError::UnsupportedFormat`
  (replaces the previous stub behaviour); empty transcript →
  `OutputError::EmptyTranscript`; missing directory is created.
- **Timestamp injection**: `save_transcript_at(..., now: SystemTime)` exists
  so tests assert deterministic names; the public function delegates to it.
- `SystemOpener::open` maps spawn failures to `OutputError::OpenFile` and
  never blocks on the child process.

## 5. Errors

```rust
#[derive(Debug, thiserror::Error)]
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
```

No secret or full transcript content in error messages.

## 6. TDD Tasks

- [ ] **T7.1 — Clipboard tests**: exact content for committed-only,
  live-only and mixed transcripts; single space join; trimming; empty →
  `EmptyTranscript` and sink untouched; `CopySummary` counts.
- [ ] **T7.2 — Paste tests**: copy-then-paste order via a recording mock;
  copy failure short-circuits paste; paste failure propagates.
- [ ] **T7.3 — Filename/sanitisation tests**: spaces/case/`/`/`..`/NUL/
  unicode handling; 64-char cap; empty fallback; collision avoidance
  (`-2` suffix); `.md` vs `.txt` selection and `UnsupportedFormat` for
  `.pdf`.
- [ ] **T7.4 — File content tests**: markdown heading + trimmed body + one
  trailing newline; text body only; `save_transcript_at` deterministic name;
  parent directory created; previous file preserved on injected write
  failure.
- [ ] **T7.5 — `FileOpener` contract test**: `MockOpener` records the path;
  opener failure maps to `OutputError::OpenFile`.
- [ ] **T7.6 — Implementation**: add `arboard` + `enigo`, implement
  `output.rs`, register the module.
- [ ] **T7.7 — Gates**: `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test` clean; no test
  accesses a real clipboard, X11/Wayland input, or launches an editor.

## Acceptance Criteria

- Copying a memory take yields byte-exact transcript text on the clipboard.
- Saving writes `<sanitised-name>-<timestamp>.md|.txt` atomically, never
  overwriting an existing file, and rejects other extensions.
- Open File delegates through `FileOpener` and reports typed failures.
- All tests run headless with injected fakes.
