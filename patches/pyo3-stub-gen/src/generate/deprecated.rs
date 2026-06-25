use crate::type_info::DeprecatedInfo;
use std::fmt;

impl fmt::Display for DeprecatedInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "@typing_extensions.deprecated(")?;
        match (&self.since, &self.note) {
            (Some(since), Some(note)) => {
                write!(f, "\"[Since {since}] {note}\"")?;
            }
            (Some(since), None) => {
                write!(f, "\"[Since {since}]\"")?;
            }
            (None, Some(note)) => {
                write!(f, "\"{note}\"")?;
            }
            (None, None) => {
                write!(f, "\"\"")?;
            }
        }

        write!(f, ")")
    }
}
