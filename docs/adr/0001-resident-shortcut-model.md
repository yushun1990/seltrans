# ADR 0001 — Resident instance and selection-trigger IPC

Status: **Accepted**  
Date: 2026-10-15

## Context

seltrans serves frequent, repeated lookups while the user reads. Each `Alt+Q` must feel like sending new selected text to an already running assistant, not like launching a desktop app and then waiting for it to initialize. A one-shot GUI flow is not acceptable even if convenient to prototype.

On GNOME/Wayland, selection must be captured while the original application still owns the focus/selection. Showing the seltrans window first risks changing what primary selection yields.

## Decision

Adopt a **resident single-instance application + lightweight trigger** model starting in M1.

1. The user starts `seltrans` once. For the initial version, manual startup is acceptable and preferred to unreliable automatic startup.
2. GNOME registers `Alt+Q` as its global shortcut. The app does not need to own a system-wide hotkey.
3. The lightweight trigger obtains the **current** primary selection **before** presenting or focusing the seltrans window. The trigger must never send a stale copied selection without making that state visible.
4. The trigger passes the selection and an invocation ID to the already running instance via local IPC.
5. The resident instance reuses its GTK application, providers, caches, state, and **one existing result window**; it updates the content for this request rather than constructing a new window or reinitializing GTK.
6. The main window can be hidden and shown again. An explicit Quit ends the resident process.
7. A newer invocation supersedes an older one; asynchronous older results must not overwrite current content.

The trigger can be `seltrans trigger` or a tiny `seltrans-trigger` executable. Prefer the simplest reliable single-instance mechanism compatible with GTK/GIO/GNOME, for example application IPC over the session D-Bus or a Unix-domain socket. Do not introduce a general-purpose daemon framework merely to satisfy this requirement.

## Startup and unavailable-app behavior

Initial default: **manual-resident**.

- If the app is not running, the trigger returns a clear not-running error or lightweight desktop notification.
- Do **not** start a new full GTK process for every `Alt+Q`.
- Optional **auto-start-resident** may be added later, but it must launch at most one persistent instance, then reuse it. It must not turn the trigger into a per-request app launcher.

## Interaction and failure constraints

- No clipboard monitoring or implicit translation.
- Primary selection must be read on explicit trigger.
- Never fake success from a successful IPC send while no receiving instance exists.
- Do not let UI focus change invalidate the selected text.
- Do not block GTK's main thread on provider/network calls.
- Report selection/IPC errors distinctly from translation-provider errors.
- Reuse one window; do not accumulate windows after repeated invocations.
- Handle rapid repeated invocations deterministically, with cancellation or generation fencing for stale completions.
- No persistence of clipboard/selection history unless explicitly saved by the user.

## Non-goals

- A background system service.
- A self-managed global-hotkey registration mechanism.
- Cross-desktop platform support in M1.
- A guarantee that network LLM response is instantaneous.

## Acceptance tests

1. Start `seltrans` once manually.
2. Select text A in Papers; press `Alt+Q`; the already running result window shows A.
3. Select text B in Firefox; press `Alt+Q`; **the same window** shows B, without GTK cold start or a second application window.
4. Trigger rapidly with A/B/C and delay A's provider response: only C may update the current result.
5. Hide the result window; invoke `Alt+Q` again: the hidden window is reused.
6. Stop the resident process; invoke `Alt+Q`: the trigger fails visibly without repeatedly cold-starting the GUI.
7. Verify no clipboard listener is active and the source text is captured before window focus changes.

## Consequences

We accept the one-time startup/persistent-process cost to make the repeated reading workflow fast and predictable. M1 must implement the warm-path architecture before LLM/persistence features are layered on top. Performance work in later milestones concerns provider/client optimization, not a postponed migration from per-request GTK startup.
