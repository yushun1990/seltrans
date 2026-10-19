# Roadmap

The roadmap is organized as vertical slices. A milestone should produce a usable behavior, not merely another abstraction layer.

## M0 — Repository bootstrap

- [x] Product vision and requirements
- [x] Architecture boundaries
- [x] Initial roadmap
- [x] Minimal Rust crate
- [x] CI: format / check / test (workflow added; green run pending)
- [x] Development prerequisites for Fedora documented
- [x] Initial UI wireframe and interaction ADR recorded

Exit condition: a clean baseline repository that agents and humans can build on without re-inventing product intent.

## M1 — Selection to native window

Implementation proposed in the M1 PR; Fedora desktop acceptance is still pending.
See [M1 setup and manual acceptance](M1.md).

Goal:

```text
start seltrans once -> select text -> Alt+Q -> send selection to existing GTK window
```

Tasks:

- [x] SelectionProvider trait
- [x] `wl-paste --primary --no-newline` backend
- [x] resident GTK application and a lightweight CLI/trigger invocation path
- [x] local IPC (private Unix socket)
- [x] capture selection before focus shifts to the popup
- [x] reuse the same result window on every repeated invocation (code path; desktop check pending)
- [x] manual-resident startup behavior and clear not-running error
- [ ] integration tests for repeated triggers, stale provider results, window reuse (manual checks pending)
- [x] empty-selection error
- [x] word/passage classifier
- [x] GTK4/libadwaita application shell
- [x] provider loading/result/error states (delivered in M2)
- [x] documented GNOME `Alt+Q` custom-shortcut setup

Exit condition: after one manual start, repeated shortcuts show the current selection in the same GTK window, without repeating GUI initialization or creating duplicate windows.

## M2 — Passage translation

Implementation proposed in the M2 PR; Fedora desktop acceptance is still pending.
See [M2 setup and manual acceptance](M2.md).

Goal:

```text
select passage -> Alt+Q -> Chinese translation
```

Tasks:

- [x] config loading
- [x] OpenAI-compatible provider
- [x] API key from environment
- [x] async request without blocking GTK
- [x] timeout/cancellation (per-request timeout; stale completions fenced by invocation sequence)
- [x] structured response validation
- [x] original + translation view

Exit condition: useful passage translation works end to end (code complete; desktop check pending).

## M3 — Reading analysis

Implementation proposed in the M3 PR; Fedora desktop acceptance is still pending.
See [M3 setup and manual acceptance](M3.md).

Goal:

```text
passage -> translation + selected difficult words + useful expressions
```

Tasks:

- [x] structured analysis schema v1
- [x] selective vocabulary extraction
- [x] idioms
- [x] slang
- [x] phrasal verbs
- [x] collocations
- [x] contextual meanings
- [x] sentence patterns
- [x] technical/non-literal usages
- [x] compact passage UI

Guardrail:

> Vocabulary is selective; expressions are exhaustive.

Exit condition: the result teaches useful language without dumping every token (code complete; desktop check pending).

## Post-M3 — Result window polish

Presentation-only slice taken before M4: the resident window should read
as a finished GNOME app rather than a debug view.

- [x] first-run status page explaining the select-then-Alt+Q flow
- [x] card-based source/translation sections inside an `AdwClamp` reading column
- [x] boxed-list vocabulary/expression rows with kind pills
- [x] error banner with one-click retry; empty-selection notice
- [x] window subtitle reflecting passage/word mode

Exit condition: analysis, fencing and resident behavior unchanged; only
presentation and error surfacing changed.

## M4 — Single-word experience

Goal:

```text
select one word -> detailed lexical/context view
```

Tasks:

- lemma normalization
- DictionaryProvider boundary
- reliable IPA source
- US/UK variants where available
- part of speech
- common definitions
- contextual meaning from analysis provider
- examples
- pronunciation playback

Exit condition: single-word lookup is fast and materially better than generic translation.

## M5 — Personal notebook

Goal:

```text
result -> save useful item -> find it later with original context
```

Tasks:

- SQLite repository
- XDG data path
- LearningItem model
- item kinds beyond words
- save action
- duplicate handling
- context history
- notebook browse/search UI

Exit condition: saved knowledge is durable and retains why it mattered.

## M6 — Pronunciation and speech polish

- dictionary audio where available
- SpeechProvider abstraction
- system/local TTS fallback
- sentence playback
- playback state/error UX

Exit condition: pronunciation is dependable enough for daily learning use.

## M7 — Personal difficulty model

- known/learning/mastered state
- avoid repeatedly surfacing mastered vocabulary
- combine lexical difficulty + user history + context
- tune selection density

Exit condition: analysis becomes visibly personalized to the learner.

## M8 — Review

- FSRS integration
- review state
- review events
- words + expressions + patterns
- daily review UX

Exit condition: saved reading discoveries can be retained through spaced repetition.

## M9 — Latency optimization

Measure and tune the already resident architecture; **do not defer residency or IPC until M9**.

Potential work:

- persistent HTTP client reuse
- dictionary cache
- warm-request performance instrumentation
- prewarmed model/provider connection where possible

Exit condition: common invocations feel immediate, without unnecessary architectural complexity.

## Later / optional

- native Wayland selection backend
- OCR / screenshot translation
- browser integration
- import/export
- Anki interoperability
- additional desktop environments
- other source/target languages
- sync

These should not delay the core English-reading workflow.
