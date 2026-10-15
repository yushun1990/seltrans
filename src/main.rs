use std::process::ExitCode;
use std::sync::mpsc::sync_channel;

use seltrans::ipc;
use seltrans::selection::{SelectionProvider, WlPasteSelection};
use seltrans::ui;

fn main() -> ExitCode {
    let command = std::env::args().nth(1);
    let result = match command.as_deref() {
        None | Some("run") => run_resident(),
        Some("trigger") => WlPasteSelection
            .read_selection()
            .map_err(|error| error.to_string())
            .and_then(|text| ipc::send_to_resident(&text).map_err(|error| error.to_string())),
        Some("--help" | "-h") => {
            println!("Usage: seltrans [run|trigger]\n  run      Start once and remain resident (default)\n  trigger  Capture the Wayland primary selection and send it to the resident instance");
            return ExitCode::SUCCESS;
        }
        _ => Err("unknown command; use seltrans --help".to_owned()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("seltrans: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_resident() -> Result<(), String> {
    let (listener, socket_guard) = ipc::bind_resident().map_err(|e| e.to_string())?;
    let (sender, receiver) = sync_channel(64);
    std::thread::spawn(move || ipc::serve(listener, sender));
    println!("seltrans is resident. Set GNOME Alt+Q to seltrans trigger; Ctrl+C stops this process.");
    ui::run(receiver);
    // Do not remove the socket until the GTK resident instance has stopped.
    drop(socket_guard);
    Ok(())
}
