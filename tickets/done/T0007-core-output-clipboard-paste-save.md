# T0007 — Core: Clipboard, Paste and Save-to-File Adapters

- **Track**: Rust core (`crates/speechtt-core`)
- **Wave**: 1 (independent)
- **Depends on**: `session::{OutputTarget, FinishedSession}` (T0001)
- **Blocks**: T0009 (end-to-end wiring)
- **PRODUCT.md**: Phase 5
- **Owner**: coding sub-agent (deepseek-v4.1-flash), supervised by CTO
- **Started**: 2026-10-27

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

- [x] **T7.1 — Clipboard tests**: exact content for committed-only,
  live-only and mixed transcripts; single space join; trimming; empty →
  `EmptyTranscript` and sink untouched; `CopySummary` counts.
- [x] **T7.2 — Paste tests**: copy-then-paste order via a recording mock;
  copy failure short-circuits paste; paste failure propagates.
- [x] **T7.3 — Filename/sanitisation tests**: spaces/case/`/`/`..`/NUL/
  unicode handling; 64-char cap; empty fallback; collision avoidance
  (`-2` suffix); `.md` vs `.txt` selection and `UnsupportedFormat` for
  `.pdf`.
- [x] **T7.4 — File content tests**: markdown heading + trimmed body + one
  trailing newline; text body only; `save_transcript_at` deterministic name;
  parent directory created; previous file preserved on injected write
  failure.
- [x] **T7.5 — `FileOpener` contract test**: `MockOpener` records the path;
  opener failure maps to `OutputError::OpenFile`.
- [x] **T7.6 — Implementation**: add `arboard` + `enigo`, implement
  `output.rs`, register the module.
- [x] **T7.7 — Gates**: `cargo fmt -- --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test` clean; no test
  accesses a real clipboard, X11/Wayland input, or launches an editor.

## Acceptance Criteria

- Copying a memory take yields byte-exact transcript text on the clipboard.
- Saving writes `<sanitised-name>-<timestamp>.md|.txt` atomically, never
  overwriting an existing file, and rejects other extensions.
- Open File delegates through `FileOpener` and reports typed failures.
- All tests run headless with injected fakes.

## Implementation Notes

**Status**: implemented, all gates green, awaiting CTO review (ticket left in
`tickets/in_progress/`).
**Date**: 2026-10-27.

### Files touched

- `crates/speechtt-core/Cargo.toml` — appended `arboard = { version = "3",
  default-features = false }` and `enigo = "0.6"`; every pre-existing line is
  unchanged. `default-features = false` keeps arboard's image payload out of
  the build; only text clipboard support is needed. enigo keeps its default
  `x11rb` backend (pure Rust, no `libxdo` system dependency).
- `crates/speechtt-core/src/lib.rs` — appended `pub mod output;`; no existing
  line changed.
- `crates/speechtt-core/src/output.rs` — new single-file module with the public
  submodules `output::clipboard`, `output::paste`, `output::file` and 47 unit
  tests.
- `Cargo.lock` — regenerated by cargo for the two new dependencies. This is the
  only file outside the stated scope; it is unavoidable when adding crates and
  no unrelated entry was removed.

### Design decisions

- **Clipboard composition.** `compose` trims each part first and joins them
  with one space when both are non-empty, so `"Hello "` + `" world"` yields
  `"Hello world"` (never a double space). `copy_transcript` rejects an empty
  result before `ClipboardSink::copy` is called, so the sink can be asserted
  untouched. `CopySummary.characters` counts Unicode scalars (not bytes);
  `lines` uses `str::lines`.
- **System clipboard.** `SystemClipboard` stores `Option<arboard::Clipboard>`
  and creates it on the first `copy`; `Default`/`new` and `Debug` never open an
  X11/Wayland connection, which keeps construction safe in tests. arboard
  failures map to `OutputError::Clipboard` without echoing the transcript.
- **System paste.** `SystemPaste` is zero-sized and creates the enigo
  connection per call. The modifier is released even when the `v` keystroke
  fails, so a failed paste cannot leave Ctrl stuck. The trait documents Ctrl+V;
  like real applications, macOS uses Cmd+V (`Key::Meta`) because Ctrl+V is not
  the system paste shortcut there — the docs in `paste` state this.
- **Timestamps.** `save_transcript_at` formats `SystemTime` as UTC
  `YYYYMMDD-HHMMSS` with Howard Hinnant's civil-from-days algorithm, so no date
  crate is added. Pre-epoch clock values clamp to 1970-01-01. The public
  `save_transcript` delegates with `SystemTime::now()`.
- **Sanitisation.** Names are trimmed, lower-cased and filtered to
  `[a-z0-9._-]`; whitespace and `-` runs collapse to one `-`, edge `-`/`.` are
  stripped, then the stem is capped at 64 chars (trailing separators are
  stripped again after the cap). `/`, `\`, NUL, control and non-ASCII
  characters are dropped rather than causing an error, so traversal is
  impossible by construction: the result is always one path component.
  Empty/`.`/`..` results fall back to `speechtt-<timestamp>`.
- **Atomic save.** The target is written as `<target>.tmp`, `sync_all`ed and
  renamed. A free name is chosen against existing files (`-2`, `-3`, …); the
  target is re-checked after the temp write and `AlreadyExists` retries the
  next suffix, so a file created concurrently is never replaced. Any write
  error removes the temp file and preserves the previous target.
- **Content.** Markdown is `# <title>\n\n<body>\n`, Text is `<body>\n`; the
  body is trimmed so both formats end with exactly one newline. The heading
  collapses whitespace in the session name and falls back to `SpeechTT` when
  the name is blank.
- **`save_for_session`.** `OutputTarget::File` supplies the directory and
  extension; `OutputTarget::Memory` has no directory, and the ticket's error
  enum has no dedicated variant, so it returns
  `OutputError::Write("session output target is memory, not a file")`.
  `~` is not expanded here (no `$HOME` access in core); callers pass an
  expanded path.
- **`SystemOpener`.** Spawns `xdg-open`/`open`/`cmd /C start` with null stdio,
  returns immediately, and reaps the child on a detached thread. Spawn failure
  maps to `OutputError::OpenFile`.

### Test coverage

All tests are hermetic: clipboard and paste use `Arc<Mutex<Vec<_>>>` recording
fakes, file tests use unique `std::env::temp_dir()` directories removed on
`Drop`, and no test constructs `SystemClipboard` (only checks that `Default`
does not connect), `SystemPaste` (only checks its size) or `SystemOpener`.

| Area | Tests | Highlights |
|---|---:|---|
| `clipboard` | 9 | committed-only, live-only, mixed single-space join, trimming, newline preservation, empty rejected with sink untouched, copy failure propagation, Unicode scalar and line counts, lazy system clipboard |
| `paste` | 6 | copy-before-paste ordering, copy failure short-circuits paste, empty transcript touches neither sink, paste failure propagates after copying, summary passthrough, zero-sized system paste |
| `file` | 30 | Markdown/Text rendering, deterministic timestamp, UTC/epoch/leap-day formatting, directory creation, temp-file cleanup, empty transcript, sanitisation (spaces, case, separators, `..`, NUL, Unicode, 64-char cap, edge separators), fallback names, `-2`/`-3` collisions, no overwrite, injected write failure, `File`/`Memory` targets, `.md`/`.txt`/`.pdf` handling, `MockOpener` path recording and typed failure |
| errors | 2 | stable `thiserror` messages, no transcript text in errors |

### TDD evidence

- **Red** — with `copy_transcript`, `copy_and_paste`, `sanitise_stem` and
  `save_transcript_at` stubbed with `todo!()`:
  `cargo test -p speechtt-core output` →
  `test result: FAILED. 9 passed; 37 failed; 0 ignored; 116 filtered out`.
- **Green** — after implementing `output.rs`:
  `cargo test -p speechtt-core output` → all 47 output tests pass.

### Quality gates (exact)

```text
$ cargo fmt --all -- --check
(no output)                                   exit 0

$ cargo clippy --all-targets -- -D warnings
    Checking speechtt-core v0.1.0 (/home/zipang/Workspace/projects/speechtt/crates/speechtt-core)
    Checking speechtt-ui v0.1.0 (/home/zipang/Workspace/projects/speechtt/crates/speechtt-ui)
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.88s
                                              exit 0

$ cargo test
speechtt-core lib        164 passed; 0 failed
speechtt-core bridge       2 passed; 0 failed
speechtt-core session     11 passed; 0 failed
speechtt-ui lib           14 passed; 0 failed
speechtt-ui bin            0 passed; 0 failed
speechtt-ui dialogs        8 passed; 0 failed
speechtt-ui session_flow  17 passed; 0 failed
speechtt-ui theme_contract 4 passed; 0 failed
speechtt-ui widget         2 passed; 0 failed
doc-tests                  0 passed; 0 failed
                                              exit 0
```

The CLI equivalent of the ticket's gate, `cargo fmt -- --check`, is covered by
`cargo fmt --all -- --check` (workspace root; no package-specific config).
