//! Launch gate: the compiled widget binary must open its window and keep running.
//!
//! `tests/startup.rs` runs the startup seam on a headless context, which parses
//! every registration the application performs — but it never executes
//! `gpui_kit::application().run`'s event loop, so a panic inside that closure
//! below the seam, a failed window creation and a GPU surface failure are all
//! invisible to it. This file closes that gap by starting
//! `CARGO_BIN_EXE_speechtt-ui`, the real binary a user launches.
//!
//! The binary is judged by **liveness plus one positive signal**:
//!
//! 1. It writes [`speechtt_ui::diagnostics::launched_line`] once its window is
//!    open. This test waits for that line, so the verdict distinguishes "still
//!    initialising" from "running" instead of inferring it from a timeout.
//! 2. It must still be alive [`SETTLE_WINDOW`] after that line, because a panic
//!    on the first frame happens after `open_window` returns.
//! 3. It must not exit at all — least of all with status 0, since the widget
//!    holds no quit path at startup. An exit is always a failure, with the
//!    child's `stderr` attached.
//!
//! The child runs in a throwaway `HOME` and `XDG_CONFIG_HOME` so it never reads
//! or writes the user's persisted settings, and it is killed and reaped on
//! every path, including a failing one, so no widget process outlives the test.
//!
//! A machine with no display server cannot open a `gpui` window: the test then
//! prints why it skipped instead of passing silently or hiding behind
//! `#[ignore]`.
//!
//! Run by `cargo test` (AGENTS.md §5).

use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use speechtt_core::settings::JsonFileStore;

/// How long the binary may take to report that its window is open.
///
/// Generous on purpose: a debug binary cold-loads the GPU backend and the
/// fonts, and a slow CI runner must not be mistaken for a dead widget. The
/// window is a backstop for the positive signal, not the assertion itself.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(20);

/// How long the binary must stay alive after reporting its window.
///
/// Covers the failure modes that land after `open_window` returns: a panic in
/// the first frame's render, a GPU surface that dies while presenting it.
const SETTLE_WINDOW: Duration = Duration::from_secs(3);

/// How often the child's status and `stderr` are sampled.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// The widget reaches its first window and stays up.
///
/// Hermetic apart from the display server it needs: the child's settings
/// directory is a temporary directory this test creates and removes, and the
/// user's real settings file is checked to be untouched afterwards.
#[test]
fn the_built_binary_reaches_a_running_window() {
    let Some(variable) = display_server() else {
        eprintln!(
            "[launch] skipped: neither DISPLAY nor WAYLAND_DISPLAY is set, so gpui has no \
             display server to open the widget's window on; this gate needs a graphical \
             session to run"
        );
        return;
    };
    eprintln!("[launch] running the built widget with {variable} set");

    let sandbox = Sandbox::create();
    // The user's own store, resolved in this process's environment. Only its
    // metadata is read, never its contents, and only to prove the child did not
    // write it.
    let real_settings = JsonFileStore::default_path();
    let real_settings_before = file_state(&real_settings);

    let mut child = Command::new(env!("CARGO_BIN_EXE_speechtt-ui"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("HOME", sandbox.home())
        .env("XDG_CONFIG_HOME", sandbox.config())
        .spawn()
        .expect("the built widget binary must be runnable");

    let captured = Arc::new(Mutex::new(Captured::default()));
    let stderr_reader = capture_stderr(&mut child, Arc::clone(&captured));
    let stdout_drain = drain_stdout(&mut child);

    let signal = speechtt_ui::diagnostics::launched_line();
    let verdict = judge(&mut child, &captured, &signal);

    // Cleanup runs on every path, the panic paths included, so the widget
    // process never outlives this test.
    let _ = child.kill();
    let _ = child.wait();
    let _ = stderr_reader.join();
    if let Some(drain) = stdout_drain {
        let _ = drain.join();
    }

    let stderr = captured.lock().expect("captured stderr lock").stderr();
    let real_settings_after = file_state(&real_settings);

    assert_eq!(
        real_settings_before,
        real_settings_after,
        "the widget must not write the user's real settings file at {}",
        real_settings.display()
    );
    assert!(
        sandbox.root.starts_with(env::temp_dir()),
        "the widget's settings must live in a temporary directory, got {}",
        sandbox.root.display()
    );

    if let Err(reason) = verdict {
        panic!("{reason}\n--- widget stderr ---\n{stderr}");
    }
}

/// The launch signal is one physical line on the diagnostic channel, and it
/// names no path and no environment value.
///
/// It is the handshake this gate waits for, and a diagnostic a user may see in
/// a terminal, so it stays a fixed token: the throwaway directory and the
/// display server must not leak into it.
#[test]
fn the_launch_signal_is_one_path_free_line() {
    let line = speechtt_ui::diagnostics::launched_line();

    assert_eq!(line.lines().count(), 1, "one physical line, got {line:?}");
    assert!(
        line.starts_with("[speechtt] "),
        "the signal belongs to the diagnostic channel, got {line:?}"
    );
    assert!(
        !line.contains('/') && !line.contains('\\'),
        "the signal must name no path, got {line:?}"
    );
    for inherited in ["HOME", "XDG_CONFIG_HOME", "DISPLAY"] {
        let value = env::var_os(inherited).map(|value| value.to_string_lossy().into_owned());
        if let Some(value) = value.filter(|value| !value.is_empty()) {
            assert!(
                !line.contains(&value),
                "the signal must not echo {inherited}, got {line:?}"
            );
        }
    }
}

/// An exit inside the observation window is always a failed launch, and status
/// 0 is called out separately: the widget holds no quit path before its window,
/// so a clean exit means no window was opened.
#[cfg(unix)]
#[test]
fn an_exit_inside_the_window_fails_the_launch() {
    use std::os::unix::process::ExitStatusExt;

    let clean = exited(ExitStatus::from_raw(0), "before its window");
    assert!(clean.contains("status 0"), "{clean}");
    assert!(clean.contains("never opened a window"), "{clean}");

    let crashed = exited(ExitStatus::from_raw(1 << 8), "before its window");
    assert!(crashed.contains("exit status: 1"), "{crashed}");
    assert!(!crashed.contains("status 0"), "{crashed}");
}

/// Which display-server variable lets `gpui` open a window, if any.
///
/// Both are read from this process's environment because the child inherits
/// them: the gate runs in a graphical session and skips elsewhere.
fn display_server() -> Option<&'static str> {
    ["WAYLAND_DISPLAY", "DISPLAY"]
        .into_iter()
        .find(|name| env::var_os(name).is_some_and(|value| !value.is_empty()))
}

/// Judges a spawned widget by its liveness and its launch signal.
///
/// Returns `Err` with the reason the launch did not happen; the caller adds the
/// captured `stderr` and cleans the child up. The child is left running on
/// success — the caller kills and reaps it.
fn judge(child: &mut Child, captured: &Arc<Mutex<Captured>>, signal: &str) -> Result<(), String> {
    let deadline = Instant::now() + LAUNCH_TIMEOUT;
    loop {
        if captured.lock().expect("captured stderr lock").saw(signal) {
            break;
        }
        if let Some(status) = exit_status(child)? {
            return Err(exited(status, "before reporting that its window is open"));
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "the widget did not report `{signal}` within {}s: it is alive but never \
                 reached its first window",
                LAUNCH_TIMEOUT.as_secs()
            ));
        }
        thread::sleep(POLL_INTERVAL);
    }

    let settle_deadline = Instant::now() + SETTLE_WINDOW;
    while Instant::now() < settle_deadline {
        if let Some(status) = exit_status(child)? {
            return Err(exited(status, "after reporting that its window is open"));
        }
        thread::sleep(POLL_INTERVAL);
    }

    Ok(())
}

/// `Some(status)` once the child has exited, `None` while it runs.
fn exit_status(child: &mut Child) -> Result<Option<ExitStatus>, String> {
    match child.try_wait() {
        Ok(status) => Ok(status),
        Err(error) => Err(format!("cannot read the widget's exit status: {error}")),
    }
}

/// Why an exit inside the observation window is a failed launch.
///
/// The widget has no quit path before its first window, so status 0 means it
/// returned from `run` without opening one; a signal or a non-zero status is
/// the shape a startup panic leaves behind.
fn exited(status: ExitStatus, when: &str) -> String {
    if status.success() {
        format!(
            "the widget exited with status 0 {when}: it has no quit path at startup, so a \
             clean exit means it never opened a window"
        )
    } else {
        format!("the widget exited ({status}) {when}: a panic while starting up looks like this")
    }
}

/// Everything the child wrote on its pipes, shared with the reader threads.
#[derive(Default)]
struct Captured {
    stderr: Vec<String>,
}

impl Captured {
    /// Whether any captured line carries `signal`.
    fn saw(&self, signal: &str) -> bool {
        self.stderr.iter().any(|line| line.contains(signal))
    }

    /// The captured `stderr`, for a failing verdict's message.
    fn stderr(&self) -> String {
        if self.stderr.is_empty() {
            "<the widget wrote nothing on stderr>".to_string()
        } else {
            self.stderr.join("\n")
        }
    }
}

/// Reads the child's `stderr` to the end on its own thread.
///
/// The loop needs the lines while the child is still running, so they are
/// pushed into the shared buffer as they arrive; a panic message written by the
/// child is therefore already captured by the time the exit status is seen.
fn capture_stderr(child: &mut Child, captured: Arc<Mutex<Captured>>) -> thread::JoinHandle<()> {
    let pipe = child.stderr.take().expect("the widget's stderr is piped");
    thread::spawn(move || {
        for line in BufReader::new(pipe).lines() {
            let Ok(line) = line else { break };
            captured
                .lock()
                .expect("captured stderr lock")
                .stderr
                .push(line);
        }
    })
}

/// Discards the child's `stdout` so a chatty widget cannot block on a full pipe.
///
/// `stdout` is not judged: the widget's protocol output travels over its own
/// worker pipes, and nothing about the launch is reported there.
fn drain_stdout(child: &mut Child) -> Option<thread::JoinHandle<()>> {
    let pipe = child.stdout.take()?;
    Some(thread::spawn(move || {
        let mut pipe = BufReader::new(pipe);
        let mut sink = io::sink();
        let _ = io::copy(&mut pipe, &mut sink);
    }))
}

/// A throwaway settings root for the child process.
///
/// The widget resolves its store through `XDG_CONFIG_HOME`, falling back to
/// `$HOME/.config`, so overriding both keeps every read and write of the child
/// inside [`Sandbox::root`]. The directory is removed when the guard drops,
/// which is also what a failing assertion's unwind does.
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    /// Creates the sandbox under the system temporary directory.
    fn create() -> Self {
        let unique = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_nanos());
        let root = env::temp_dir().join(format!("speechtt-launch-{}-{unique}", std::process::id()));
        let sandbox = Self { root };
        for directory in [sandbox.home(), sandbox.config()] {
            fs::create_dir_all(&directory)
                .unwrap_or_else(|error| panic!("cannot create {}: {error}", directory.display()));
        }
        sandbox
    }

    /// The child's `$HOME`.
    fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    /// The child's `$XDG_CONFIG_HOME`.
    fn config(&self) -> PathBuf {
        self.root.join("config")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

/// Size and modification time of `path`, or `None` when it does not exist.
///
/// Metadata only: the gate proves the user's settings file is not written
/// without reading what it holds.
fn file_state(path: &Path) -> Option<(u64, Option<SystemTime>)> {
    let metadata = fs::metadata(path).ok()?;
    Some((metadata.len(), metadata.modified().ok()))
}
