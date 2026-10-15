//! The Alt+Q warm path: capture the Wayland primary selection and forward it
//! to the resident instance. This binary lives in the GUI-free core crate, so
//! it cannot link GTK/libadwaita by construction.

use std::process::ExitCode;

use seltrans_core::ipc;
use seltrans_core::selection::{SelectionProvider, WlPasteSelection};

fn main() -> ExitCode {
    let result = WlPasteSelection
        .read_selection()
        .map_err(|error| error.to_string())
        .and_then(|text| ipc::send_to_resident(&text).map_err(|error| error.to_string()));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("seltrans: {error}");
            ExitCode::FAILURE
        }
    }
}
