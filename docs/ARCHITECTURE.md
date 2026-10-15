# Architecture

## 1. Architecture goal

Keep the first version small while preserving boundaries that matter for correctness, latency, and future replacement of external services.

The system is organized around one explicit user action:

```text
desktop shortcut
    -> selection capture
    -> selection classification
    -> analysis pipeline
    -> native result UI
    -> optional learning persistence
```

## 2. Initial runtime shape

**Accepted ADR:** [0001 — Resident shortcut model](adr/0001-resident-shortcut-model.md).

The user launches `seltrans` once. The running application owns GTK, its one reusable result window, current request state, configuration, provider clients, and learning storage. A lightweight per-invocation trigger — the separate `seltrans-trigger` executable — reads the active selection **before any focus change** and sends that exact text to the resident instance over local IPC.

```text
GNOME Alt+Q
    |
    v
lightweight trigger -- read current selection BEFORE changing focus
    |
    +--> IPC request (selection + request id)
              |
              v
      resident seltrans process (started once)
              |
              +--> reuse existing GTK result window
              +--> SelectionClassifier
              +--> AnalysisService
              |       +--> Translation / LLM provider
              |       +--> Dictionary provider
              |       +--> Speech provider (on demand)
              +--> LearningRepository (SQLite)
```

The trigger is the separate `seltrans-trigger` binary built from the GUI-free `seltrans-core` crate (selection, classification, domain vocabulary, IPC transport). The resident `seltrans` binary is the only target that depends on GTK/libadwaita, so "the trigger never touches GTK" holds by construction rather than by convention, and CI asserts the trigger binary links no GUI libraries. A per-user Unix-domain socket under `XDG_RUNTIME_DIR`, guarded by an advisory `flock`, provides the simplest robust single-instance mechanism; no general background service framework is necessary.

For the first release, prefer **manual-resident** mode: if no resident instance exists, show a clear not-running notification or nonzero exit and do not silently cold-start a new GUI per shortcut. Optional one-time auto-start may be introduced if it reliably starts only one resident instance. Closing the result view should hide/reuse it; quitting the application is an explicit separate action. Multiple successive requests must not create additional windows. This is a product requirement, not a late optimization.

## 3. Core boundaries

### 3.1 SelectionProvider

Responsibility:

- obtain the text explicitly requested by the user.

Initial implementation:

- execute `wl-paste --primary --no-newline`.

Future implementations may use native Wayland protocols or platform-specific APIs.

This interface must not imply clipboard monitoring.

### 3.2 SelectionClassifier

Responsibility:

- classify the selected text into an interaction mode.

Initial modes:

- `Word`;
- `Passage`.

The classifier should be deterministic where obvious and may use lexical heuristics.

Do not spend an LLM request merely to decide whether a clearly single lexical token is a word.

### 3.3 AnalysisService

Orchestrates translation and learning analysis.

For passage mode it returns:

- translation;
- selected vocabulary;
- expressions/language notes.

For word mode it returns:

- lexical facts;
- contextual meaning;
- examples;
- pronunciation metadata where available.

The service should merge provider results without erasing provenance or confidence.

### 3.4 TranslationProvider

Responsibilities:

- translate passages;
- explain contextual meaning;
- identify difficult/contextually important vocabulary candidates;
- identify expressions, idioms, slang, collocations, and sentence patterns.

Initial provider target:

- OpenAI-compatible endpoint.

The application should request structured output and validate it before use.

### 3.5 DictionaryProvider

Responsibilities:

- canonical headword / lemma when supported;
- IPA;
- parts of speech;
- concise lexical definitions;
- pronunciation-audio metadata where supported.

Dictionary data is distinct from LLM analysis.

The application should not present LLM-generated IPA as authoritative dictionary data.

### 3.6 SpeechProvider

Responsibilities:

- play or obtain pronunciation audio for a word or sentence.

Potential backends:

- dictionary audio;
- system TTS;
- Piper;
- remote TTS.

Speech failure is non-fatal to translation.

### 3.7 LearningRepository

Responsibilities:

- persist learning items;
- query/search items;
- deduplicate or associate repeated observations;
- persist review history later.

Initial backend:

- SQLite.

## 4. Domain model

The core model should use `LearningItem`, not `WordEntry`.

Conceptual Rust shape:

```rust
enum LearningItemKind {
    Word,
    PhrasalVerb,
    Idiom,
    Collocation,
    Expression,
    SentencePattern,
}

struct LearningItem {
    id: LearningItemId,
    kind: LearningItemKind,
    canonical: String,
    surface: String,
    meaning: String,
    contextual_meaning: Option<String>,
    source_text: Option<String>,
    source_translation: Option<String>,
    example: Option<String>,
    pronunciation: Vec<Pronunciation>,
}
```

This is a conceptual contract, not a frozen persistence schema.

### Pronunciation

```rust
struct Pronunciation {
    locale: String,          // e.g. en-US / en-GB
    ipa: Option<String>,
    audio: Option<AudioRef>,
}
```

The model must permit missing pronunciation facts instead of fabricating them.

## 5. Passage analysis model

A passage result should separate vocabulary from expressions.

```rust
struct PassageAnalysis {
    source: String,
    translation: String,
    vocabulary: Vec<VocabularyNote>,
    expressions: Vec<ExpressionNote>,
}
```

Vocabulary is intentionally selective.

Expressions may contain:

- phrasal verb;
- idiom;
- slang;
- collocation;
- contextual meaning;
- sentence pattern;
- technical usage.

Each note should carry enough source span/context to render it meaningfully and save it as a learning item.

## 6. Structured LLM output

Do not parse prose heuristically if the provider can return structured JSON.

The LLM response contract should be versioned.

Conceptual payload:

```json
{
  "schema_version": 1,
  "translation": "...",
  "vocabulary": [
    {
      "surface": "acknowledged",
      "lemma": "acknowledge",
      "contextual_meaning": "响应/确认",
      "reason": "context-specific technical meaning"
    }
  ],
  "expressions": [
    {
      "surface": "not so much A as B",
      "kind": "sentence_pattern",
      "meaning": "与其说是 A，不如说是 B",
      "explanation": "..."
    }
  ]
}
```

The application validates size, required fields, and enum values.

Do not let provider output directly become SQL rows or GTK widgets.

## 7. Data provenance

Some fields are facts; some are model interpretations.

The architecture should preserve that distinction.

Examples:

- IPA -> dictionary source;
- pronunciation audio -> dictionary/TTS provider;
- translation -> LLM provider;
- contextual meaning -> LLM analysis;
- review state -> local user data.

This becomes important when providers disagree or one provider is unavailable.

## 8. Local persistence

Use XDG locations.

Expected data directory:

```text
$XDG_DATA_HOME/seltrans/
```

Fallback:

```text
~/.local/share/seltrans/
```

Initial database:

```text
vocabulary.db
```

Expected high-level tables later:

- learning_items;
- learning_item_contexts;
- pronunciations;
- review_state;
- review_events.

Do not freeze the exact schema before the first vertical slice proves the UI/domain needs.

## 9. Configuration and secrets

Expected configuration:

```text
$XDG_CONFIG_HOME/seltrans/config.toml
```

Fallback:

```text
~/.config/seltrans/config.toml
```

Prefer API keys from environment variables or an OS secret store.

Example config may reference:

```toml
provider = "openai-compatible"

[providers.openai-compatible]
base_url = "https://example.invalid/v1"
model = "model-name"
api_key_env = "SELTRANS_API_KEY"
```

Never commit real keys.

## 10. UI architecture

GTK4/libadwaita is the initial UI stack.

The main result view should be data-driven from domain response objects.

### Word view

Sections:

- headword;
- IPA/pronunciation;
- parts of speech and meanings;
- current-context meaning;
- example/source context;
- save action.

### Passage view

Sections:

- original;
- translation;
- vocabulary;
- expressions/language notes.

Detailed lexical information should be expandable rather than making the primary passage view excessively dense.

## 11. Concurrency

Network calls must not block the GTK main thread.

Likely implementation options:

- Tokio runtime integrated with application tasks;
- GLib async bridging where appropriate.

The exact runtime arrangement should be chosen during implementation, not assumed by the product spec.

Cancellation matters: if the user closes a result or triggers a new analysis, stale network results should not overwrite newer UI state.

## 12. Latency strategy

The resident-instance model is mandatory from the first interactive vertical slice. Measure the *warm invocation path* separately from the one-time application startup and provider/network latency.

On each warm `Alt+Q`:

- capture the current selection **before** the result window receives focus;
- send it through local IPC to the existing process;
- reuse GTK state and the same result window;
- show the new source/visible loading state promptly;
- perform network analysis asynchronously using reusable clients;
- never let an older response overwrite a newer invocation.

A very small short-lived trigger process is acceptable. Re-launching GTK/application initialization for each shortcut is not. If startup/on-demand activation feels slow, require a manual once-per-session start rather than compromising this flow.

A resident HTTP client, connection reuse and dictionary caches are implementation concerns to be measured and tuned; the single-instance invariant is not optional.

## 13. Failure isolation

Provider failures should degrade independently where possible.

Examples:

- translation succeeds, dictionary fails -> show translation without IPA;
- dictionary succeeds, TTS fails -> show IPA and meanings, disable playback;
- provider response malformed -> show actionable provider error;
- selection empty -> fail before network work.

## 14. Security / privacy

- no implicit clipboard monitoring;
- no telemetry by default;
- no plaintext API keys in repository examples;
- send only user-selected text to remote analysis providers;
- local learning database remains local by default.

## 15. Proposed source layout

This is directional, not frozen. The current workspace already separates the
GUI-free core from the application:

```text
crates/
  seltrans-core/           # no GUI dependencies; builds seltrans-trigger
    src/
      lib.rs
      classifier.rs
      domain.rs
      selection.rs
      ipc.rs
      bin/seltrans-trigger.rs
  seltrans/                # GTK4/libadwaita resident application
    src/
      main.rs
      ui.rs
    # later: analysis/, providers/, storage/, config/ (also GUI-free where possible)
```

Prefer narrow provider traits and domain-level results over provider-specific types leaking across the application. New non-UI logic belongs in `seltrans-core` (or a sibling core crate) so the warm path and future CLI tooling stay GUI-free.

## 16. Architectural guardrails

Do not:

- couple GTK widgets directly to HTTP response JSON;
- couple SQLite rows directly to provider payloads;
- make LLM output authoritative phonetic data;
- introduce a second full GUI process on every shortcut;
- allow the selection to be replaced by changing focus before capture;
- treat a one-shot UI as the production interaction model;
- monitor the clipboard continuously;
- implement every provider before the first end-to-end flow works;
- turn v0.1 into a general-purpose language-learning platform.

Build vertical slices.
