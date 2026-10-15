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
            .stderr(Stdio::null())
            .spawn()
            .map_err(SelectionError::Unavailable)?;

        let mut bytes = Vec::new();
        let read_result = child
            .stdout
            .take()
            .expect("stdout was piped")
            .take((MAX_SELECTION_BYTES + 1) as u64)
            .read_to_end(&mut bytes);

        if let Err(error) = read_result {
            let _ = child.kill();
            let _ = child.wait();
            return Err(SelectionError::Failed(error.to_string()));
        }
        if bytes.len() > MAX_SELECTION_BYTES {
            let _ = child.kill();
            let _ = child.wait();
            return Err(SelectionError::TooLarge);
        }

        let status = child
            .wait()
            .map_err(|error| SelectionError::Failed(error.to_string()))?;
        if !status.success() {
            return Err(SelectionError::Failed(format!(
                "wl-paste exited with {status}"
            )));
        }
        let text = String::from_utf8(bytes).map_err(|_| SelectionError::InvalidUtf8)?;
        if text.trim().is_empty() {
            return Err(SelectionError::Empty);
        }
        Ok(text)
    }
}
