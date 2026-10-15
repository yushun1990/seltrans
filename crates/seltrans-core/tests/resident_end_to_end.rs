//! End-to-end IPC over a real per-user socket directory: trigger requests
//! reach the bounded queue with an "OK" acknowledgement, duplicate residents
//! are rejected while the lock is held, and the socket is reusable after a
//! clean shutdown even though resident.lock is intentionally retained.

use std::sync::mpsc::{self, TrySendError};
use std::time::Duration;

use seltrans_core::ipc::{self, Invocation};

fn enqueue_via(
    sender: mpsc::SyncSender<Invocation>,
) -> impl Fn(Invocation) -> Result<(), Invocation> {
    move |invocation: Invocation| {
        sender.try_send(invocation).map_err(|error| match error {
            TrySendError::Full(invocation) | TrySendError::Disconnected(invocation) => invocation,
        })
    }
}

#[test]
fn trigger_round_trip_duplicate_rejection_and_rebind() {
    let runtime = std::env::temp_dir().join(format!("seltrans-ipc-test-{}", std::process::id()));
    std::fs::create_dir_all(&runtime).unwrap();
    // SAFETY: only this test reads or writes XDG_RUNTIME_DIR in this binary,
    // and it runs as the sole test, so there is no concurrent env access.
    unsafe { std::env::set_var("XDG_RUNTIME_DIR", &runtime) };

    let (listener, socket) = ipc::bind_resident().expect("first resident binds");
    let (sender, received) = mpsc::sync_channel(4);
    std::thread::spawn(move || ipc::serve(listener, enqueue_via(sender)));

    ipc::send_to_resident("hello end to end").expect("trigger is acknowledged");
    let invocation = received
        .recv_timeout(Duration::from_secs(2))
        .expect("queued invocation");
    assert_eq!(invocation.text, "hello end to end");
    assert_eq!(invocation.sequence, 1);

    assert!(
        ipc::bind_resident().is_err(),
        "a duplicate resident must be rejected without disrupting the first"
    );

    // Clean shutdown keeps the lock file on disk but releases its flock.
    drop(socket);
    let (listener, socket) = ipc::bind_resident().expect("rebind after clean shutdown");
    let (sender, received) = mpsc::sync_channel(4);
    std::thread::spawn(move || ipc::serve(listener, enqueue_via(sender)));

    ipc::send_to_resident("after restart").expect("second trigger is acknowledged");
    let invocation = received
        .recv_timeout(Duration::from_secs(2))
        .expect("queued invocation after restart");
    assert_eq!(invocation.text, "after restart");
    assert_eq!(
        invocation.sequence, 1,
        "sequence numbering restarts per resident"
    );
    drop(socket);
}
