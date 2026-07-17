//! Represents a canonical participant identifier.

use std::{
    fmt::{Debug, Display},
    hash::Hash,
};

use nautilus_core::correctness::{
    CorrectnessResult, CorrectnessResultExt, FAILED, check_valid_string_utf8,
};
use ustr::Ustr;

/// A canonical identifier for an observed participant.
#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct ParticipantId(Ustr);

impl ParticipantId {
    /// Creates a new [`ParticipantId`] with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if `value` is empty, only whitespace, or contains invalid
    /// control characters.
    pub fn new_checked<T: AsRef<str>>(value: T) -> CorrectnessResult<Self> {
        let value = value.as_ref();
        check_valid_string_utf8(value, stringify!(value))?;
        Ok(Self(Ustr::from(value)))
    }

    /// Creates a new [`ParticipantId`].
    ///
    /// # Panics
    ///
    /// Panics if `value` is invalid. See [`ParticipantId::new_checked`].
    #[must_use]
    pub fn new<T: AsRef<str>>(value: T) -> Self {
        Self::new_checked(value).expect_display(FAILED)
    }

    /// Sets the inner identifier value.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    pub(crate) fn set_inner(&mut self, value: &str) {
        self.0 = Ustr::from(value);
    }

    /// Returns the inner identifier value.
    #[must_use]
    pub fn inner(&self) -> Ustr {
        self.0
    }

    /// Returns the identifier as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl Debug for ParticipantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "\"{}\"", self.0)
    }
}

impl Display for ParticipantId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PARTICIPANT_ID: &str = "0x0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn test_participant_id_roundtrip() {
        let participant_id = ParticipantId::new(PARTICIPANT_ID);
        let json = serde_json::to_string(&participant_id).unwrap();
        let decoded: ParticipantId = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, participant_id);
        assert_eq!(decoded.as_str(), PARTICIPANT_ID);
    }

    #[test]
    fn test_participant_id_rejects_empty_value() {
        assert!(ParticipantId::new_checked("").is_err());
        assert!(serde_json::from_str::<ParticipantId>(r#"""#).is_err());
    }
}
