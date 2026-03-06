//! Provides macros for generating identifier functionality.

macro_rules! impl_serialization_for_identifier {
    ($ty:ty) => {
        impl Serialize for $ty {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                self.inner().serialize(serializer)
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value_str: &str = Deserialize::deserialize(deserializer)?;
                Self::new_checked(value_str).map_err(serde::de::Error::custom)
            }
        }
    };
}

macro_rules! impl_from_str_for_identifier {
    ($ty:ty) => {
        impl From<&str> for $ty {
            fn from(value: &str) -> Self {
                Self::new(value)
            }
        }

        impl From<String> for $ty {
            fn from(value: String) -> Self {
                Self::new(value)
            }
        }
    };
}

macro_rules! impl_as_ref_for_identifier {
    ($ty:ty) => {
        impl AsRef<str> for $ty {
            fn as_ref(&self) -> &str {
                self.as_str()
            }
        }
    };
}
