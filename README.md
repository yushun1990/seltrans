# seltrans

> A context-aware English reading assistant that translates selected text, explains difficult words and expressions, and turns them into a personal vocabulary and review system.

seltrans is a Linux-first English reading companion designed for people who learn English while reading real technical documents, books, web pages, source-code discussions, and terminal output.

The core interaction is deliberately small:

```text
select English text -> press a global shortcut -> understand -> capture what is worth learning
```

seltrans is **not just a translator**. Translation is the entry point. The product goal is:

```text
Reading -> Understand -> Discover -> Capture -> Remember
```

## Product principles

- **Explicit invocation only.** seltrans never translates merely because the clipboard changed.
- **One shortcut everywhere.** The desktop environment owns the global shortcut; seltrans only handles the requested selection.
- **Single-word and passage selections behave differently.**
- **Vocabulary is selective; expressions are exhaustive.**
- **Context matters.** A word's meaning in the selected sentence is more important than a generic first dictionary definition.
- **Dictionary facts and LLM reasoning are separate.** IPA, lemma, pronunciation metadata and other lexical facts should come from reliable lexical sources when available; LLMs must not silently invent authoritative phonetics.
- **Learning data belongs to the user.** Vocabulary and review history are stored locally.
- **Linux / GNOME / Wayland first**, with portable interfaces where practical.

## Core user experience

### Single-word selection

Selecting one word should show:

- lemma / canonical form;
- US/UK IPA when available;
- pronunciation playback;
- part of speech;
- concise common meanings;
- the meaning in the current context;
- source sentence when context is available;
- useful example sentences;
- one-click save to the personal learning notebook.

### Passage selection

Selecting a sentence or paragraph should show:

1. the original text;
2. a natural Simplified Chinese translation;
3. a **small, selective** set of difficult, uncommon, or contextually important words;
4. all useful language phenomena detected in the passage, including:
   - phrasal verbs;
   - idioms;
   - slang;
   - collocations;
   - context-specific meanings;
   - non-literal expressions;
   - sentence patterns whose meaning is not obvious from word-by-word translation.

Common words should not be mechanically listed just because they occur in the selection.

## Personal learning notebook

Saved items are not limited to words. A learning item may be:

- a word;
- a phrasal verb;
- an idiom;
- a collocation;
- an expression;
- a sentence pattern.

Each saved item should retain the reading context that caused the user to save it.

Planned review uses spaced repetition (FSRS) and local review history.

## Initial platform target

- Fedora / GNOME / Wayland
- Rust
- GTK4 / libadwaita
- local SQLite storage
- OpenAI-compatible LLM providers first
- pluggable dictionary and speech providers
- GNOME global shortcut example: `Alt+Q`
- selection capture initially via `wl-paste --primary`

The global shortcut is intentionally configured by the desktop environment rather than registered by seltrans itself.

## Repository documents

- [Product requirements](docs/PRODUCT.md)
- [Architecture](docs/ARCHITECTURE.md)
- [Roadmap](docs/ROADMAP.md)

## Status

Early design / bootstrap stage. The first milestone is a thin vertical slice:

```text
Alt+Q
  -> read current selection
  -> classify word vs passage
  -> translate / analyze
  -> show GTK result window
  -> optionally save a learning item locally
```

## Non-goals for v0.1

- OCR
- browser extensions
- automatic clipboard monitoring
- full Anki replacement
- account/cloud sync
- custom global-shortcut daemon
- dozens of translation providers
- learning analytics dashboards

These may be considered later only if the core reading workflow proves useful.
