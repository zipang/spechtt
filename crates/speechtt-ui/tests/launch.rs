//! Launch gate: the compiled widget binary reaches its first window and keeps
//! running.
//!
//! `tests/startup.rs` runs the startup seam on a headless context, which parses
//! every registration the application performs — but it never executes
//! `gpui_kit::application().run`'s event loop, so a panic inside that closure
//! below the seam, a failed window creation and a runtime failure while the loop
//! runs are all invisible to it. This file closes that gap by starting
//! `CARGO_BIN_EXE_speechtt-ui`, the real binary a user launches.
//!
//! The child runs **headless**: [`widget_command`] removes `DISPLAY` and
//! `WAYLAND_DISPLAY`, and `gpui-kit` then starts the application with no display
//! connection, so `cargo test` opens no window on the developer's desktop.
//! `Application::with_windowing` documents the behaviour this relies on:
//! "Defaults to windowed. On Linux, the default environment is the process's own,
//! and the app starts headless if that names no allowed display server."
//! <https://docs.rs/gpui-kit/0.9.1/gpui_kit/struct.Application.html>
//!
//! The binary is judged by **liveness plus one positive signal**:
//!
//! 1. It writes [`speechtt_ui::diagnostics::launched_line`] once its window is
//!    open. This test waits for that line, so the verdict distinguishes "still
//!    initialising" from "running" instead of inferring it from a timeout.
//! 2. It must still be alive [`SETTLE_WINDOW`] after that line, because a panic
//!    in the first frame's layout happens after `open_window` returns.
//! 3. It must not exit at all — least of all with status 0, since the widget
//!    holds no quit path at startup. An exit is always a failure, with the
//!    child's `stderr` attached.
//!
//! The child is spawned with its settings root redirected — `HOME` and
//! `XDG_CONFIG_HOME` point into a throwaway directory — so the persisted window
//! layout it reads and writes lands there and not in the user's config
//! directory. [`the_child_resolves_its_settings_inside_the_sandbox`] asserts
//! that redirection, which is a fact about the command this file builds. It does
//! not observe the user's real settings file: another running instance
//! legitimately saves it, so watching it would make the gate fail for a reason
//! outside its subject. The child is killed and reaped on every path, including
//! a failing one, so no widget process outlives the test.
//!
//! No machine state skips this file: it needs no display server, no audio server
//! and no user configuration, so it runs on every developer machine and in CI.
//!
//! Run by `cargo test` (AGENTS.md §5).

use std::env;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

/// The two variables that make `gpui` connect to a display server.
///
/// [`widget_command`] removes both, which is what starts the child headless;
/// [`the_child_resolves_its_settings_inside_the_sandbox`] pins their removal.
const DISPLAY_VARIABLES: [&str; 2] = ["DISPLAY", "WAYLAND_DISPLAY"];

/// How long the binary may take to report that its window is open.
///
/// Generous on purpose: a debug binary cold-loads the fonts, the theme and the
/// worker process, and a slow CI runner must not be mistaken for a dead widget.
/// The window is a backstop for the positive signal, not the assertion itself.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(20);

/// How long the binary must stay alive after reporting its window.
///
/// Covers the failure modes that land after `open_window` returns: a panic in
/// the first frame's layout, and a runtime that gives up while the event loop
/// runs.
const SETTLE_WINDOW: Duration = Duration::from_secs(3);

/// How often the child's status and `stderr` are sampled.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// The widget reaches its first window and stays up.
///
/// Runs headless, so it needs no display server and opens no window: the child
/// runs in the throwaway settings root [`widget_command`] builds, and this test
/// removes that root afterwards.
#[test]
fn the_built_binary_reaches_a_running_window() {
    let sandbox = Sandbox::create();

    let mut child = widget_command(&sandbox)
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

    if let Err(reason) = verdict {
        panic!("{reason}\n--- widget stderr ---\n{stderr}");
    }
}

/// The command this gate spawns redirects the child's settings and detaches it
/// from every display server.
///
/// Two documented facts make that the whole isolation: `JsonFileStore::default_path()`
/// reads `XDG_CONFIG_HOME` and falls back to `$HOME/.config`, so pinning both
/// overrides keeps the child's reads and writes in the sandbox; and `gpui` starts
/// headless when the environment names no allowed display server, so removing
/// `DISPLAY` and `WAYLAND_DISPLAY` opens no window on the desktop.
///
/// The check is a property of the command — true or false regardless of what any
/// other process on the machine is doing, and needing no display server itself —
/// so a dropped or mistyped `.env(..)` or `.env_remove(..)` fails it
/// deterministically.
#[test]
fn the_child_resolves_its_settings_inside_the_sandbox() {
    let sandbox = Sandbox::create();
    let command = widget_command(&sandbox);
    let env: Vec<(OsString, Option<OsString>)> = command
        .get_envs()
        .map(|(key, value)| (key.to_owned(), value.map(OsStr::to_owned)))
        .collect();

    // `None` means the variable is not touched, `Some(None)` that it is removed,
    // `Some(Some(..))` that it is set — so a lookup answers all three.
    let found = |name: &str| {
        env.iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };

    for (name, expected) in [
        ("HOME", sandbox.home()),
        ("XDG_CONFIG_HOME", sandbox.config()),
    ] {
        let resolved = found(name).flatten().map(PathBuf::from);
        assert_eq!(
            resolved.as_deref(),
            Some(expected.as_path()),
            "the child must be spawned with {name} pointing into its throwaway settings \
             root, got {resolved:?} instead of {expected:?}"
        );
    }

    for name in DISPLAY_VARIABLES {
        let removed = matches!(found(name), Some(None));
        assert!(
            removed,
            "the child must be spawned with {name} removed, so `gpui` starts headless and \
             opens no window on the desktop; found {:?}",
            found(name).flatten()
        );
    }

    assert!(
        sandbox.config().starts_with(env::temp_dir()),
        "the child's settings root must be temporary, got {}",
        sandbox.config().display()
    );
    for directory in [sandbox.home(), sandbox.config()] {
        assert!(
            directory.is_dir(),
            "{} must exist before the child starts, or it reads no settings at all",
            directory.display()
        );
    }
}

/// The launch signal is one physical line on the diagnostic channel, and it
/// names no path and no environment value.
///
/// It is the handshake this gate waits for, and a diagnostic a user may see in
/// a terminal, so it stays a fixed token: the throwaway settings root must not
/// leak into it.
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
    for inherited in ["HOME", "XDG_CONFIG_HOME"] {
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

/// The command that launches the built widget inside `sandbox`, headless.
///
/// One place builds it so the gate's spawn and
/// [`the_child_resolves_its_settings_inside_the_sandbox`] cannot drift: the
/// redirection and the display detachment are asserted on this command rather
/// than inferred from a running process.
///
/// `stdin` is null and both output channels are piped, because the gate judges
/// the child by its `stderr` and must never block on a full pipe. `HOME` and
/// `XDG_CONFIG_HOME` carry the throwaway settings root. `DISPLAY` and
/// `WAYLAND_DISPLAY` are removed, which is what makes `gpui` start headless — the
/// gate must not flash a window on the desktop. Everything else is inherited: the
/// text system, the worker and the rest of the runtime read their own variables.
fn widget_command(sandbox: &Sandbox) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_speechtt-ui"));
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("HOME", sandbox.home())
        .env("XDG_CONFIG_HOME", sandbox.config());
    for variable in DISPLAY_VARIABLES {
        command.env_remove(variable);
    }
    command
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

/// The packaged AppDir layout resolves the worker from the executable's own
/// directory.
///
/// The binary is a **copy** into `usr/bin`, not a symlink: `current_exe()`
/// resolves a symlink back to the build tree, which would hide the AppDir
/// lookup behind the Cargo fallback. A copy of the real worker sits at
/// `usr/share/speechtt/worker.js`, and a fake `bun` first on `PATH` records the
/// argv it receives instead of running a worker. The verdict is therefore about
/// the packaged path — the AppDir's worker, not a repository path — while the
/// existing launch handshake still proves the window opens.
///
/// The child keeps the [`Sandbox`] isolation of the launch gate: throwaway
/// `HOME`/`XDG_CONFIG_HOME` and no display variables, so no window appears on
/// the developer's desktop. The child is killed and reaped on every path.
#[cfg(unix)]
#[test]
fn the_packaged_appdir_resolves_the_worker_through_the_usr_share_layout() {
    use std::os::unix::fs::PermissionsExt;

    let sandbox = Sandbox::create();
    let app_dir = sandbox.root.join("SpeechTT.AppDir");
    let bin_dir = app_dir.join("usr/bin");
    let worker_dir = app_dir.join("usr/share/speechtt");
    let fake_bin = sandbox.root.join("fake-bin");
    for directory in [&bin_dir, &worker_dir, &fake_bin] {
        fs::create_dir_all(directory)
            .unwrap_or_else(|error| panic!("cannot create {}: {error}", directory.display()));
    }

    let binary = bin_dir.join("speechtt-ui");
    fs::copy(env!("CARGO_BIN_EXE_speechtt-ui"), &binary)
        .unwrap_or_else(|error| panic!("cannot copy the widget binary: {error}"));
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755))
        .expect("the copied widget binary must be executable");

    let worker_source = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src/stt/worker.ts");
    let worker = worker_dir.join("worker.js");
    fs::copy(&worker_source, &worker)
        .unwrap_or_else(|error| panic!("cannot copy the worker: {error}"));

    let marker = sandbox.root.join("bun-args.txt");
    let fake_bun = fake_bin.join("bun");
    fs::write(
        &fake_bun,
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$SPEECHTT_FAKE_BUN_MARKER\"\nexit 0\n",
    )
    .expect("the fake bun script must be writable");
    fs::set_permissions(&fake_bun, fs::Permissions::from_mode(0o755))
        .expect("the fake bun script must be executable");

    let path = format!(
        "{}:{}",
        fake_bin.display(),
        env::var("PATH").unwrap_or_default()
    );
    let mut command = Command::new(&binary);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("HOME", sandbox.home())
        .env("XDG_CONFIG_HOME", sandbox.config())
        .env("PATH", path)
        .env("SPEECHTT_FAKE_BUN_MARKER", &marker);
    for variable in DISPLAY_VARIABLES {
        command.env_remove(variable);
    }

    let mut child = command
        .spawn()
        .expect("the packaged widget binary must be runnable");
    let captured = Arc::new(Mutex::new(Captured::default()));
    let stderr_reader = capture_stderr(&mut child, Arc::clone(&captured));
    let stdout_drain = drain_stdout(&mut child);

    let signal = speechtt_ui::diagnostics::launched_line();
    let verdict = judge(&mut child, &captured, &signal);
    let recorded = if verdict.is_ok() {
        wait_for_marker(&marker, Duration::from_secs(5))
    } else {
        None
    };

    let _ = child.kill();
    let _ = child.wait();
    let _ = stderr_reader.join();
    if let Some(drain) = stdout_drain {
        let _ = drain.join();
    }

    let stderr = captured.lock().expect("captured stderr lock").stderr();

    if let Err(reason) = verdict {
        panic!("{reason}\n--- widget stderr ---\n{stderr}");
    }

    let recorded = recorded.unwrap_or_else(|| {
        panic!("the fake bun was never spawned; the widget found no worker\n--- widget stderr ---\n{stderr}")
    });
    let mut lines = recorded.lines();
    assert_eq!(
        lines.next(),
        Some("run"),
        "bun must be invoked as `bun run`"
    );
    let spawned = PathBuf::from(lines.next().unwrap_or_default());
    let expected = bin_dir.join("../share/speechtt/worker.js");
    assert_eq!(
        spawned, expected,
        "the packaged widget must spawn the AppDir worker, not a repository path"
    );
}

/// Polls for the fake `bun`'s marker file until it has args or `timeout` passes.
fn wait_for_marker(marker: &PathBuf, timeout: Duration) -> Option<String> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Ok(contents) = fs::read_to_string(marker)
            && !contents.is_empty()
        {
            return Some(contents);
        }
        if Instant::now() >= deadline {
            return None;
        }
        thread::sleep(Duration::from_millis(50));
    }
}
