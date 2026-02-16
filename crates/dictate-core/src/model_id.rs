//! Validated model identifier for LLM post-processing.
//!
//! [`ModelId`] ensures that model identifiers are non-empty, within length
//! limits, and contain only safe ASCII characters — catching typos and
//! accidental misuse at parse time rather than at API call time.

use std::fmt;
use std::str::FromStr;

use crate::error::ModelIdError;

/// Maximum allowed length for a model identifier.
const MAX_LEN: usize = 128;

/// Well-known model: Llama 3.1 8B Instant (fast, lightweight post-processing).
pub const LLAMA_3_1_8B: &str = "llama-3.1-8b-instant";

/// Well-known model: Llama 3.3 70B Versatile (higher-quality post-processing).
pub const LLAMA_3_3_70B: &str = "llama-3.3-70b-versatile";

/// A validated LLM model identifier.
///
/// Guarantees:
/// - Non-empty
/// - At most 128 characters
/// - Contains only `[a-zA-Z0-9._/-]`
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModelId(String);

impl ModelId {
    /// Create a new `ModelId` after validation.
    ///
    /// # Errors
    ///
    /// Returns [`ModelIdError`] if the string is empty, too long, or contains
    /// characters outside `[a-zA-Z0-9._/-]`.
    pub fn new(id: impl Into<String>) -> Result<Self, ModelIdError> {
        let id = id.into();

        if id.is_empty() {
            return Err(ModelIdError::Empty);
        }
        if id.len() > MAX_LEN {
            return Err(ModelIdError::TooLong { len: id.len() });
        }
        if !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'/' || b == b'-')
        {
            return Err(ModelIdError::InvalidCharacters);
        }

        Ok(Self(id))
    }

    /// Borrow the inner model ID string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for ModelId {
    type Err = ModelIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl fmt::Display for ModelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_model_ids() {
        assert!(ModelId::new("llama-3.1-8b-instant").is_ok());
        assert!(ModelId::new("llama-3.3-70b-versatile").is_ok());
        assert!(ModelId::new("gpt-4o").is_ok());
        assert!(ModelId::new("org/model-name").is_ok());
        assert!(ModelId::new("model_v2").is_ok());
    }

    #[test]
    fn empty_rejected() {
        assert_eq!(ModelId::new(""), Err(ModelIdError::Empty));
    }

    #[test]
    fn too_long_rejected() {
        let long = "a".repeat(129);
        assert_eq!(ModelId::new(&long), Err(ModelIdError::TooLong { len: 129 }));

        // Exactly 128 should be accepted
        let at_limit = "a".repeat(128);
        assert!(ModelId::new(&at_limit).is_ok());
    }

    #[test]
    fn invalid_chars_rejected() {
        assert_eq!(
            ModelId::new("model name"),
            Err(ModelIdError::InvalidCharacters)
        );
        assert_eq!(
            ModelId::new("model@v1"),
            Err(ModelIdError::InvalidCharacters)
        );
        assert_eq!(
            ModelId::new("model\n"),
            Err(ModelIdError::InvalidCharacters)
        );
        assert_eq!(ModelId::new("模型"), Err(ModelIdError::InvalidCharacters));
    }

    #[test]
    fn from_str_works() {
        let id: ModelId = "llama-3.1-8b-instant".parse().unwrap();
        assert_eq!(id.as_str(), "llama-3.1-8b-instant");
    }

    #[test]
    fn from_str_rejects_invalid() {
        assert!("".parse::<ModelId>().is_err());
        assert!("has spaces".parse::<ModelId>().is_err());
    }

    #[test]
    fn display_roundtrips() {
        let id = ModelId::new("llama-3.1-8b-instant").unwrap();
        assert_eq!(id.to_string(), "llama-3.1-8b-instant");
    }

    #[test]
    fn well_known_constants_are_valid() {
        assert!(ModelId::new(LLAMA_3_1_8B).is_ok());
        assert!(ModelId::new(LLAMA_3_3_70B).is_ok());
    }

    #[test]
    fn equality_and_hash() {
        use std::collections::HashSet;

        let a = ModelId::new("llama-3.1-8b-instant").unwrap();
        let b = ModelId::new("llama-3.1-8b-instant").unwrap();
        let c = ModelId::new("gpt-4o").unwrap();

        assert_eq!(a, b);
        assert_ne!(a, c);

        let mut set = HashSet::new();
        set.insert(a);
        assert!(set.contains(&b));
        assert!(!set.contains(&c));
    }
}
