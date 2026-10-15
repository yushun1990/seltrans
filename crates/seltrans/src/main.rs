use std::process::ExitCode;

use seltrans_core::ipc::{self, Invocation};

mod ui;

fn main() -> ExitCode {
    if let Some(command) = std::env::args().nth(1) {
        match command.as_str() {
            "--help" | "-h" => {
                println!(
                    "Usage: seltrans [run]\n  run  Start once and remain resident (default)\nThe Alt+Q trigger is the separate seltrans-trigger executable."
                );
                return ExitCode::SUCCESS;
            }
            "run" => {}
            other => {
                eprintln!("seltrans: unknown command '{other}'; use seltrans --help");
                return ExitCode::FAILURE;
            }
        }
    }
    match run_resident() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("seltrans: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run_resident() -> Result<(), String> {
    let (listener, socket_guard) = ipc::bind_resident().map_err(|e| e.to_string())?;
    // Arrivals are delivered as main-context events instead of a polling
    // timeout, so the GTK loop can sleep fully between triggers.
    let (sender, receiver) = async_channel::bounded::<Invocation>(64);
    std::thread::spawn(move || {
        ipc::serve(listener, move |invocation| {
            sender.try_send(invocation).map_err(|error| match error {
                async_channel::TrySendError::Full(invocation)
                | async_channel::TrySendError::Closed(invocation) => invocation,
            })
        })
    });
    println!(
        "seltrans is resident. Set GNOME Alt+Q to seltrans-trigger; Ctrl+C stops this process."
    );
    ui::run(receiver);
    // Do not remove the socket until the GTK resident instance has stopped.
    drop(socket_guard);
    Ok(())
}
