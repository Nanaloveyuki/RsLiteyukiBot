use super::*;
use futures_util::{SinkExt, StreamExt};
use std::collections::VecDeque;
use tokio::net::TcpStream;
use tokio::sync::broadcast;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

#[derive(Default)]
pub(super) struct WebTerminalState {
    next_session_id: AtomicU64,
    sessions: Mutex<HashMap<String, Arc<TerminalSession>>>,
}

pub(super) struct TerminalSession {
    pub(super) id: String,
    pub(super) shell: String,
    lifecycle: AtomicU8,
    cols: AtomicU16,
    rows: AtomicU16,
    pub(super) output_tx: broadcast::Sender<String>,
    handles: Mutex<TerminalProcessHandles>,
    history: Mutex<TerminalHistory>,
}

#[derive(Default)]
struct TerminalProcessHandles {
    input_tx: Option<std::sync::mpsc::Sender<Vec<u8>>>,
    master: Option<Arc<Mutex<Box<dyn MasterPty + Send>>>>,
    killer: Option<Arc<Mutex<Box<dyn portable_pty::ChildKiller + Send + Sync>>>>,
}

#[derive(Default)]
struct TerminalHistory {
    lines: VecDeque<String>,
    partial: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct TerminalClientMessage {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    data: String,
    cols: Option<u16>,
    rows: Option<u16>,
}

impl WebTerminalState {
    pub(super) fn create_session(&self, cols: u16, rows: u16) -> String {
        let session_id = format!(
            "term-{}",
            self.next_session_id.fetch_add(1, Ordering::SeqCst) + 1
        );
        let session =
            TerminalSession::new(session_id.clone(), cols, rows, preferred_terminal_shell());
        self.sessions
            .lock()
            .expect("terminal session map should not be poisoned")
            .insert(session_id.clone(), session);
        session_id
    }

    pub(super) fn list_sessions(&self) -> Vec<String> {
        let mut ids = self
            .sessions
            .lock()
            .expect("terminal session map should not be poisoned")
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        ids.sort();
        ids
    }

    pub(super) fn get_session(&self, id: &str) -> Option<Arc<TerminalSession>> {
        self.sessions
            .lock()
            .expect("terminal session map should not be poisoned")
            .get(id)
            .cloned()
    }

    pub(super) fn close_session(&self, id: &str) -> bool {
        let session = self
            .sessions
            .lock()
            .expect("terminal session map should not be poisoned")
            .remove(id);
        if let Some(session) = session {
            session.mark_closed();
            session.shutdown();
            true
        } else {
            false
        }
    }

    fn remove_if_same(&self, id: &str, session: &Arc<TerminalSession>) {
        let mut sessions = self
            .sessions
            .lock()
            .expect("terminal session map should not be poisoned");
        if sessions
            .get(id)
            .is_some_and(|current| Arc::ptr_eq(current, session))
        {
            sessions.remove(id);
        }
    }
}

impl TerminalSession {
    pub(super) fn new(id: String, cols: u16, rows: u16, shell: String) -> Arc<Self> {
        let (output_tx, _) = broadcast::channel(TERMINAL_OUTPUT_CHANNEL_CAPACITY);
        Arc::new(Self {
            id,
            shell,
            lifecycle: AtomicU8::new(TERMINAL_STATE_IDLE),
            cols: AtomicU16::new(cols.max(1)),
            rows: AtomicU16::new(rows.max(1)),
            output_tx,
            handles: Mutex::new(TerminalProcessHandles::default()),
            history: Mutex::new(TerminalHistory::default()),
        })
    }

    pub(super) fn subscribe(&self) -> broadcast::Receiver<String> {
        self.output_tx.subscribe()
    }

    pub(super) fn recent_history_text(&self) -> Option<String> {
        let history = self
            .history
            .lock()
            .expect("terminal history should not be poisoned");
        let mut lines = history.lines.iter().cloned().collect::<Vec<_>>();
        if !history.partial.is_empty() {
            lines.push(history.partial.clone());
        }
        if lines.len() > TERMINAL_HISTORY_LINES {
            lines = lines.split_off(lines.len() - TERMINAL_HISTORY_LINES);
        }
        if lines.is_empty() {
            None
        } else {
            Some(lines.join("\r\n"))
        }
    }

    pub(super) fn resize(&self, cols: u16, rows: u16) -> Result<(), String> {
        let cols = cols.max(1);
        let rows = rows.max(1);
        self.cols.store(cols, Ordering::SeqCst);
        self.rows.store(rows, Ordering::SeqCst);

        let master = self
            .handles
            .lock()
            .expect("terminal handles should not be poisoned")
            .master
            .clone();
        if let Some(master) = master {
            master
                .lock()
                .expect("terminal pty master should not be poisoned")
                .resize(pty_size(rows, cols))
                .map_err(|err| format!("failed to resize terminal: {err}"))?;
        }
        Ok(())
    }

    fn mark_closed(&self) {
        self.lifecycle
            .store(TERMINAL_STATE_CLOSED, Ordering::SeqCst);
    }

    pub(super) async fn ensure_started(
        self: &Arc<Self>,
        terminal_state: Arc<WebTerminalState>,
    ) -> Result<(), String> {
        loop {
            match self.lifecycle.load(Ordering::SeqCst) {
                TERMINAL_STATE_RUNNING => return Ok(()),
                TERMINAL_STATE_CLOSED => return Err("terminal session is closed".to_string()),
                TERMINAL_STATE_STARTING => sleep(Duration::from_millis(25)).await,
                TERMINAL_STATE_IDLE => {
                    if self
                        .lifecycle
                        .compare_exchange(
                            TERMINAL_STATE_IDLE,
                            TERMINAL_STATE_STARTING,
                            Ordering::SeqCst,
                            Ordering::SeqCst,
                        )
                        .is_ok()
                    {
                        break;
                    }
                }
                _ => return Err("terminal session entered an invalid state".to_string()),
            }
        }

        match self.start_process(terminal_state) {
            Ok(()) => {
                self.lifecycle
                    .store(TERMINAL_STATE_RUNNING, Ordering::SeqCst);
                let _ = self.output_tx.send(format!(
                    "\u{1b}[90m[terminal:{}] local {} session ready in {}\u{1b}[0m\r\n",
                    self.id,
                    self.shell,
                    std::env::current_dir()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|_| ".".to_string())
                ));
                Ok(())
            }
            Err(err) => {
                self.lifecycle.store(TERMINAL_STATE_IDLE, Ordering::SeqCst);
                Err(err)
            }
        }
    }

    fn start_process(
        self: &Arc<Self>,
        terminal_state: Arc<WebTerminalState>,
    ) -> Result<(), String> {
        let pty_system = native_pty_system();
        let pty_pair = pty_system
            .openpty(pty_size(
                self.rows.load(Ordering::SeqCst),
                self.cols.load(Ordering::SeqCst),
            ))
            .map_err(|err| format!("failed to open PTY: {err}"))?;

        let mut command = build_terminal_command(&self.shell);
        command.cwd(workspace_root());

        let reader = pty_pair
            .master
            .try_clone_reader()
            .map_err(|err| format!("failed to clone PTY reader: {err}"))?;
        let writer = pty_pair
            .master
            .take_writer()
            .map_err(|err| format!("failed to take PTY writer: {err}"))?;

        let master = Arc::new(Mutex::new(pty_pair.master));
        let mut child = pty_pair
            .slave
            .spawn_command(command)
            .map_err(|err| format!("failed to start local PTY shell '{}': {err}", self.shell))?;
        let killer = Arc::new(Mutex::new(child.clone_killer()));
        let (input_tx, input_rx) = std::sync::mpsc::channel::<Vec<u8>>();

        {
            let mut handles = self
                .handles
                .lock()
                .expect("terminal handles should not be poisoned");
            handles.input_tx = Some(input_tx);
            handles.master = Some(Arc::clone(&master));
            handles.killer = Some(Arc::clone(&killer));
        }

        spawn_terminal_reader_thread(Arc::clone(self), reader, self.output_tx.clone());
        spawn_terminal_writer_thread(writer, input_rx, self.output_tx.clone());

        let session = Arc::clone(self);
        thread::spawn(move || {
            let exit_result = child.wait();
            session.mark_closed();
            session.clear_runtime_handles();
            let message = match exit_result {
                Ok(status) => format!(
                    "\r\n\u{1b}[90m[terminal:{}] PTY shell exited with {}\u{1b}[0m\r\n",
                    session.id, status
                ),
                Err(err) => format!(
                    "\r\n\u{1b}[31m[terminal:{}] PTY wait failed: {}\u{1b}[0m\r\n",
                    session.id, err
                ),
            };
            let _ = session.output_tx.send(message);
            terminal_state.remove_if_same(&session.id, &session);
        });

        Ok(())
    }

    pub(super) fn write_input(&self, data: &str) -> Result<(), String> {
        let sender = self
            .handles
            .lock()
            .expect("terminal handles should not be poisoned")
            .input_tx
            .clone()
            .ok_or_else(|| "terminal session is not running".to_string())?;
        sender
            .send(data.as_bytes().to_vec())
            .map_err(|_| "terminal writer is offline".to_string())
    }

    fn shutdown(&self) {
        let killer = {
            let mut handles = self
                .handles
                .lock()
                .expect("terminal handles should not be poisoned");
            let killer = handles.killer.clone();
            handles.input_tx = None;
            handles.master = None;
            handles.killer = None;
            killer
        };
        if let Some(killer) = killer {
            let _ = killer
                .lock()
                .expect("terminal killer should not be poisoned")
                .kill();
        }
    }

    fn clear_runtime_handles(&self) {
        let mut handles = self
            .handles
            .lock()
            .expect("terminal handles should not be poisoned");
        handles.input_tx = None;
        handles.master = None;
        handles.killer = None;
    }

    pub(super) fn remember_output(&self, chunk: &str) {
        if chunk.is_empty() {
            return;
        }

        let normalized = chunk.replace("\r\n", "\n").replace('\r', "\n");
        let mut history = self
            .history
            .lock()
            .expect("terminal history should not be poisoned");

        for ch in normalized.chars() {
            if ch == '\n' {
                let line = history.partial.trim_end().to_string();
                history.partial.clear();
                if !line.is_empty() {
                    history.lines.push_back(line);
                    while history.lines.len() > TERMINAL_HISTORY_LINES {
                        history.lines.pop_front();
                    }
                }
            } else {
                history.partial.push(ch);
            }
        }
    }
}

pub(super) fn preferred_terminal_shell() -> String {
    if cfg!(windows) {
        "pwsh.exe".to_string()
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }
}

fn build_terminal_command(shell: &str) -> PtyCommandBuilder {
    let mut command = PtyCommandBuilder::new(shell);
    if cfg!(windows) {
        if shell.eq_ignore_ascii_case("pwsh.exe") || shell.eq_ignore_ascii_case("powershell.exe") {
            command.args(["-NoLogo", "-NoProfile"]);
        } else if shell.eq_ignore_ascii_case("cmd.exe") {
            command.args(["/Q"]);
        }
    } else {
        command.arg("-i");
    }
    command
}

fn pty_size(rows: u16, cols: u16) -> PtySize {
    PtySize {
        rows: rows.max(1),
        cols: cols.max(1),
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn spawn_terminal_reader_thread(
    session: Arc<TerminalSession>,
    mut reader: Box<dyn std::io::Read + Send>,
    output_tx: broadcast::Sender<String>,
) {
    thread::spawn(move || {
        let mut buffer = vec![0_u8; TERMINAL_STREAM_BUFFER_BYTES];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(read) => {
                    let chunk = String::from_utf8_lossy(&buffer[..read]).into_owned();
                    if !chunk.is_empty() {
                        session.remember_output(chunk.as_str());
                        let _ = output_tx.send(chunk);
                    }
                }
                Err(err) => {
                    let _ = output_tx.send(format!(
                        "\r\n\u{1b}[31m[terminal] PTY read failed: {}\u{1b}[0m\r\n",
                        err
                    ));
                    break;
                }
            }
        }
    });
}

fn spawn_terminal_writer_thread(
    mut writer: Box<dyn std::io::Write + Send>,
    input_rx: std::sync::mpsc::Receiver<Vec<u8>>,
    output_tx: broadcast::Sender<String>,
) {
    thread::spawn(move || {
        while let Ok(payload) = input_rx.recv() {
            if let Err(err) = writer.write_all(payload.as_slice()) {
                let _ = output_tx.send(format!(
                    "\r\n\u{1b}[31m[terminal] PTY write failed: {}\u{1b}[0m\r\n",
                    err
                ));
                break;
            }
            if let Err(err) = writer.flush() {
                let _ = output_tx.send(format!(
                    "\r\n\u{1b}[31m[terminal] PTY flush failed: {}\u{1b}[0m\r\n",
                    err
                ));
                break;
            }
        }
    });
}

pub(super) fn handle_terminal_client_message(
    session: &Arc<TerminalSession>,
    payload: &str,
) -> Result<(), String> {
    let message = serde_json::from_str::<TerminalClientMessage>(payload)
        .map_err(|err| format!("invalid terminal message: {err}"))?;
    match message.kind.as_str() {
        "input" => session.write_input(message.data.as_str()),
        "resize" => session.resize(
            message.cols.unwrap_or(TERMINAL_DEFAULT_COLS),
            message.rows.unwrap_or(TERMINAL_DEFAULT_ROWS),
        ),
        other => Err(format!("unsupported terminal message type: {other}")),
    }
}

pub(super) fn terminal_ws_text(text: impl Into<String>) -> Message {
    let payload = serde_json::json!({ "data": text.into() }).to_string();
    Message::Text(payload)
}

pub(super) async fn handle_terminal_websocket(
    service: &WebHostService,
    socket: TcpStream,
) -> io::Result<()> {
    let request_path = Arc::new(Mutex::new(None::<String>));
    let capture = Arc::clone(&request_path);
    let ws_stream = accept_hdr_async(socket, move |request: &Request, response: Response| {
        if let Ok(mut slot) = capture.lock() {
            *slot = request
                .uri()
                .path_and_query()
                .map(|value| value.as_str().to_string())
                .or_else(|| Some(request.uri().path().to_string()));
        }
        Ok(response)
    })
    .await
    .map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("websocket upgrade failed: {err}"),
        )
    })?;

    let raw_path = request_path
        .lock()
        .ok()
        .and_then(|slot| slot.clone())
        .unwrap_or_default();
    let query = parse_query_string(raw_path.as_str());
    let session_id = query.get("id").cloned().unwrap_or_default();
    let token = query.get("token").cloned().unwrap_or_default();
    let Some(session) = service.terminal_state.get_session(session_id.as_str()) else {
        return serve_terminal_socket_with_error(ws_stream, "terminal session not found").await;
    };
    if !service.auth.is_session_token_valid(token.as_str()) {
        return serve_terminal_socket_with_error(ws_stream, "terminal token is invalid").await;
    }
    if let Err(err) = session
        .ensure_started(Arc::clone(&service.terminal_state))
        .await
    {
        return serve_terminal_socket_with_error(ws_stream, err.as_str()).await;
    }

    let (mut ws_write, mut ws_read) = ws_stream.split();
    if let Some(history) = session.recent_history_text() {
        ws_write
            .send(terminal_ws_text(format!("{history}\r\n")))
            .await
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    format!("failed to replay terminal history: {err}"),
                )
            })?;
    }
    ws_write
        .send(terminal_ws_text(format!(
            "\u{1b}[90m[terminal:{}] attached to local {} shell\u{1b}[0m\r\n",
            session.id, session.shell
        )))
        .await
        .map_err(|err| {
            io::Error::new(
                io::ErrorKind::BrokenPipe,
                format!("failed to write terminal banner: {err}"),
            )
        })?;

    let mut output_rx = session.subscribe();
    let writer = tokio::spawn(async move {
        loop {
            match output_rx.recv().await {
                Ok(first_chunk) => {
                    let mut chunk = first_chunk;
                    loop {
                        match tokio::time::timeout(TERMINAL_WS_BATCH_INTERVAL, output_rx.recv())
                            .await
                        {
                            Ok(Ok(next_chunk)) => chunk.push_str(next_chunk.as_str()),
                            Ok(Err(broadcast::error::RecvError::Lagged(_))) => continue,
                            Ok(Err(broadcast::error::RecvError::Closed)) => break,
                            Err(_) => break,
                        }
                    }
                    if ws_write.send(terminal_ws_text(chunk)).await.is_err() {
                        break;
                    }
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let input_session = Arc::clone(&session);
    let reader = tokio::spawn(async move {
        while let Some(message) = ws_read.next().await {
            match message {
                Ok(Message::Text(payload)) => {
                    if let Err(err) =
                        handle_terminal_client_message(&input_session, payload.as_ref())
                    {
                        let _ = input_session.output_tx.send(format!(
                            "\r\n\u{1b}[31m[terminal:{}] {}\u{1b}[0m\r\n",
                            input_session.id, err
                        ));
                    }
                }
                Ok(Message::Binary(payload)) => {
                    if let Ok(text) = String::from_utf8(payload.to_vec())
                        && let Err(err) =
                            handle_terminal_client_message(&input_session, text.as_str())
                    {
                        let _ = input_session.output_tx.send(format!(
                            "\r\n\u{1b}[31m[terminal:{}] {}\u{1b}[0m\r\n",
                            input_session.id, err
                        ));
                    }
                }
                Ok(Message::Close(_)) => break,
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) | Ok(Message::Frame(_)) => {}
                Err(_) => break,
            }
        }
    });

    let _ = tokio::join!(writer, reader);
    Ok(())
}

async fn serve_terminal_socket_with_error<S>(
    mut ws_stream: tokio_tungstenite::WebSocketStream<S>,
    message: &str,
) -> io::Result<()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let _ = ws_stream
        .send(terminal_ws_text(format!(
            "\u{1b}[31m[terminal] {}\u{1b}[0m\r\n",
            message
        )))
        .await;
    let _ = ws_stream.close(None).await;
    Ok(())
}
