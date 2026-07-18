use std::io::{BufRead, BufReader, Read};
use std::process::{Child, ChildStderr, ChildStdout};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::Deserialize;

use crate::app::App;
use crate::herdr::Client;

const FALLBACK_REFRESH: Duration = Duration::from_secs(1);
const HEALTH_REFRESH: Duration = Duration::from_secs(5);
const INITIAL_RETRY: Duration = Duration::from_secs(2);
const MAX_RETRY: Duration = Duration::from_secs(30);
const STDERR_LIMIT: usize = 8 * 1024;

pub struct Controller {
    connection: Connection,
    target: Option<String>,
    dimensions: (u16, u16),
    snapshot_at: Instant,
    reaper: Reaper,
}

impl Controller {
    pub fn new() -> Self {
        Self {
            connection: Connection::disconnected_now(),
            target: None,
            dimensions: (0, 0),
            snapshot_at: Instant::now(),
            reaper: Reaper::new(),
        }
    }

    pub fn refresh(&mut self, app: &mut App, client: &Client, dimensions: (u16, u16)) {
        let wanted = app.selected_agent().and_then(|agent| agent.target());
        let changed = wanted != self.target.as_deref() || dimensions != self.dimensions;
        if changed {
            self.connection = Connection::disconnected_now();
            self.target = wanted.map(str::to_owned);
            self.dimensions = dimensions;
            app.preview_error = None;
            if let Some(target) = self.target.as_deref() {
                refresh_snapshot(app, client, target);
                self.snapshot_at = Instant::now();
            } else {
                app.preview.clear();
                return;
            }
        }

        let Some(target) = self.target.as_deref() else {
            return;
        };

        let now = Instant::now();
        if let Some(error) =
            self.connection
                .connect_if_due(client, target, dimensions, &self.reaper, now)
        {
            app.preview_error = Some(error);
        }

        match self.connection.poll() {
            ConnectionUpdate::Updated { revision, text } => {
                self.connection.mark_rendered(revision);
                app.preview = text;
                app.preview_error = None;
                return;
            }
            ConnectionUpdate::Ended(diagnostic) => {
                let error = diagnostic.unwrap_or_else(|| "stream ended unexpectedly".to_string());
                app.preview_error = Some(format!("live preview unavailable: {error}"));
                self.connection.disconnect(Instant::now());
            }
            ConnectionUpdate::Waiting => {}
        }

        let refresh_interval = if self.connection.is_observing() {
            HEALTH_REFRESH
        } else {
            FALLBACK_REFRESH
        };
        if self.snapshot_at.elapsed() >= refresh_interval {
            refresh_snapshot(app, client, target);
            self.snapshot_at = Instant::now();
        }
    }
}

fn refresh_snapshot(app: &mut App, client: &Client, target: &str) {
    app.preview = client
        .read_agent(target)
        .unwrap_or_else(|error| format!("preview unavailable: {error}"));
}

enum Connection {
    Disconnected {
        retry_at: Instant,
        retry_delay: Duration,
    },
    Observing {
        observer: Observer,
        rendered_revision: u64,
        retry_delay: Duration,
    },
}

impl Connection {
    fn disconnected_now() -> Self {
        Self::Disconnected {
            retry_at: Instant::now(),
            retry_delay: INITIAL_RETRY,
        }
    }

    fn connect_if_due(
        &mut self,
        client: &Client,
        target: &str,
        dimensions: (u16, u16),
        reaper: &Reaper,
        now: Instant,
    ) -> Option<String> {
        let Self::Disconnected {
            retry_at,
            retry_delay,
        } = self
        else {
            return None;
        };
        if now < *retry_at {
            return None;
        }
        let delay = *retry_delay;
        match Observer::start(client, target, dimensions, reaper.sender()) {
            Ok(observer) => {
                *self = Self::Observing {
                    observer,
                    rendered_revision: 0,
                    retry_delay: delay,
                };
                None
            }
            Err(error) => {
                *retry_at = now + delay;
                *retry_delay = next_retry(delay);
                Some(format!(
                    "live preview unavailable: {error}; retrying in {}s",
                    delay.as_secs()
                ))
            }
        }
    }

    fn poll(&self) -> ConnectionUpdate {
        let Self::Observing {
            observer,
            rendered_revision,
            ..
        } = self
        else {
            return ConnectionUpdate::Waiting;
        };
        observer.preview_after(*rendered_revision)
    }

    fn mark_rendered(&mut self, revision: u64) {
        if let Self::Observing {
            rendered_revision,
            retry_delay,
            ..
        } = self
        {
            *rendered_revision = revision;
            *retry_delay = INITIAL_RETRY;
        }
    }

    fn disconnect(&mut self, now: Instant) {
        let Self::Observing { retry_delay, .. } = self else {
            return;
        };
        let delay = *retry_delay;
        *self = Self::Disconnected {
            retry_at: now + delay,
            retry_delay: next_retry(delay),
        };
    }

    fn is_observing(&self) -> bool {
        matches!(self, Self::Observing { .. })
    }
}

fn next_retry(delay: Duration) -> Duration {
    delay.saturating_mul(2).min(MAX_RETRY)
}

enum ConnectionUpdate {
    Updated { revision: u64, text: String },
    Waiting,
    Ended(Option<String>),
}

struct Observer {
    child: Option<Child>,
    readers: Vec<JoinHandle<()>>,
    shared: Arc<Mutex<StreamState>>,
    reaper: Option<SyncSender<Resources>>,
}

impl Observer {
    fn start(
        client: &Client,
        target: &str,
        (columns, rows): (u16, u16),
        cleanup_sender: Option<SyncSender<Resources>>,
    ) -> Result<Self, StartError> {
        let mut child = client.observe_agent(target, columns, rows)?;
        let Some(stdout) = child.stdout.take() else {
            terminate(child, Vec::new());
            return Err(StartError::MissingPipe("stdout"));
        };
        let Some(stderr) = child.stderr.take() else {
            terminate(child, Vec::new());
            return Err(StartError::MissingPipe("stderr"));
        };

        let shared = Arc::new(Mutex::new(StreamState::default()));
        let stdout_shared = Arc::clone(&shared);
        let stdout_reader = thread::Builder::new()
            .name("agents-picker-preview".to_string())
            .spawn(move || read_frames(stdout, &stdout_shared));
        let stdout_reader = match stdout_reader {
            Ok(handle) => handle,
            Err(source) => {
                terminate(child, Vec::new());
                return Err(StartError::Thread(source));
            }
        };

        let stderr_shared = Arc::clone(&shared);
        let stderr_reader = thread::Builder::new()
            .name("agents-picker-preview-stderr".to_string())
            .spawn(move || read_stderr(stderr, &stderr_shared));
        let stderr_reader = match stderr_reader {
            Ok(handle) => handle,
            Err(source) => {
                terminate(child, vec![stdout_reader]);
                return Err(StartError::Thread(source));
            }
        };

        Ok(Self {
            child: Some(child),
            readers: vec![stdout_reader, stderr_reader],
            shared,
            reaper: cleanup_sender,
        })
    }

    fn preview_after(&self, rendered_revision: u64) -> ConnectionUpdate {
        let Ok(state) = self.shared.lock() else {
            return ConnectionUpdate::Ended(Some("preview state lock was poisoned".to_string()));
        };
        if state.revision > rendered_revision {
            if let Some(parser) = &state.parser {
                return ConnectionUpdate::Updated {
                    revision: state.revision,
                    text: parser.screen().contents(),
                };
            }
        }
        if state.is_ended() {
            ConnectionUpdate::Ended(state.diagnostic.clone())
        } else {
            ConnectionUpdate::Waiting
        }
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return;
        };
        let _ = child.kill();
        let resources = Resources {
            child,
            readers: std::mem::take(&mut self.readers),
        };
        if let Some(reaper) = &self.reaper {
            match reaper.try_send(resources) {
                Ok(()) => return,
                Err(TrySendError::Full(resources) | TrySendError::Disconnected(resources)) => {
                    finish_cleanup(resources);
                    return;
                }
            }
        }
        finish_cleanup(resources);
    }
}

#[derive(Debug, thiserror::Error)]
enum StartError {
    #[error(transparent)]
    Herdr(#[from] crate::herdr::Error),
    #[error("observer {0} was not piped")]
    MissingPipe(&'static str),
    #[error("failed to start preview reader thread: {0}")]
    Thread(std::io::Error),
}

struct Resources {
    child: Child,
    readers: Vec<JoinHandle<()>>,
}

fn terminate(mut child: Child, readers: Vec<JoinHandle<()>>) {
    let _ = child.kill();
    finish_cleanup(Resources { child, readers });
}

fn finish_cleanup(mut resources: Resources) {
    let _ = resources.child.wait();
    for reader in resources.readers {
        let _ = reader.join();
    }
}

struct Reaper {
    sender: Option<SyncSender<Resources>>,
    thread: Option<JoinHandle<()>>,
}

impl Reaper {
    fn new() -> Self {
        let (sender, receiver) = mpsc::sync_channel(4);
        match thread::Builder::new()
            .name("agents-picker-preview-reaper".to_string())
            .spawn(move || {
                while let Ok(resources) = receiver.recv() {
                    finish_cleanup(resources);
                }
            }) {
            Ok(thread) => Self {
                sender: Some(sender),
                thread: Some(thread),
            },
            Err(_) => Self {
                sender: None,
                thread: None,
            },
        }
    }

    fn sender(&self) -> Option<SyncSender<Resources>> {
        self.sender.clone()
    }
}

impl Drop for Reaper {
    fn drop(&mut self) {
        self.sender = None;
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[derive(Default)]
struct StreamState {
    parser: Option<vt100::Parser>,
    revision: u64,
    last_sequence: Option<u64>,
    stdout_ended: bool,
    stderr_ended: bool,
    fatal: bool,
    diagnostic: Option<String>,
}

impl StreamState {
    fn apply(&mut self, frame: &TerminalFrame<'_>, decoded: &[u8]) -> Result<(), &'static str> {
        match self.last_sequence {
            Some(previous) if frame.sequence <= previous => {
                return Err("terminal frame sequence did not advance");
            }
            Some(previous) if !frame.full && frame.sequence != previous.wrapping_add(1) => {
                return Err("terminal frame sequence gap");
            }
            None if !frame.full => {
                return Err("incremental frame arrived before a full frame");
            }
            _ => {}
        }

        if frame.full {
            self.parser = Some(vt100::Parser::new(frame.height, frame.width, 0));
        }
        let Some(parser) = &mut self.parser else {
            return Err("terminal parser is not initialized");
        };
        parser.process(decoded);
        self.revision = self.revision.wrapping_add(1);
        self.last_sequence = Some(frame.sequence);
        Ok(())
    }

    fn fail(&mut self, diagnostic: impl Into<String>) {
        self.fatal = true;
        self.diagnostic = Some(diagnostic.into());
    }

    fn is_ended(&self) -> bool {
        self.fatal || (self.stdout_ended && self.stderr_ended)
    }
}

#[derive(Deserialize)]
struct TerminalRecord<'a> {
    #[serde(rename = "type", borrow)]
    kind: &'a str,
    #[serde(default, borrow)]
    bytes: Option<&'a str>,
    #[serde(default)]
    full: bool,
    #[serde(default)]
    height: u16,
    #[serde(default, borrow)]
    encoding: Option<&'a str>,
    #[serde(default, rename = "seq")]
    sequence: u64,
    #[serde(default)]
    width: u16,
}

struct TerminalFrame<'a> {
    bytes: &'a str,
    full: bool,
    height: u16,
    sequence: u64,
    width: u16,
}

impl<'a> TryFrom<TerminalRecord<'a>> for TerminalFrame<'a> {
    type Error = &'static str;

    fn try_from(record: TerminalRecord<'a>) -> Result<Self, Self::Error> {
        if record.kind != "terminal.frame" {
            return Err("record is not a terminal frame");
        }
        if record.height == 0 || record.width == 0 {
            return Err("terminal frame has zero dimensions");
        }
        if record.encoding != Some("ansi") {
            return Err("terminal frame encoding is not ANSI");
        }
        Ok(Self {
            bytes: record.bytes.ok_or("terminal frame has no bytes")?,
            full: record.full,
            height: record.height,
            sequence: record.sequence,
            width: record.width,
        })
    }
}

fn read_frames(stdout: ChildStdout, shared: &Mutex<StreamState>) {
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();
    let mut decoded = Vec::new();
    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break,
            Err(error) => {
                fail(shared, format!("failed to read terminal frame: {error}"));
                return;
            }
            Ok(_) => {}
        }
        let record = match serde_json::from_str::<TerminalRecord<'_>>(&line) {
            Ok(record) => record,
            Err(error) => {
                fail(shared, format!("invalid terminal frame JSON: {error}"));
                return;
            }
        };
        if record.kind == "terminal.closed" {
            break;
        }
        if record.kind != "terminal.frame" {
            continue;
        }
        let frame = match TerminalFrame::try_from(record) {
            Ok(frame) => frame,
            Err(error) => {
                fail(shared, error);
                return;
            }
        };
        decoded.clear();
        if let Err(error) = STANDARD.decode_vec(frame.bytes, &mut decoded) {
            fail(shared, format!("invalid terminal frame base64: {error}"));
            return;
        }
        let Ok(mut state) = shared.lock() else {
            return;
        };
        if let Err(error) = state.apply(&frame, &decoded) {
            state.fail(error);
            return;
        }
    }
    if let Ok(mut state) = shared.lock() {
        state.stdout_ended = true;
    }
}

fn read_stderr(mut stderr: ChildStderr, shared: &Mutex<StreamState>) {
    let mut captured = Vec::with_capacity(STDERR_LIMIT);
    let mut buffer = [0; 1024];
    loop {
        match stderr.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                let remaining = STDERR_LIMIT.saturating_sub(captured.len());
                captured.extend_from_slice(&buffer[..read.min(remaining)]);
            }
        }
    }
    if let Ok(mut state) = shared.lock() {
        let diagnostic = String::from_utf8_lossy(&captured).trim().to_string();
        if !diagnostic.is_empty() {
            state.diagnostic = Some(diagnostic);
        }
        state.stderr_ended = true;
    }
}

fn fail(shared: &Mutex<StreamState>, diagnostic: impl Into<String>) {
    if let Ok(mut state) = shared.lock() {
        state.fail(diagnostic);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(sequence: u64, full: bool, ansi: &[u8]) -> (TerminalFrame<'_>, &[u8]) {
        (
            TerminalFrame {
                bytes: "",
                full,
                height: 4,
                sequence,
                width: 20,
            },
            ansi,
        )
    }

    #[test]
    fn full_and_incremental_frames_reconstruct_the_latest_screen() {
        let mut state = StreamState::default();
        let (first, ansi) = frame(1, true, b"\x1b[2J\x1b[Hhello");
        state.apply(&first, ansi).unwrap();
        let (second, ansi) = frame(2, false, b"\x1b[Hworld");
        state.apply(&second, ansi).unwrap();

        assert_eq!(state.parser.unwrap().screen().contents(), "world");
    }

    #[test]
    fn incremental_frame_requires_contiguous_sequence() {
        let mut state = StreamState::default();
        let (first, ansi) = frame(1, true, b"ready");
        state.apply(&first, ansi).unwrap();
        let (third, ansi) = frame(3, false, b"lost");

        assert_eq!(
            state.apply(&third, ansi),
            Err("terminal frame sequence gap")
        );
    }

    #[test]
    fn newer_full_frame_can_resynchronize_after_a_gap() {
        let mut state = StreamState::default();
        let (first, ansi) = frame(1, true, b"old");
        state.apply(&first, ansi).unwrap();
        let (newer, ansi) = frame(4, true, b"new");
        state.apply(&newer, ansi).unwrap();

        assert_eq!(state.parser.unwrap().screen().contents(), "new");
    }

    #[test]
    fn stale_full_frame_is_rejected() {
        let mut state = StreamState::default();
        let (first, ansi) = frame(2, true, b"new");
        state.apply(&first, ansi).unwrap();
        let (stale_frame, ansi) = frame(1, true, b"old");

        assert_eq!(
            state.apply(&stale_frame, ansi),
            Err("terminal frame sequence did not advance")
        );
    }

    #[test]
    fn terminal_frame_payload_is_borrowed_and_decoded() {
        let record = serde_json::from_str::<TerminalRecord<'_>>(
            r#"{"bytes":"aGVsbG8=","encoding":"ansi","full":true,"height":24,"seq":7,"type":"terminal.frame","width":80}"#,
        )
        .unwrap();
        let frame = TerminalFrame::try_from(record).unwrap();
        let decoded = STANDARD.decode(frame.bytes).unwrap();

        assert_eq!(
            (
                frame.sequence,
                frame.full,
                frame.width,
                frame.height,
                decoded
            ),
            (7, true, 80, 24, b"hello".to_vec())
        );
    }
}
