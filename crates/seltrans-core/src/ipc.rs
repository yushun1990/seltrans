//! A small bounded per-user Unix socket transport for manual-resident mode.
//! Wire format: u128 invocation id, u32 UTF-8 byte length, selection bytes;
//! response: "OK" after the resident has queued the request, "ER" otherwise.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fs4::fs_std::FileExt;

pub const MAX_SELECTION_BYTES: usize = 64 * 1024;
const SOCKET_FILE: &str = "trigger.sock";
const LOCK_FILE: &str = "resident.lock";
const SOCKET_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub invocation_id: u128,
    /// Delivery order assigned by the resident, not by wall-clock timestamps.
    pub sequence: u64,
    pub text: String,
}

pub struct ResidentSocket {
    path: PathBuf,
    // The advisory lock prevents a second resident from deleting/rebinding
    // a live instance's socket, even during startup races.
    _lock: File,
}

impl Drop for ResidentSocket {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
        // Intentionally retain resident.lock: unlinking locked files allows
        // another process to lock a different inode at the same pathname.
    }
}

fn runtime_directory() -> io::Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "XDG_RUNTIME_DIR is unset"))?;
    Ok(PathBuf::from(runtime).join("seltrans"))
}

fn socket_directory() -> io::Result<PathBuf> {
    let dir = runtime_directory()?;
    // XDG_RUNTIME_DIR must already be private to the current user.
    fs::create_dir_all(&dir)?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    Ok(dir)
}

pub fn bind_resident() -> io::Result<(UnixListener, ResidentSocket)> {
    let dir = socket_directory()?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(dir.join(LOCK_FILE))?;
    let acquired = lock
        .try_lock_exclusive()
        .map_err(|error| io::Error::other(format!("cannot acquire the resident lock: {error}")))?;
    if !acquired {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "seltrans is already running",
        ));
    }

    let path = dir.join(SOCKET_FILE);
    if let Ok(meta) = fs::symlink_metadata(&path) {
        if !meta.file_type().is_socket() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "socket path exists but is not a Unix socket",
            ));
        }
        // With the instance lock held, any socket left by a crashed previous
        // process is safe to unlink before rebinding.
        fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    Ok((listener, ResidentSocket { path, _lock: lock }))
}

fn socket_path() -> io::Result<PathBuf> {
    Ok(runtime_directory()?.join(SOCKET_FILE))
}

pub fn send_to_resident(text: &str) -> io::Result<()> {
    if text.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "empty selection",
        ));
    }
    if text.len() > MAX_SELECTION_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "selection exceeds 64 KiB",
        ));
    }
    let path = socket_path()?;
    let mut stream = UnixStream::connect(&path).map_err(|error| {
        io::Error::new(
            io::ErrorKind::NotConnected,
            format!("seltrans is not running; start seltrans once first ({error})"),
        )
    })?;
    stream.set_write_timeout(Some(SOCKET_TIMEOUT))?;
    stream.set_read_timeout(Some(SOCKET_TIMEOUT))?;
    let invocation_id = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    stream.write_all(&invocation_id.to_be_bytes())?;
    stream.write_all(&(text.len() as u32).to_be_bytes())?;
    stream.write_all(text.as_bytes())?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut acknowledgement = [0; 2];
    stream.read_exact(&mut acknowledgement)?;
    if &acknowledgement != b"OK" {
        return Err(io::Error::other("resident rejected the selection request"));
    }
    Ok(())
}

fn read_invocation(reader: &mut impl Read, sequence: u64) -> io::Result<Invocation> {
    let mut invocation_bytes = [0u8; 16];
    reader.read_exact(&mut invocation_bytes)?;
    let mut length_bytes = [0u8; 4];
    reader.read_exact(&mut length_bytes)?;
    let length = u32::from_be_bytes(length_bytes) as usize;
    if length == 0 || length > MAX_SELECTION_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid selection length",
        ));
    }
    let mut payload = vec![0u8; length];
    reader.read_exact(&mut payload)?;
    let text = String::from_utf8(payload)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "non-UTF-8 selection"))?;
    if text.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "empty selection",
        ));
    }
    Ok(Invocation {
        invocation_id: u128::from_be_bytes(invocation_bytes),
        sequence,
        text,
    })
}

/// Delivery is decoupled from any specific queue: the GUI supplies an enqueue
/// callback (a bounded channel send), and "OK" still means the invocation was
/// accepted by that bounded queue, not merely that the socket accepted it.
fn process_connection<E>(mut stream: UnixStream, sequence: u64, enqueue: &E)
where
    E: Fn(Invocation) -> Result<(), Invocation>,
{
    let _ = stream.set_read_timeout(Some(SOCKET_TIMEOUT));
    let result = read_invocation(&mut stream, sequence).and_then(|request| {
        enqueue(request).map_err(|_| io::Error::other("resident queue is full or has no receiver"))
    });
    let _ = stream.write_all(if result.is_ok() { b"OK" } else { b"ER" });
}

/// This loop owns delivery sequence numbers. The GUI consumes arrivals as
/// main-context events and presents only the newest invocation of a burst.
pub fn serve<E>(listener: UnixListener, enqueue: E)
where
    E: Fn(Invocation) -> Result<(), Invocation> + Send + 'static,
{
    let mut sequence = 0_u64;
    for connection in listener.incoming() {
        match connection {
            Ok(stream) => {
                sequence = sequence.saturating_add(1);
                process_connection(stream, sequence, &enqueue);
            }
            Err(error) => eprintln!("seltrans: incoming trigger error: {error}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use std::sync::mpsc::sync_channel;

    fn frame(id: u128, text: &str) -> Vec<u8> {
        let mut bytes = id.to_be_bytes().to_vec();
        bytes.extend_from_slice(&(text.len() as u32).to_be_bytes());
        bytes.extend_from_slice(text.as_bytes());
        bytes
    }

    #[test]
    fn round_trip_preserves_multiline_utf8_and_id() {
        let mut wire = Cursor::new(frame(42, "Hello, 世界\nNext line"));
        let msg = read_invocation(&mut wire, 7).unwrap();
        assert_eq!(msg.invocation_id, 42);
        assert_eq!(msg.sequence, 7);
        assert_eq!(msg.text, "Hello, 世界\nNext line");
    }

    #[test]
    fn rejects_invalid_or_empty_payload_without_enqueuing() {
        assert!(read_invocation(&mut Cursor::new(frame(1, " \n")), 1).is_err());
        let mut over_limit = 1u128.to_be_bytes().to_vec();
        over_limit.extend_from_slice(&((MAX_SELECTION_BYTES + 1) as u32).to_be_bytes());
        assert!(read_invocation(&mut Cursor::new(over_limit), 1).is_err());
        let mut invalid = 1u128.to_be_bytes().to_vec();
        invalid.extend_from_slice(&1u32.to_be_bytes());
        invalid.push(0xff);
        assert!(read_invocation(&mut Cursor::new(invalid), 1).is_err());
    }

    #[test]
    fn acknowledgement_means_the_request_was_queued() {
        let (sender, receiver) = sync_channel(1);
        let (mut client, server) = UnixStream::pair().unwrap();
        let handler = std::thread::spawn(move || {
            let enqueue = move |invocation: Invocation| {
                sender.try_send(invocation).map_err(|error| match error {
                    std::sync::mpsc::TrySendError::Full(invocation)
                    | std::sync::mpsc::TrySendError::Disconnected(invocation) => invocation,
                })
            };
            process_connection(server, 3, &enqueue);
        });
        client.write_all(&frame(99, "text A")).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        let mut acknowledgement = [0; 2];
        client.read_exact(&mut acknowledgement).unwrap();
        handler.join().unwrap();
        assert_eq!(&acknowledgement, b"OK");
        assert_eq!(receiver.try_recv().unwrap().text, "text A");
    }
}
