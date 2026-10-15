# Roadmap

The roadmap is organized as vertical slices. A milestone should produce a usable behavior, not merely another abstraction layer.

## M0 — Repository bootstrap

- [x] Product vision and requirements
- [x] Architecture boundaries
- [x] Initial roadmap
- [x] Minimal Rust crate
- [ ] CI: format / check / test
- [ ] Development prerequisites for Fedora documented

Exit condition: a clean baseline repository that agents and humans can build on without re-inventing product intent.

## M1 — Selection to native window

Goal:

```text
select text -> invoke seltrans -> see the exact selection in GTK
```

Tasks:

- SelectionProvider trait
- `wl-paste --primary --no-newline` backend
- empty-selection error
- word/passage classifier
- GTK4/libadwaita application shell
- loading/result/error states
- documented GNOME `Alt+Q` custom-shortcut setup

Exit condition: the shortcut reliably opens a native result window containing the current selection.

## M2 — Passage translation

Goal:

```text
select passage -> Alt+Q -> Chinese translation
```

Tasks:

- config loading
- OpenAI-compatible provider
- API key from environment
- async request without blocking GTK
- timeout/cancellation
- structured response validation
- original + translation view

Exit condition: useful passage translation works end to end.

## M3 — Reading analysis

Goal:

```text
passage -> translation + selected difficult words + useful expressions
```

Tasks:

- structured analysis schema v1
- selective vocabulary extraction
- idioms
- slang
- phrasal verbs
- collocations
- contextual meanings
- sentence patterns
- technical/non-literal usages
- compact passage UI

Guardrail:

> Vocabulary is selective; expressions are exhaustive.

Exit condition: the result teaches useful language without dumping every token.

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

Only after measurement.

Potential work:

- persistent HTTP client reuse
- dictionary cache
- resident application process
- D-Bus or Unix-socket activation
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
