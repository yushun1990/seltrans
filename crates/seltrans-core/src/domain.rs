/// The kinds of language knowledge that may be saved for later review.
///
/// This deliberately goes beyond a traditional word-only vocabulary list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LearningItemKind {
    Word,
    PhrasalVerb,
    Idiom,
    Collocation,
    Expression,
    SentencePattern,
}

/// High-level interaction mode derived from the current selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    Word,
    Passage,
}

/// A minimal bootstrap representation of a saved learning item.
///
/// The persistence schema is intentionally not frozen yet. This type exists to
/// encode the product invariant that saved knowledge retains its reading
/// context and is not limited to single words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LearningItem {
    pub kind: LearningItemKind,
    pub canonical: String,
    pub surface: String,
    pub meaning: String,
    pub contextual_meaning: Option<String>,
    pub source_text: Option<String>,
    pub source_translation: Option<String>,
}
