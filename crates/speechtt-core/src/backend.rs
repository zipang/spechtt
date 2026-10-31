//! Duplex STT backend: line-framed NDJSON transport and the Bun subprocess.
//!
//! Two layers, deliberately separated so the controller and tests never need
//! an OS process:
//!
//! - [`StdioClient`] is a pure generic NDJSON transport over any
//!   `Write`/`BufRead` pair. Each command is one JSON line plus `\n`, flushed
//!   immediately; each event line is parsed in isolation.
//! - [`BunSubprocess`] spawns `bun run src/stt/worker.ts`, owns the child's
//!   `stdin` for commands, and forwards parsed `stdout` events over an
//!   [`async_channel`]. Dropping it closes `stdin`, which tells the worker to
//!   abort its session and exit.

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::thread::JoinHandle;

use async_channel::{Receiver, Sender};

use crate::protocol::{SttCommand, SttEvent};

/// Worker entrypoint, relative to the repository root.
pub const WORKER_SCRIPT: &str = "src/stt/worker.ts";

/// Events forwarded from a duplex backend. `Err` reports a transport or
/// protocol failure; the stream ends (channel closed) when the worker exits.
pub type SttEventReceiver = Receiver<Result<SttEvent, BackendError>>;

/// Outbound side of a duplex STT session. The controller owns one optionally;
/// tests substitute an in-memory recorder.
pub trait SttSessionSink: Send {
    /// Send one command to the backend.
    ///
    /// # Errors
    ///
    /// Returns [`BackendError`] when serialization or the underlying transport
    /// fails.
    fn send(&mut self, cmd: SttCommand) -> Result<(), BackendError>;

    /// The backend's inbound event stream, when it is duplex. Purely outbound
    /// sinks (and test recorders) return `None`.
    fn events(&self) -> Option<SttEventReceiver> {
        None
    }
}

/// Why a backend operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BackendError {
    #[error("stt backend i/o failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("failed to encode an stt command: {0}")]
    Encode(serde_json::Error),
    #[error("failed to decode an stt event: {0}")]
    Decode(serde_json::Error),
    #[error("failed to spawn stt worker {program:?}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("the stt worker is no longer accepting commands")]
    Disconnected,
}

/// A line-framed NDJSON client over any writer/reader pair.
pub struct StdioClient<W: Write, R: BufRead> {
    writer: W,
    reader: R,
}

impl<W: Write, R: BufRead> StdioClient<W, R> {
    pub fn new(writer: W, reader: R) -> Self {
        Self { writer, reader }
    }

    /// Serialize one command as a single JSON line and flush it.
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::Encode`] or [`BackendError::Io`].
    pub fn send(&mut self, cmd: &SttCommand) -> Result<(), BackendError> {
        write_command(&mut self.writer, cmd)
    }

    /// Read the next non-blank event line. `Ok(None)` means the stream ended.
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::Decode`] for a malformed line or
    /// [`BackendError::Io`] when reading fails.
    pub fn read_event(&mut self) -> Result<Option<SttEvent>, BackendError> {
        read_event(&mut self.reader)
    }
}

impl<W: Write + Send, R: BufRead + Send> SttSessionSink for StdioClient<W, R> {
    fn send(&mut self, cmd: SttCommand) -> Result<(), BackendError> {
        self.send(&cmd)
    }
}

/// The real backend: a long-lived Bun process speaking NDJSON.
pub struct BunSubprocess {
    child: Child,
    /// `None` once dropped/closed; later sends report [`BackendError::Disconnected`].
    stdin: Option<ChildStdin>,
    events: SttEventReceiver,
    reader: Option<JoinHandle<()>>,
}

impl BunSubprocess {
    /// Spawn `bun run <script>` with piped `stdin`/`stdout`.
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::Spawn`] when `bun` is missing or the script
    /// cannot be started.
    pub fn spawn(script: impl AsRef<Path>) -> Result<Self, BackendError> {
        Self::spawn_with("bun", script)
    }

    /// Spawn the repository worker ([`WORKER_SCRIPT`]).
    ///
    /// # Errors
    ///
    /// See [`BunSubprocess::spawn`].
    pub fn spawn_default() -> Result<Self, BackendError> {
        Self::spawn(WORKER_SCRIPT)
    }

    /// Spawn `program run <script>`. `program` is injectable so tests can
    /// exercise the spawn-failure path without a Bun installation.
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::Spawn`] when the process cannot start, or
    /// [`BackendError::Io`] when the stdout reader thread cannot be created.
    pub fn spawn_with(
        program: impl Into<String>,
        script: impl AsRef<Path>,
    ) -> Result<Self, BackendError> {
        let program = program.into();
        let mut child = Command::new(&program)
            .arg("run")
            .arg(script.as_ref())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Diagnostics stay human-readable on stderr; stdout is protocol-only.
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|source| BackendError::Spawn {
                program: program.clone(),
                source,
            })?;

        let Some(stdin) = child.stdin.take() else {
            abort(&mut child);
            return Err(BackendError::Disconnected);
        };
        let Some(stdout) = child.stdout.take() else {
            abort(&mut child);
            return Err(BackendError::Disconnected);
        };

        let (sender, events) = async_channel::unbounded();
        let reader = match std::thread::Builder::new()
            .name("speechtt-stt-reader".to_string())
            .spawn(move || forward_events(stdout, &sender))
        {
            Ok(handle) => handle,
            Err(source) => {
                abort(&mut child);
                return Err(BackendError::Io(source));
            }
        };

        Ok(Self {
            child,
            stdin: Some(stdin),
            events,
            reader: Some(reader),
        })
    }

    /// Send one command to the worker's `stdin`.
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::Disconnected`] after `stdin` has been closed.
    pub fn send(&mut self, cmd: &SttCommand) -> Result<(), BackendError> {
        self.write(cmd)
    }

    /// Write one raw line to the worker's `stdin`.
    ///
    /// Typed commands should go through [`BunSubprocess::send`]; this escape
    /// hatch exists for protocol-fault tests and diagnostics. `line` must not
    /// contain a newline.
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::Disconnected`] after `stdin` has been closed, or
    /// [`BackendError::Io`] when the write fails.
    pub fn send_raw_line(&mut self, line: &str) -> Result<(), BackendError> {
        let stdin = self.stdin.as_mut().ok_or(BackendError::Disconnected)?;
        stdin.write_all(line.as_bytes())?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;
        Ok(())
    }

    /// A cloneable handle to the inbound event stream.
    #[must_use]
    pub fn events(&self) -> SttEventReceiver {
        self.events.clone()
    }

    fn write(&mut self, cmd: &SttCommand) -> Result<(), BackendError> {
        let stdin = self.stdin.as_mut().ok_or(BackendError::Disconnected)?;
        write_command(stdin, cmd)
    }
}

impl SttSessionSink for BunSubprocess {
    fn send(&mut self, cmd: SttCommand) -> Result<(), BackendError> {
        self.write(&cmd)
    }

    fn events(&self) -> Option<SttEventReceiver> {
        Some(self.events())
    }
}

impl Drop for BunSubprocess {
    fn drop(&mut self) {
        // Closing stdin is the worker's EOF signal: abort the session, flush
        // what it can, exit. Give it a short grace period, then reap.
        self.stdin.take();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(100);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if std::time::Instant::now() < deadline => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                Ok(None) | Err(_) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    break;
                }
            }
        }
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

/// Serialize `cmd` as one line, write it and flush.
fn write_command<W: Write>(writer: &mut W, cmd: &SttCommand) -> Result<(), BackendError> {
    let mut line = serde_json::to_string(cmd).map_err(BackendError::Encode)?;
    line.push('\n');
    writer.write_all(line.as_bytes())?;
    writer.flush()?;
    Ok(())
}

/// Read the next non-blank line and decode it.
fn read_event<R: BufRead>(reader: &mut R) -> Result<Option<SttEvent>, BackendError> {
    let mut line = String::new();
    loop {
        line.clear();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        return serde_json::from_str(line)
            .map(Some)
            .map_err(BackendError::Decode);
    }
}

/// Background reader: decode every worker line until EOF or a closed channel.
fn forward_events(stdout: ChildStdout, sender: &Sender<Result<SttEvent, BackendError>>) {
    let mut reader = BufReader::new(stdout);
    loop {
        match read_event(&mut reader) {
            Ok(Some(event)) => {
                if sender.send_blocking(Ok(event)).is_err() {
                    break;
                }
            }
            Ok(None) => break,
            Err(error) => {
                let _ = sender.send_blocking(Err(error));
                break;
            }
        }
    }
}

/// Best-effort cleanup when setup fails after the child exists.
fn abort(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use std::io::{BufReader, Cursor, Write};
    use std::path::Path;

    use super::*;

    #[derive(Default)]
    struct FlushRecorder {
        bytes: Vec<u8>,
        flushes: usize,
    }

    impl Write for FlushRecorder {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.bytes.extend_from_slice(buffer);
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            self.flushes += 1;
            Ok(())
        }
    }

    #[test]
    fn send_writes_one_command_per_line_and_flushes_each_time() {
        let mut output = FlushRecorder::default();
        {
            let mut client = StdioClient::new(&mut output, Cursor::new(Vec::<u8>::new()));

            client.send(&SttCommand::Pause).expect("writes");
            client.send(&SttCommand::Stop).expect("writes");
        }

        assert_eq!(
            output.bytes,
            b"{\"type\":\"pause\"}\n{\"type\":\"stop\"}\n".to_vec()
        );
        assert_eq!(output.flushes, 2);
    }

    #[test]
    fn audio_commands_are_framed_as_ndjson_without_pretty_printing() {
        let mut output = Vec::new();
        {
            let mut client = StdioClient::new(&mut output, Cursor::new(Vec::<u8>::new()));
            client
                .send(&SttCommand::audio_from_pcm16(&[1, -1]))
                .expect("writes");
        }

        let text = String::from_utf8(output).expect("utf-8");
        assert_eq!(text.lines().count(), 1);
        assert!(text.ends_with('\n'));
        assert!(
            text.starts_with(r#"{"type":"audio","pcm16Base64":""#),
            "{text}"
        );
    }

    #[test]
    fn read_event_parses_one_event_per_line_and_reports_eof() {
        let input = concat!(
            r#"{"type":"ready"}"#,
            "\n",
            r#"{"type":"partial","sessionId":"take-1","text":"audio"}"#,
            "\n",
        );
        let mut client = StdioClient::new(Vec::new(), BufReader::new(Cursor::new(input)));

        assert_eq!(client.read_event().expect("reads"), Some(SttEvent::Ready));
        assert_eq!(
            client.read_event().expect("reads"),
            Some(SttEvent::Partial {
                session_id: "take-1".into(),
                text: "audio".into(),
            })
        );
        assert_eq!(client.read_event().expect("reads"), None);
        // EOF stays EOF.
        assert_eq!(client.read_event().expect("reads"), None);
    }

    #[test]
    fn read_event_tolerates_blank_lines_and_crlf_framing() {
        let input = "\r\n{\"type\":\"ready\"}  \r\n\n{\"type\":\"stopped\",\"sessionId\":\"take-1\",\"fullText\":\"done\"}\r\n";
        let mut client = StdioClient::new(Vec::new(), Cursor::new(input));

        assert_eq!(client.read_event().expect("reads"), Some(SttEvent::Ready));
        assert_eq!(
            client.read_event().expect("reads"),
            Some(SttEvent::Stopped {
                session_id: "take-1".into(),
                full_text: "done".into(),
            })
        );
        assert_eq!(client.read_event().expect("reads"), None);
    }

    #[test]
    fn malformed_lines_surface_as_decode_errors() {
        let mut client = StdioClient::new(Vec::new(), Cursor::new(b"not json\n".as_slice()));

        assert!(matches!(client.read_event(), Err(BackendError::Decode(_))));
    }

    #[test]
    fn the_generic_client_is_a_session_sink() {
        fn send_through(sink: &mut dyn SttSessionSink) -> Result<(), BackendError> {
            sink.send(SttCommand::Resume)
        }

        let mut output = Vec::new();
        {
            let mut client = StdioClient::new(&mut output, Cursor::new(Vec::<u8>::new()));
            send_through(&mut client).expect("writes");
        }

        assert_eq!(output, b"{\"type\":\"resume\"}\n".to_vec());
    }

    #[test]
    fn a_missing_bun_binary_reports_a_spawn_error() {
        let result = BunSubprocess::spawn_with(
            "speechtt-bun-binary-that-does-not-exist",
            Path::new(WORKER_SCRIPT),
        );

        match result {
            Err(BackendError::Spawn { program, .. }) => {
                assert_eq!(program, "speechtt-bun-binary-that-does-not-exist");
            }
            Err(other) => panic!("expected a spawn error, got {other:?}"),
            Ok(_) => panic!("the fake bun binary must not exist"),
        }
    }

    #[test]
    fn the_worker_script_path_matches_the_bun_package_layout() {
        assert_eq!(WORKER_SCRIPT, "src/stt/worker.ts");
    }
}
