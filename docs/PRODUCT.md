# Product Requirements

## 1. Product statement

seltrans is a context-aware English reading assistant for Linux.

It lets the user select a word, sentence, or passage in any application, invoke one desktop shortcut, understand the selection in context, discover the language knowledge worth learning, and optionally save that knowledge for later review.

The product is built around real reading rather than isolated vocabulary study.

```text
Reading -> Understand -> Discover -> Capture -> Remember
```

## 2. Target user

The initial target user:

- reads English technical books, documentation, GitHub discussions, terminal output, and web content;
- can understand part of the text but frequently needs contextual translation;
- wants translation to contribute to long-term English learning;
- prefers a keyboard-driven workflow;
- uses Fedora / GNOME / Wayland.

## 3. Primary interaction

The user:

1. selects English text in the current application;
2. presses a desktop-global shortcut (initial recommendation: `Alt+Q`);
3. sees a result window;
4. optionally plays pronunciation or saves one or more learning items;
5. closes the result and continues reading.

seltrans MUST NOT automatically translate on clipboard or selection changes.

The desktop environment owns global shortcut registration. v0.1 does not implement its own global-hotkey daemon.

## 4. Selection modes

Mode selection is automatic. The user should not choose "word mode" or "passage mode" manually.

### 4.1 Single-word mode

A selection that represents one lexical item should open a detailed lexical view.

Required information:

- selected form;
- lemma / canonical form;
- part of speech;
- concise common meanings;
- meaning in the current context, when context is available;
- US/UK IPA when a reliable source provides it;
- pronunciation playback when available;
- one or more useful examples;
- source sentence/context when available;
- save-to-notebook action.

Inflected forms should be normalized when appropriate, for example:

```text
acknowledged -> acknowledge
running      -> run
```

The UI should still preserve the exact text the user selected.

### 4.2 Passage mode

A sentence or multi-word passage should prioritize comprehension first.

Required sections:

1. Original text.
2. Natural Simplified Chinese translation.
3. Selected vocabulary.
4. Expressions and language notes.

The vocabulary section MUST NOT mechanically list every word.

It should contain a small number of words that are relatively difficult, uncommon, technically important, or contextually surprising.

The expression analysis should try to identify all materially useful items in the passage, including:

- phrasal verbs;
- idioms;
- slang;
- collocations;
- fixed expressions;
- context-specific meanings;
- non-literal meanings;
- sentence patterns whose meaning cannot be reliably inferred word by word;
- technically conventional meanings that differ from everyday usage.

Product rule:

> Vocabulary is selective; expressions are exhaustive.

"Exhaustive" means the analyzer should prefer recall over aggressive filtering for meaningful expressions. It does not mean inventing questionable language phenomena.

## 5. Context-aware interpretation

Context is a first-class product concept.

For example, a generic dictionary may define `acknowledge` as "admit or recognize", while in:

```text
The processor acknowledges the interrupt.
```

the useful interpretation is closer to "respond to / confirm the interrupt".

seltrans should preserve both lexical knowledge and the meaning relevant to the current reading context.

## 6. Learning items

The notebook stores `LearningItem` objects rather than only "words".

Supported kinds:

- Word
- PhrasalVerb
- Idiom
- Collocation
- Expression
- SentencePattern

A saved learning item should retain enough context to answer:

> Why did I save this when I was reading?

Expected stored information includes, where applicable:

- canonical form;
- selected surface form;
- kind;
- IPA / pronunciation references;
- part of speech;
- concise meaning;
- contextual meaning;
- source text or source sentence;
- translation of the source text;
- example sentence;
- creation time;
- learning/review state.

## 7. Vocabulary selection

Passage-mode vocabulary selection should eventually consider:

- word frequency;
- learner difficulty (for example CEFR-like levels where reliable data exists);
- technical importance;
- contextual unusualness;
- whether the item is already known or under study;
- LLM-based semantic judgment.

The system should progressively avoid repeatedly surfacing items the user has already mastered.

For v0.1, a simpler LLM-assisted selection strategy is acceptable, but the architecture must not hard-code "LLM output equals truth".

## 8. Expressions and language notes

The system should surface useful structures even when every individual word is common.

Example:

```text
This is not so much a performance problem as a correctness problem.
```

The useful learning item is:

```text
not so much A as B
= 与其说是 A，不如说是 B
```

This category is important because literal translation and single-word dictionaries often fail to teach it.

## 9. Translation

Initial target:

- English -> Simplified Chinese.

Translation should:

- preserve technical identifiers, commands, code, and proper names;
- prefer natural Chinese over word-by-word rendering;
- preserve technical meaning;
- avoid unnecessary explanation in the translation itself.

Explanations belong in language notes.

## 10. Lexical facts and LLM boundaries

LLMs are useful for:

- contextual translation;
- contextual meaning;
- identifying noteworthy vocabulary;
- detecting idioms, slang, collocations, and special structures;
- generating or selecting context-relevant examples.

LLMs MUST NOT be treated as the authoritative source for:

- IPA;
- canonical pronunciation;
- dictionary headword metadata when a reliable lexical source is available.

The product should keep dictionary, LLM, and speech providers conceptually separate.

## 11. Pronunciation

The detailed lexical view should support:

- US pronunciation when available;
- UK pronunciation when available;
- playback for words;
- later: playback for complete example/source sentences.

Speech should be behind a provider boundary so the implementation can use:

- dictionary audio;
- system TTS;
- local TTS such as Piper;
- remote TTS services.

## 12. Personal notebook

Storage is local-first.

Initial storage target:

- SQLite under the user's XDG data directory.

The notebook should support:

- add item;
- remove/archive item;
- browse/search;
- detect duplicates;
- retain multiple contexts for the same canonical item where useful;
- track review state.

Cloud accounts and synchronization are explicitly out of scope for v0.1.

## 13. Review

A later milestone adds spaced repetition.

Preferred scheduling algorithm:

- FSRS.

Review should support learning items beyond words, including idioms and sentence patterns.

Example card:

```text
Front:
not so much A as B

Back:
与其说是 A，不如说是 B
This is not so much a design problem as an implementation problem.
```

## 14. Desktop UX requirements

Initial platform:

- Fedora;
- GNOME;
- Wayland.

The result UI should be a small native GTK4/libadwaita window or popover-like window optimized for keyboard use.

Desired interactions:

- `Esc`: close;
- `Ctrl+C`: copy selected/result text where conventional;
- pronunciation buttons;
- one-click / one-key save;
- minimal visual interruption to reading.

The result window should remain useful for both one word and several paragraphs.

## 15. Selection capture

Initial implementation may call:

```bash
wl-paste --primary --no-newline
```

This dependency is acceptable for the first vertical slice.

Selection capture must be behind an interface so a native Wayland implementation or other platforms can be added later.

The program must only read the selection after explicit invocation.

## 16. Configuration

Planned configuration path:

```text
~/.config/seltrans/config.toml
```

Configuration should eventually include:

- target language;
- provider;
- endpoint;
- model;
- API-key environment variable name;
- dictionary provider;
- speech provider;
- UI preferences;
- vocabulary-analysis preferences.

Secrets should not be committed to configuration examples or source control.

## 17. Provider requirements

### Translation / analysis provider

First-class target:

- OpenAI-compatible HTTP API.

Later possibilities:

- Ollama/local OpenAI-compatible endpoints;
- other LLM APIs through adapters.

### Dictionary provider

Must be swappable.

Responsibilities may include:

- lemma/headword;
- IPA;
- part of speech;
- dictionary meanings;
- pronunciation audio references.

### Speech provider

Must be swappable and independent from translation.

## 18. Local data and privacy

Default principle:

- learning history remains local;
- only the text required for a configured remote provider is sent remotely;
- the UI/settings should eventually make this boundary understandable.

The project should avoid storing API keys in the SQLite learning database.

## 19. Performance requirements

The interaction is latency-sensitive.

Desired behavior:

- selection capture should feel immediate;
- the UI should appear promptly;
- network analysis may complete asynchronously;
- repeated invocations should not require expensive process initialization once a resident architecture is introduced.

v0.1 may be a one-shot process.

If startup + TLS + GTK initialization becomes a noticeable part of latency, v0.2 may introduce a resident process with a lightweight IPC command.

Do not introduce a daemon before measurement shows it is useful.

## 20. Error handling

The user should get a useful, compact error for:

- empty selection;
- unavailable Wayland primary selection;
- provider timeout;
- invalid credentials;
- malformed provider response;
- offline state;
- dictionary lookup failure;
- speech failure.

A dictionary failure should not necessarily make passage translation fail, and vice versa.

## 21. v0.1 acceptance criteria

The first useful vertical slice is complete when, on Fedora/GNOME/Wayland:

1. the user selects English text in an application;
2. a GNOME shortcut invokes seltrans;
3. seltrans reads the primary selection;
4. seltrans determines word vs passage mode;
5. passage mode returns Chinese translation plus selected vocabulary and useful expressions;
6. word mode returns a detailed lexical/context view;
7. a GTK4/libadwaita result window presents the response;
8. at least one learning item can be saved to local SQLite;
9. the saved item retains its source context;
10. no clipboard-change monitoring is required.

## 22. Explicit non-goals for v0.1

- OCR or screenshot translation;
- browser extensions;
- mobile clients;
- cloud synchronization;
- user accounts;
- custom hotkey daemon;
- automatic clipboard watchers;
- full dictionary authoring;
- Anki compatibility;
- full learning analytics;
- dozens of translation providers;
- Windows/macOS parity.
