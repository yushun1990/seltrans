//! Deterministic, local classification. No network request is needed to
//! distinguish an ordinary selected English word from a passage.

use crate::domain::SelectionMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClassificationError {
    EmptySelection,
}

impl std::fmt::Display for ClassificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptySelection => write!(f, "the current selection is empty"),
        }
    }
}

impl std::error::Error for ClassificationError {}

pub fn classify_selection(selection: &str) -> Result<SelectionMode, ClassificationError> {
    let text = selection.trim();
    if text.is_empty() {
        return Err(ClassificationError::EmptySelection);
    }

    // Only ordinary surrounding punctuation can be stripped; a technical
    // token such as C++ must not silently become a single English word.
    let candidate = text.trim_matches(|c: char| {
        matches!(
            c,
            '"' | '“' | '”' | '(' | ')' | '[' | ']' | '{' | '}' | '.' | ',' | '!' | '?' | ';' | ':'
        )
    });
    let letters: Vec<char> = candidate.chars().collect();
    let is_word = !letters.is_empty()
        && letters.iter().enumerate().all(|(i, c)| {
            c.is_alphabetic()
                || (matches!(c, '\'' | '’' | '-')
                    && i > 0
                    && i + 1 < letters.len()
                    && letters[i - 1].is_alphabetic()
                    && letters[i + 1].is_alphabetic())
        });

    Ok(if is_word {
        SelectionMode::Word
    } else {
        SelectionMode::Passage
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_words_with_context_punctuation() {
        for text in [
            "acknowledged",
            " running ",
            "don't",
            "well-known",
            "\"hello,\"",
        ] {
            assert_eq!(classify_selection(text), Ok(SelectionMode::Word), "{text}");
        }
    }

    #[test]
    fn classifies_passages_and_non_words() {
        for text in [
            "A short sentence.",
            "not so much A as B",
            "HTTP/2",
            "C++",
            "...",
        ] {
            assert_eq!(
                classify_selection(text),
                Ok(SelectionMode::Passage),
                "{text}"
            );
        }
    }

    #[test]
    fn rejects_whitespace_only_selection() {
        assert_eq!(
            classify_selection("  \n\t"),
            Err(ClassificationError::EmptySelection)
        );
    }
}
