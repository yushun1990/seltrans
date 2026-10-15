# AGENTS.md

## Project intent

seltrans is not a generic translator.

It is a context-aware English reading and learning assistant whose core loop is:

```text
Reading -> Understand -> Discover -> Capture -> Remember
```

Before making product or architecture changes, read:

1. `README.md`
2. `docs/PRODUCT.md`
3. `docs/ARCHITECTURE.md`
4. `docs/ROADMAP.md`
5. `docs/adr/0001-resident-shortcut-model.md`
6. `docs/UI_REFERENCES.md`

## Product invariants

1. Translation occurs only after explicit user invocation.
2. Do not introduce automatic clipboard monitoring.
3. Single-word and passage selections are different UX modes.
4. Passage vocabulary is selective.
5. Meaningful idioms, slang, phrasal verbs, collocations, contextual meanings, and special sentence patterns should be surfaced rather than hidden by aggressive filtering.
6. Context is preserved when learning items are saved.
7. A learning item is not necessarily a word.
8. LLM output is not an authoritative source of IPA/pronunciation facts.
9. Learning data is local-first.
10. The first platform is Fedora/GNOME/Wayland.
11. The GUI is resident after first launch: Alt+Q submits new text to the running instance through IPC, reusing a single result window; no repeated GTK cold starts.
12. Default first-version startup may be manual. Selection must be captured before the popup changes focus.

## Engineering principles

- Build vertical slices.
- Keep provider-specific types behind provider boundaries.
- Keep domain models independent of GTK widgets and raw HTTP JSON.
- Keep persistence models from becoming the public domain API.
- Prefer structured provider responses and validate them.
- Make partial provider failure degradable where possible.
- Follow [ADR 0001](docs/adr/0001-resident-shortcut-model.md) for the mandatory resident model. Measure and optimize warm invocation latency without introducing unrelated daemon infrastructure.
- Do not add abstractions that have no current consumer merely for theoretical portability.

## Initial implementation direction

- Rust
- GTK4 / libadwaita
- SQLite
- OpenAI-compatible analysis provider
- pluggable dictionary and speech providers
- primary selection initially via `wl-paste --primary --no-newline`

## Change discipline

When a change affects a product invariant or architecture boundary:

- explain the reason in the PR;
- update the relevant document;
- call out compatibility/migration impact;
- add tests for observable behavior where practical.

Do not silently redefine the product through implementation convenience.
