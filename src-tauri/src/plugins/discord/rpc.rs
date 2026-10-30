//! Discord's local IPC: length-prefixed JSON frames over the
//! `discord-ipc-N` named pipe (Windows) or Unix socket.

use serde_json::Value;
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::mpsc,
};

use super::Command;

// Opcodes of Discord's IPC frames
pub(super) const OP_HANDSHAKE: u32 = 0;
pub(super) const OP_FRAME: u32 = 1;
pub(super) const OP_CLOSE: u32 = 2;
pub(super) const OP_PING: u32 = 3;
pub(super) const OP_PONG: u32 = 4;

pub(super) type Writer = Box<dyn AsyncWrite + Send + Unpin>;

pub(super) async fn write_frame(writer: &mut Writer, op: u32, payload: &Value) -> std::io::Result<()> {
    let body = serde_json::to_vec(payload)?;
    let mut frame = Vec::with_capacity(8 + body.len());
    frame.extend_from_slice(&op.to_le_bytes());
    frame.extend_from_slice(&(body.len() as u32).to_le_bytes());
    frame.extend_from_slice(&body);
    writer.write_all(&frame).await?;
    writer.flush().await
}

pub(super) async fn read_frames(
    mut reader: Box<dyn AsyncRead + Send + Unpin>,
    generation: u64,
    commands: mpsc::UnboundedSender<Command>,
) {
    loop {
        let mut header = [0u8; 8];
        if reader.read_exact(&mut header).await.is_err() {
            break;
        }
        let op = u32::from_le_bytes(header[..4].try_into().unwrap());
        let length = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
        let mut body = vec![0u8; length];
        if reader.read_exact(&mut body).await.is_err() {
            break;
        }
        let payload = serde_json::from_slice(&body).unwrap_or(Value::Null);
        if commands.send(Command::Incoming(generation, Some((op, payload)))).is_err() {
            return;
        }
    }
    let _ = commands.send(Command::Incoming(generation, None));
}

type Halves = (Box<dyn AsyncRead + Send + Unpin>, Writer);

/// Discord listens on `discord-ipc-0` to `-9`, the first free one.
#[cfg(windows)]
pub(super) async fn open_socket() -> std::io::Result<Halves> {
    use tokio::net::windows::named_pipe::ClientOptions;
    let mut last_error = None;
    for index in 0..10 {
        match ClientOptions::new().open(format!(r"\\.\pipe\discord-ipc-{index}")) {
            Ok(pipe) => {
                let (reader, writer) = tokio::io::split(pipe);
                return Ok((Box::new(reader), Box::new(writer)));
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.expect("tried at least one pipe"))
}

#[cfg(unix)]
pub(super) async fn open_socket() -> std::io::Result<Halves> {
    let base = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .iter()
        .find_map(|key| std::env::var(key).ok())
        .unwrap_or_else(|| "/tmp".to_owned());
    let mut last_error = None;
    for index in 0..10 {
        match tokio::net::UnixStream::connect(format!("{base}/discord-ipc-{index}")).await {
            Ok(stream) => {
                let (reader, writer) = stream.into_split();
                return Ok((Box::new(reader), Box::new(writer)));
            }
            Err(error) => last_error = Some(error),
        }
    }
    Err(last_error.expect("tried at least one socket"))
}

