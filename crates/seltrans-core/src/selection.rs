//! Explicit user-triggered selection capture (never passive monitoring).

use std::io::{self, Read};
use std::process::{Command, Stdio};

use crate::ipc::MAX_SELECTION_BYTES;

pub trait SelectionProvider {
    fn read_selection(&self) -> Result<String, SelectionError>;
}

#[derive(Debug)]
pub enum SelectionError {
    Unavailable(io::Error),
    Failed(String),
    TooLarge,
    InvalidUtf8,
    Empty,
}

impl std::fmt::Display for SelectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(error) => {
                write!(f, "cannot start wl-paste: {error} (install wl-clipboard)")
            }
            Self::Failed(reason) => write!(f, "cannot read Wayland primary selection: {reason}"),
            Self::TooLarge => write!(f, "selection exceeds the 64 KiB limit"),
            Self::InvalidUtf8 => write!(f, "selected text is not valid UTF-8"),
            Self::Empty => write!(
                f,
                "nothing is selected; select some text and press Alt+Q again"
            ),
        }
    }
}

impl std::error::Error for SelectionError {}

pub struct WlPasteSelection;

impl SelectionProvider for WlPasteSelection {
    fn read_selection(&self) -> Result<String, SelectionError> {
        let mut child = Command::new("wl-paste")
            .args(["--primary", "--no-newline"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(SelectionError::Unavailable)?;

        let mut bytes = Vec::new();
        let read_result = child
            .stdout
            .take()
            .expect("stdout was piped")
            .take((MAX_SELECTION_BYTES + 1) as u64)
            .read_to_end(&mut bytes);

        // Taken before wait() so the pipe stays readable afterwards; drained
        // only once the child has exited, when EOF is guaranteed.
        let mut stderr = child.stderr.take().expect("stderr was piped");

        if let Err(error) = read_result {
            let _ = child.kill();
            let _ = child.wait();
            let reason = with_stderr(&error.to_string(), &drain_stderr(&mut stderr));
            return Err(SelectionError::Failed(reason));
        }
        if bytes.len() > MAX_SELECTION_BYTES {
            let _ = child.kill();
            let _ = child.wait();
            return Err(SelectionError::TooLarge);
        }

        let status = child
            .wait()
            .map_err(|error| SelectionError::Failed(error.to_string()))?;
        let wl_paste_reason = with_stderr(
            &format!("wl-paste exited with {status}"),
            &drain_stderr(&mut stderr),
        );
        if !status.success() {
            return Err(SelectionError::Failed(wl_paste_reason));
        }
        let text = String::from_utf8(bytes).map_err(|_| SelectionError::InvalidUtf8)?;
        if text.trim().is_empty() {
            return Err(SelectionError::Empty);
        }
        Ok(text)
    }
}

/// wl-paste's own diagnostic (for example: not a Wayland session, or the
/// compositor lacks primary-selection support) is the most useful reason a
/// capture failed, so keep a trimmed line of it for failure messages.
fn drain_stderr(stderr: &mut impl Read) -> String {
    let mut buffer = Vec::new();
    let _ = stderr.read_to_end(&mut buffer);
    String::from_utf8_lossy(&buffer)
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .to_owned()
}

fn with_stderr(reason: &str, stderr: &str) -> String {
    if stderr.is_empty() {
        reason.to_owned()
    } else {
        format!("{reason}: {stderr}")
    }
}
