# T0024 — Accept only media files for file transcription

- **Track**: Rust core + Rust UI
- **Wave**: 2 (integration)
- **Depends on**: T0015 (file transcription), T0023 (file entry order and the
  pending file)
- **Blocks**: nothing
- **References**: `AGENTS.md` §2 (core/UI boundary), `crates/speechtt-core/src/output.rs`
  (`TranscriptFormat::from_extension`, the existing extension-table pattern),
  `src/stt/file-input.ts` (ffmpeg is the final decoder)

## Context & Goal

The file picker accepts anything. A user who picks a PDF, a `.txt` or a random
binary gets a file take that starts, decodes nothing, and fails somewhere in the
worker with an ffmpeg error. Even with T0023 in place, the failure arrives after
a model has been chosen — far too late, and addressed to the wrong layer.

The restriction cannot be applied inside the native dialog: `gpui-kit 0.8`'s
`PathPromptOptions` has only `files`/`directories`/`multiple`/`prompt`, and the
Linux portal call passes no `filters`. Restricting that would mean forking the
UI framework, which this ticket explicitly does not do.

**Done means:** a picked path is validated against one shared media allowlist
before it is accepted, a non-media file is refused immediately with an explicit
message, and the user lands back on the welcome screen with no pending file and
no started take.

## Scope Boundary

May create or modify:

- `crates/speechtt-core/src/media.rs` — **new**: the extension allowlist and
  `is_media_path` / `is_media_extension`.
- `crates/speechtt-core/src/lib.rs` — **append-only** module registration.
- `crates/speechtt-ui/src/controller.rs` — the picker-path guard, the new
  `AppError` variant and its message/action mapping.
- `crates/speechtt-ui/src/diagnostics.rs` — append-only `variant_name` arm.
- `crates/speechtt-ui/src/app.rs` — the picker's post-selection branch.
- `crates/speechtt-ui/tests/{session_flow,error_ux}.rs`,
  `crates/speechtt-core/src/media.rs` unit tests.
- `tickets/README.md` — board row.

Do **not** touch: `src/stt/**` (the worker's `unsupported_file` /
`decode_failed` handling already covers a corrupted or mislabelled file),
`DESIGN.md`, `design-tokens.css`, `tokens.rs`, the T0021 footer, the T0022 take
screen, or `Cargo.toml` (no new dependency, no `[patch]`). No new component,
colour or token. No layout or geometry test.

## Contract

### 1. One media allowlist in `speechtt-core`

```rust
/// Extensions accepted for file transcription, lowercase, without the dot.
pub const MEDIA_EXTENSIONS: &[&str] = &[
    // audio
    "wav", "mp3", "m4a", "aac", "flac", "ogg", "oga", "opus", "wma", "aiff", "aif", "amr",
    // video
    "mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "flv", "mpeg", "mpg", "3gp", "ts",
];

/// True when `extension` (with or without a leading dot) is an accepted media
/// extension. Case-insensitive; never panics on non-ASCII input.
#[must_use]
pub fn is_media_extension(extension: &str) -> bool;

/// True when `path` carries an accepted media extension.
#[must_use]
pub fn is_media_path(path: &Path) -> bool;
```

Rules:

- Matching is case-insensitive (`MP4`, `Mp4` accepted).
- A missing extension, an empty extension, a trailing-dot name (`song.`) or a
  directory-shaped path with no file name is **not** media.
- A dotted stem keeps the last extension only (`archive.tar.mp3` ⇒ `mp3`).
- The list is a fast, honest first gate, not a promise about file contents:
  ffmpeg stays the authority for a file whose bytes are not what its extension
  claims, and that keeps failing through the existing `unsupported_file` /
  `decode_failed` path.
- No I/O: the function reads the path string only, so it is hermetic and
  usable on a path that does not exist yet.

### 2. The picker accepts a media path, refuses anything else

- `SessionController::set_pending_file(path)` gains the guard: a non-media path
  returns `Err(AppError::UnsupportedFileType)`, stores no pending file and
  leaves the view on `ViewState::Mode` or `Form` exactly where it was.
- `start_file_session(path)` keeps a defensive copy of the same guard, because
  it is reachable from tests and future callers: a non-media path returns
  `Err(AppError::UnsupportedFileType)` and sends no `TranscribeFile`. Ordering
  is extension check → existence/unreadability check, so a `.pdf` reports its
  type rather than "that file no longer exists".
- `app.rs::pick_transcription_file`:
  - on a media path: `set_pending_file` then show the setup form (T0023);
  - on a non-media path: report the error, keep the user on the welcome screen
    and send nothing. The native dialog is not re-opened automatically.
- The rejection never inspects file contents and never logs the full path in the
  diagnostic line (the redacted `detail` may name the extension only).

New typed error, added to `AppError` and to `diagnostics::variant_name`:

```rust
/// The picked path is not an audio or video file.
UnsupportedFileType,
```

- `message()`: `"That is not an audio or video file."`
- `action()`: `ErrorAction::Dismiss`
- `detail()`: `None`

## TDD Tasks

- [x] **T24.1 — Red first: the allowlist.** Unit tests in
      `crates/speechtt-core/src/media.rs`: every listed extension is accepted;
      uppercase and mixed case accepted; `.mp3` with a leading dot accepted;
      `pdf`, `txt`, `md`, `zip`, `exe`, `""`, `"."` rejected; a missing
      extension rejected; `archive.tar.mp3` accepted and `song.mp3.bak`
      rejected; a non-ASCII path does not panic. One test pins the exact list
      so a future edit is a deliberate change.
- [x] **T24.2 — Red first: the pending-file guard.**
      `tests/session_flow.rs`: `set_pending_file` with a `.pdf` returns
      `Err(AppError::UnsupportedFileType)`, leaves `pending_file()` `None`,
      reports no started take and sends no command; a `.mp4` is accepted.
- [x] **T24.3 — Red first: the take-start guard and its ordering.**
      `start_file_session` on a non-media path returns
      `Err(AppError::UnsupportedFileType)` with no command sent — including for
      a path that also does not exist, which must report the type rather than
      `FileNotFound`. A media path keeps the existing
      `FileNotFound` / `FileUnreadable` behaviour.
- [x] **T24.4 — Red first: the picker branch.** Drive the headless seam: a
      non-media selection leaves `view_state() == ViewState::Mode`, no pending
      file and exactly one `AppError::UnsupportedFileType`; a media selection
      leaves `ViewState::Form` with the pending file set and no take started.
- [x] **T24.5 — Implement** `media.rs`, the two guards, the picker branch and
      the new `AppError` variant (message, action, `detail`, diagnostics arm).
- [x] **T24.6 — Error surface.** `tests/error_ux.rs` gains
      `UnsupportedFileType` to the message/action table (message non-empty,
      ≤ 160 chars, `ErrorAction::Dismiss`) and to the "no secret, no transcript"
      invariant. Assert the diagnostic line contains the variant name and no
      full path.
- [x] **T24.7 — Docs + gates.** Note the allowlist and the "native dialog cannot
      filter" limitation in the README's file-transcription section or the
      ticket notes (whichever the repo uses at implementation time); update the
      board. Run `cargo fmt -- --check`,
      `cargo clippy --all-targets -- -D warnings`, `cargo test`, `bun test`,
      `bun check src/stt`. All green, zero warnings.

## Acceptance Criteria

1. Picking an audio or video file (any supported extension, any case) proceeds
   to the setup form exactly as T0023 specifies.
2. Picking a `.pdf`, `.txt`, archive, executable or extension-less file shows
   "That is not an audio or video file.", starts no take, and leaves the user on
   the welcome screen.
3. A rejected path is never stored as the pending file and never reaches the
   worker.
4. A media extension whose contents ffmpeg cannot decode still fails through
   the existing `unsupported_file` / `decode_failed` path — the allowlist does
   not mask that case.
5. The allowlist lives in `speechtt-core` and is the single source of truth;
   no duplicate extension list exists in `speechtt-ui`.
6. No new dependency, no `[patch]`, and no fork of the UI framework.
7. All gates green with zero warnings; no layout, geometry or screenshot test
   added.

## Implementation Notes

### Decisions

- **`MEDIA_EXTENSIONS` keeps the ticket's list plus four obvious relatives**
  (`mp2`, `3g2`, `mts`, `m2ts`, `ogv`). They are ordinary containers ffmpeg
  decodes, and leaving them out would refuse files the worker handles fine. The
  list is pinned by a test, so any further change is a deliberate edit.
- **Guard order is: malformed path → extension → existence → file-ness.** An
  empty or NUL-bearing path keeps `FileUnreadable` (it is not a path at all), a
  non-media extension wins over a missing file (`FileNotFound`), and a
  media-shaped path that is a directory fails on the file check. This preserves
  both pre-existing behaviours while giving a `.pdf` the message that actually
  explains it.
- **`set_pending_file` returns `Result<(), AppError>`.** It stores nothing on
  refusal, so a rejected path can never become the pending file or reach the
  worker.
- **New headless seam `SpeechTtApp::accept_picked_file`.** The picker closure
  and the tests now share one method, because the gpui harness cannot answer a
  native path prompt. It is the same pattern as `start_file_from_path`.

### Deviations

- **`file_session_rejects_a_directory` was split in two.** Its old expectation
  (`FileUnreadable` for the extension-less temp dir) now conflicts with the
  extension gate, so it asserts `UnsupportedFileType` for that case and keeps
  `FileUnreadable` for a directory named `fake.mp4`. This is a deliberate
  behaviour change, recorded here rather than silently rewritten.
- **No "picker branch" e2e test can drive the real dialog.** T24.4 is covered at
  the `accept_picked_file` seam plus the controller test, which is where the
  decision lives.

### Verification

```text
cargo fmt -- --check                          → clean
cargo clippy --all-targets -- -D warnings     → clean, 0 warnings
cargo test                                    → 14 suites ok, 0 failed
bun test                                      → 217 pass, 0 fail
bun check src/stt                             → No type errors in 23 files
```

Mutation check: forcing `is_media_extension` to return `true` fails 4 specs
(`non_media_extensions_are_rejected`, `an_empty_or_dot_only_extension_is_rejected`,
`a_path_without_an_extension_is_not_media`, `a_non_ascii_path_does_not_panic`),
so the specs are not vacuous.

