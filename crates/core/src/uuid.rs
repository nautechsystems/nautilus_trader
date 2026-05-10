// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! A `UUID4` Universally Unique Identifier (UUID) version 4 (RFC 4122).

use std::{
    ffi::CStr,
    fmt::{Debug, Display},
    hash::Hash,
    io::{Cursor, Write},
    str::FromStr,
};

#[cfg(all(feature = "simulation", madsim))]
use madsim::rand::RngCore as MadsimRngCore;
use rand::Rng;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

/// The maximum length of ASCII characters for a `UUID4` string value (includes null terminator).
pub(crate) const UUID4_LEN: usize = 37;

/// Represents a Universally Unique Identifier (UUID)
/// version 4 based on a 128-bit label as specified in RFC 4122.
#[repr(C)]
#[derive(Copy, Clone, Hash, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.core", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.core")
)]
pub struct UUID4 {
    /// The UUID v4 value as a fixed-length C string byte array (includes null terminator).
    pub(crate) value: [u8; 37], // cbindgen issue using the constant in the array
}

impl UUID4 {
    /// Creates a new [`UUID4`] instance.
    ///
    /// The UUID value is stored as a fixed-length C string byte array.
    #[must_use]
    pub fn new() -> Self {
        let mut bytes = [0u8; 16];
        #[cfg(all(feature = "simulation", madsim))]
        {
            // Deterministic RNG when running inside a madsim runtime; otherwise
            // (e.g. plain `#[rstest]` tests under `cfg(madsim)`) fall back to
            // the host RNG. Production paths under simulation always run inside
            // a runtime, so they continue to consume seeded bytes.
            if madsim::runtime::Handle::try_current().is_ok() {
                MadsimRngCore::fill_bytes(&mut madsim::rand::thread_rng(), &mut bytes);
            } else {
                rand::rng().fill_bytes(&mut bytes); // dst-ok: tests outside a madsim runtime
            }
        }
        #[cfg(not(all(feature = "simulation", madsim)))]
        rand::rng().fill_bytes(&mut bytes);

        bytes[6] = (bytes[6] & 0x0F) | 0x40; // Set the version to 4
        bytes[8] = (bytes[8] & 0x3F) | 0x80; // Set the variant to RFC 4122

        let mut value = [0u8; UUID4_LEN];
        let mut cursor = Cursor::new(&mut value[..36]);

        write!(
            cursor,
            "{:08x}-{:04x}-{:04x}-{:04x}-{:012x}",
            u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            u16::from_be_bytes([bytes[4], bytes[5]]),
            u16::from_be_bytes([bytes[6], bytes[7]]),
            u16::from_be_bytes([bytes[8], bytes[9]]),
            u64::from_be_bytes([
                bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15], 0, 0
            ]) >> 16
        )
        .expect("Error writing UUID string to buffer");

        value[36] = 0; // Add the null terminator

        debug_assert!(
            value[14] == b'4',
            "Invariant: UUID version digit must be '4' (was {})",
            value[14] as char
        );
        debug_assert!(
            matches!(value[19], b'8' | b'9' | b'a' | b'b'),
            "Invariant: UUID variant byte must be RFC 4122 (was {})",
            value[19] as char
        );
        debug_assert!(
            value[36] == 0,
            "Invariant: UUID null terminator must be at index 36"
        );

        Self { value }
    }

    /// Creates a [`UUID4`] from raw 16-byte representation.
    ///
    /// Sets the version-4 nibble and the RFC 4122 variant bits before constructing,
    /// so any 16 bytes produce a valid v4 UUID.
    #[must_use]
    pub fn from_bytes(mut bytes: [u8; 16]) -> Self {
        bytes[6] = (bytes[6] & 0x0F) | 0x40;
        bytes[8] = (bytes[8] & 0x3F) | 0x80;
        Self::from_validated_uuid(&Uuid::from_bytes(bytes))
    }

    /// Converts the [`UUID4`] to a C string reference.
    ///
    /// # Panics
    ///
    /// Panics if the internal byte array is not a valid C string (does not end with a null terminator).
    #[must_use]
    pub fn to_cstr(&self) -> &CStr {
        // We always store valid C strings
        CStr::from_bytes_with_nul(&self.value)
            .expect("UUID byte representation should be a valid C string")
    }

    /// Returns the UUID as a string slice.
    ///
    /// # Panics
    ///
    /// Never panics in practice: the stored byte representation is constructed
    /// from valid ASCII UUID strings by [`UUID4::new`] or deserialization paths.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // We always store valid ASCII UUID strings
        self.to_cstr().to_str().expect("UUID should be valid UTF-8")
    }

    /// Returns the raw UUID bytes (16 bytes).
    ///
    /// This method is optimized for serialization where the UUID bytes
    /// are needed directly without string conversion overhead.
    ///
    /// # Panics
    ///
    /// Never panics in practice: the stored byte representation is a valid
    /// UTF-8 UUID v4 string produced by [`UUID4::new`] or deserialization paths.
    #[must_use]
    pub fn as_bytes(&self) -> [u8; 16] {
        // Parse the string representation to extract the raw bytes
        // This is done once at read time to avoid repeated parsing
        let uuid_str = self.to_cstr().to_str().expect("Valid UTF-8");
        let uuid = Uuid::parse_str(uuid_str).expect("Valid UUID4");
        *uuid.as_bytes()
    }

    fn validate_v4(uuid: &Uuid) {
        // Validate this is a v4 UUID
        assert_eq!(
            uuid.get_version(),
            Some(uuid::Version::Random),
            "UUID is not version 4"
        );

        // Validate RFC4122 variant
        assert_eq!(
            uuid.get_variant(),
            uuid::Variant::RFC4122,
            "UUID is not RFC 4122 variant"
        );
    }

    fn try_validate_v4(uuid: &Uuid) -> Result<(), String> {
        if uuid.get_version() != Some(uuid::Version::Random) {
            return Err("UUID is not version 4".to_string());
        }

        if uuid.get_variant() != uuid::Variant::RFC4122 {
            return Err("UUID is not RFC 4122 variant".to_string());
        }
        Ok(())
    }

    fn from_validated_uuid(uuid: &Uuid) -> Self {
        let mut value = [0; UUID4_LEN];
        let uuid_str = uuid.to_string();
        value[..uuid_str.len()].copy_from_slice(uuid_str.as_bytes());
        value[uuid_str.len()] = 0; // Add null terminator
        Self { value }
    }
}

impl FromStr for UUID4 {
    type Err = String;

    /// Attempts to create a [`UUID4`] from a string representation.
    ///
    /// The string should be a valid UUID in the standard format (e.g., "2d89666b-1a1e-4a75-b193-4eb3b454c757").
    ///
    /// # Errors
    ///
    /// Returns an error if the `value` is not a valid UUID version 4 RFC 4122.
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let uuid = Uuid::try_parse(value).map_err(|e| e.to_string())?;
        Self::try_validate_v4(&uuid)?;
        Ok(Self::from_validated_uuid(&uuid))
    }
}

impl From<&str> for UUID4 {
    fn from(value: &str) -> Self {
        Self::from_str(value).expect("Invalid UUID4 string")
    }
}

impl From<String> for UUID4 {
    fn from(value: String) -> Self {
        Self::from_str(&value).expect("Invalid UUID4 string")
    }
}

impl From<uuid::Uuid> for UUID4 {
    /// Creates a [`UUID4`] from a [`uuid::Uuid`].
    ///
    /// # Panics
    ///
    /// Panics if the `value` is not a valid UUID version 4 RFC 4122.
    fn from(value: uuid::Uuid) -> Self {
        Self::validate_v4(&value);
        Self::from_validated_uuid(&value)
    }
}

impl From<UUID4> for uuid::Uuid {
    /// Creates a [`uuid::Uuid`] from a [`UUID4`].
    fn from(value: UUID4) -> Self {
        Self::from_bytes(value.as_bytes())
    }
}

impl Default for UUID4 {
    /// Creates a new default [`UUID4`] instance.
    ///
    /// The default UUID4 is simply a newly generated UUID version 4.
    fn default() -> Self {
        Self::new()
    }
}

impl Debug for UUID4 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", stringify!(UUID4), self)
    }
}

impl Display for UUID4 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_cstr().to_string_lossy())
    }
}

impl Serialize for UUID4 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.to_string().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for UUID4 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let uuid4_str: &str = Deserialize::deserialize(deserializer)?;
        uuid4_str.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
    };

    use proptest::prelude::*;
    use rstest::*;
    use uuid;

    use super::*;

    #[rstest]
    fn test_new() {
        let uuid = UUID4::new();
        let uuid_string = uuid.to_string();
        let uuid_parsed = Uuid::parse_str(&uuid_string).unwrap();
        assert_eq!(uuid_parsed.get_version().unwrap(), uuid::Version::Random);
        assert_eq!(uuid_parsed.to_string().len(), 36);

        // Version 4 requires bits: 0b0100xxxx
        assert_eq!(&uuid_string[14..15], "4");
        // RFC4122 variant requires bits: 0b10xxxxxx
        let variant_char = &uuid_string[19..20];
        assert!(matches!(variant_char, "8" | "9" | "a" | "b" | "A" | "B"));
    }

    #[rstest]
    fn test_uuid_format() {
        let uuid = UUID4::new();
        let bytes = uuid.value;

        // Check null termination
        assert_eq!(bytes[36], 0);

        // Verify dash positions
        assert_eq!(bytes[8] as char, '-');
        assert_eq!(bytes[13] as char, '-');
        assert_eq!(bytes[18] as char, '-');
        assert_eq!(bytes[23] as char, '-');

        let s = uuid.to_string();
        assert_eq!(s.chars().nth(14).unwrap(), '4');
    }

    #[rstest]
    #[should_panic(expected = "UUID is not version 4")]
    fn test_from_str_with_non_version_4_uuid_panics() {
        let uuid_string = "6ba7b810-9dad-11d1-80b4-00c04fd430c8"; // v1 UUID
        let _ = UUID4::from(uuid_string);
    }

    #[rstest]
    fn test_case_insensitive_parsing() {
        let upper = "2D89666B-1A1E-4A75-B193-4EB3B454C757";
        let lower = "2d89666b-1a1e-4a75-b193-4eb3b454c757";
        let uuid_upper = UUID4::from(upper);
        let uuid_lower = UUID4::from(lower);

        assert_eq!(uuid_upper, uuid_lower);
        assert_eq!(uuid_upper.to_string(), lower);
    }

    #[rstest]
    #[case("6ba7b810-9dad-11d1-80b4-00c04fd430c8")] // v1 (time-based)
    #[case("000001f5-8fa9-21d1-9df3-00e098032b8c")] // v2 (DCE Security)
    #[case("3d813cbb-47fb-32ba-91df-831e1593ac29")] // v3 (MD5 hash)
    #[case("fb4f37c1-4ba3-5173-9812-2b90e76a06f7")] // v5 (SHA-1 hash)
    #[should_panic(expected = "UUID is not version 4")]
    fn test_invalid_version(#[case] uuid_string: &str) {
        let _ = UUID4::from(uuid_string);
    }

    #[rstest]
    #[should_panic(expected = "UUID is not RFC 4122 variant")]
    fn test_non_rfc4122_variant() {
        // Valid v4 but wrong variant
        let uuid = "550e8400-e29b-41d4-0000-446655440000";
        let _ = UUID4::from(uuid);
    }

    #[rstest]
    #[case("")] // Empty string
    #[case("not-a-uuid-at-all")] // Invalid format
    #[case("6ba7b810-9dad-11d1-80b4")] // Too short
    #[case("6ba7b810-9dad-11d1-80b4-00c04fd430c8-extra")] // Too long
    #[case("6ba7b810-9dad-11d1-80b4=00c04fd430c8")] // Wrong separator
    #[case("6ba7b81019dad111d180b400c04fd430c8")] // No separators
    #[case("6ba7b810-9dad-11d1-80b4-00c04fd430")] // Truncated
    #[case("6ba7b810-9dad-11d1-80b4-00c04fd430cg")] // Invalid hex character
    fn test_invalid_uuid_cases(#[case] invalid_uuid: &str) {
        assert!(UUID4::from_str(invalid_uuid).is_err());
    }

    #[rstest]
    fn test_default() {
        let uuid: UUID4 = UUID4::default();
        let uuid_string = uuid.to_string();
        let uuid_parsed = Uuid::parse_str(&uuid_string).unwrap();
        assert_eq!(uuid_parsed.get_version().unwrap(), uuid::Version::Random);
    }

    #[rstest]
    fn test_from_str() {
        let uuid_string = "2d89666b-1a1e-4a75-b193-4eb3b454c757";
        let uuid = UUID4::from(uuid_string);
        let result_string = uuid.to_string();
        let result_parsed = Uuid::parse_str(&result_string).unwrap();
        let expected_parsed = Uuid::parse_str(uuid_string).unwrap();
        assert_eq!(result_parsed, expected_parsed);
    }

    #[rstest]
    fn test_from_uuid() {
        let original = uuid::Uuid::new_v4();
        let uuid4 = UUID4::from(original);
        assert_eq!(uuid4.to_string(), original.to_string());
    }

    #[rstest]
    fn test_equality() {
        let uuid1 = UUID4::from("2d89666b-1a1e-4a75-b193-4eb3b454c757");
        let uuid2 = UUID4::from("46922ecb-4324-4e40-a56c-841e0d774cef");
        assert_eq!(uuid1, uuid1);
        assert_ne!(uuid1, uuid2);
    }

    #[rstest]
    fn test_debug() {
        let uuid_string = "2d89666b-1a1e-4a75-b193-4eb3b454c757";
        let uuid = UUID4::from(uuid_string);
        assert_eq!(format!("{uuid:?}"), format!("UUID4({uuid_string})"));
    }

    #[rstest]
    fn test_display() {
        let uuid_string = "2d89666b-1a1e-4a75-b193-4eb3b454c757";
        let uuid = UUID4::from(uuid_string);
        assert_eq!(format!("{uuid}"), uuid_string);
    }

    #[rstest]
    fn test_to_cstr() {
        let uuid = UUID4::new();
        let cstr = uuid.to_cstr();

        assert_eq!(cstr.to_str().unwrap(), uuid.to_string());
        assert_eq!(cstr.to_bytes_with_nul()[36], 0);
    }

    #[rstest]
    fn test_as_str() {
        let uuid = UUID4::new();
        let s = uuid.as_str();

        assert_eq!(s, uuid.to_string());
        assert_eq!(s.len(), 36);
    }

    #[rstest]
    fn test_hash_consistency() {
        let uuid = UUID4::new();

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();

        uuid.hash(&mut hasher1);
        uuid.hash(&mut hasher2);

        assert_eq!(hasher1.finish(), hasher2.finish());
    }

    #[rstest]
    fn test_serialize_json() {
        let uuid_string = "2d89666b-1a1e-4a75-b193-4eb3b454c757";
        let uuid = UUID4::from(uuid_string);

        let serialized = serde_json::to_string(&uuid).unwrap();
        let expected_json = format!("\"{uuid_string}\"");
        assert_eq!(serialized, expected_json);
    }

    #[rstest]
    fn test_deserialize_json() {
        let uuid_string = "2d89666b-1a1e-4a75-b193-4eb3b454c757";
        let serialized = format!("\"{uuid_string}\"");

        let deserialized: UUID4 = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.to_string(), uuid_string);
    }

    #[rstest]
    fn test_serialize_deserialize_round_trip() {
        let uuid = UUID4::new();

        let serialized = serde_json::to_string(&uuid).unwrap();
        let deserialized: UUID4 = serde_json::from_str(&serialized).unwrap();

        assert_eq!(uuid, deserialized);
    }

    #[rstest]
    fn test_as_bytes() {
        let uuid_string = "2d89666b-1a1e-4a75-b193-4eb3b454c757";
        let uuid = UUID4::from(uuid_string);

        let bytes = uuid.as_bytes();
        assert_eq!(bytes.len(), 16);

        // Reconstruct UUID from bytes and verify it matches
        let reconstructed = Uuid::from_bytes(bytes);
        assert_eq!(reconstructed.to_string(), uuid_string);

        // Verify version 4
        assert_eq!(reconstructed.get_version().unwrap(), uuid::Version::Random);
    }

    #[rstest]
    fn test_as_bytes_round_trip() {
        let uuid1 = UUID4::new();
        let bytes = uuid1.as_bytes();
        let uuid2 = UUID4::from(Uuid::from_bytes(bytes));

        assert_eq!(uuid1, uuid2);
    }

    #[rstest]
    fn test_from_bytes_basic() {
        // A well-formed v4 / RFC 4122 input should be preserved verbatim.
        let bytes = [
            0x2d, 0x89, 0x66, 0x6b, 0x1a, 0x1e, 0x4a, 0x75, 0xb1, 0x93, 0x4e, 0xb3, 0xb4, 0x54,
            0xc7, 0x57,
        ];
        let uuid = UUID4::from_bytes(bytes);
        assert_eq!(uuid.to_string(), "2d89666b-1a1e-4a75-b193-4eb3b454c757");
        assert_eq!(uuid.as_bytes(), bytes);
    }

    #[rstest]
    fn test_from_bytes_normalizes_version() {
        // Input has version bits indicating v1 (0x10..): `from_bytes` must coerce to v4.
        let mut bytes = [0u8; 16];
        bytes[6] = 0x1a; // High nibble is version; 1 means v1
        bytes[8] = 0x80; // Already RFC 4122
        let uuid = UUID4::from_bytes(bytes);
        assert_eq!(&uuid.to_string()[14..15], "4");
        let parsed = Uuid::parse_str(uuid.as_str()).unwrap();
        assert_eq!(parsed.get_version(), Some(uuid::Version::Random));
    }

    #[rstest]
    fn test_from_bytes_normalizes_variant() {
        // Input has variant bits indicating non-RFC-4122 (0x00..): `from_bytes` must coerce.
        let mut bytes = [0u8; 16];
        bytes[6] = 0x40; // Already v4
        bytes[8] = 0x00; // Non-RFC-4122 variant
        let uuid = UUID4::from_bytes(bytes);
        let parsed = Uuid::parse_str(uuid.as_str()).unwrap();
        assert_eq!(parsed.get_variant(), uuid::Variant::RFC4122);
    }

    #[rstest]
    fn test_from_bytes_all_zero_is_valid_v4() {
        let uuid = UUID4::from_bytes([0u8; 16]);
        // After normalization, byte 6 is 0x40 and byte 8 is 0x80, so the canonical representation
        // is "00000000-0000-4000-8000-000000000000", still a valid v4 UUID.
        assert_eq!(uuid.to_string(), "00000000-0000-4000-8000-000000000000");
    }

    #[rstest]
    fn test_from_bytes_all_ones_is_valid_v4() {
        let uuid = UUID4::from_bytes([0xFFu8; 16]);
        let parsed = Uuid::parse_str(uuid.as_str()).unwrap();
        assert_eq!(parsed.get_version(), Some(uuid::Version::Random));
        assert_eq!(parsed.get_variant(), uuid::Variant::RFC4122);
    }

    #[rstest]
    fn test_from_bytes_round_trip() {
        // For inputs whose bits 6 and 8 are already v4/RFC-4122, `as_bytes` ∘ `from_bytes` is the
        // identity.
        let original = UUID4::new();
        let bytes = original.as_bytes();
        let reconstructed = UUID4::from_bytes(bytes);
        assert_eq!(original, reconstructed);
    }

    #[rstest]
    #[case("\"not-a-uuid\"")] // Invalid format
    #[case("\"6ba7b810-9dad-11d1-80b4-00c04fd430c8\"")] // v1 UUID (wrong version)
    #[case("\"\"")] // Empty string
    fn test_deserialize_invalid_uuid_returns_error(#[case] json: &str) {
        let result: Result<UUID4, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    fn uuid4_strategy() -> impl Strategy<Value = UUID4> {
        // Build from proptest-generated bytes for deterministic
        // reproduction and shrinking on failure
        any::<[u8; 16]>().prop_map(UUID4::from_bytes)
    }

    proptest! {
        #[rstest]
        fn prop_uuid4_string_roundtrip(uuid in uuid4_strategy()) {
            let s = uuid.to_string();
            let parsed = UUID4::from_str(&s);
            prop_assert!(parsed.is_ok(), "Failed to parse UUID string: {}", s);
            prop_assert_eq!(parsed.unwrap(), uuid, "String round-trip failed");
        }

        #[rstest]
        fn prop_uuid4_serde_roundtrip(uuid in uuid4_strategy()) {
            let serialized = serde_json::to_string(&uuid).unwrap();
            let deserialized: UUID4 = serde_json::from_str(&serialized).unwrap();
            prop_assert_eq!(deserialized, uuid, "Serde JSON round-trip failed");
        }

        #[rstest]
        fn prop_uuid4_rfc4122_compliance(uuid in uuid4_strategy()) {
            let s = uuid.to_string();
            let bytes = uuid.value;

            // Invariant: Total length is always 36 characters + null terminator
            prop_assert_eq!(s.len(), 36);
            prop_assert_eq!(bytes[36], 0, "Missing null terminator at index 36");

            // Invariant: Dash positions per RFC 4122
            prop_assert_eq!(bytes[8] as char, '-');
            prop_assert_eq!(bytes[13] as char, '-');
            prop_assert_eq!(bytes[18] as char, '-');
            prop_assert_eq!(bytes[23] as char, '-');

            // Invariant: Version digit must be '4' (index 14)
            prop_assert_eq!(&s[14..15], "4", "Version digit must be 4");

            // Invariant: Variant bits must be RFC 4122 (index 19)
            // Binary: 10xx -> Hex: 8, 9, a, b
            let variant_char = s.chars().nth(19).unwrap().to_ascii_lowercase();
            prop_assert!(
                matches!(variant_char, '8' | '9' | 'a' | 'b'),
                "Invalid variant character: {}", variant_char
            );
        }

        #[rstest]
        fn prop_uuid4_as_bytes_consistency(uuid in uuid4_strategy()) {
            let bytes = uuid.as_bytes();
            let reconstructed = uuid::Uuid::from_bytes(bytes);
            prop_assert_eq!(reconstructed.to_string(), uuid.to_string(), "Byte reconstruction mismatch");
        }

        #[rstest]
        fn prop_uuid4_equality_and_hashing(uuid1 in uuid4_strategy(), uuid2 in uuid4_strategy()) {
            // Identity
            prop_assert_eq!(uuid1, uuid1);

            // Equality implies hash equality
            if uuid1 == uuid2 {
                let mut h1 = DefaultHasher::new();
                let mut h2 = DefaultHasher::new();
                uuid1.hash(&mut h1);
                uuid2.hash(&mut h2);
                prop_assert_eq!(h1.finish(), h2.finish());
            }
        }

        #[rstest]
        fn prop_uuid4_from_str_never_panics(s: String) {
            // Fuzzing the parser with arbitrary strings
            let _ = UUID4::from_str(&s);
        }

        #[rstest]
        fn prop_from_bytes_always_yields_v4(bytes in any::<[u8; 16]>()) {
            // Any 16-byte input must produce a UUID that passes both v4 and RFC 4122 checks,
            // because `from_bytes` unconditionally normalizes the version and variant nibbles.
            let uuid = UUID4::from_bytes(bytes);
            let parsed = uuid::Uuid::parse_str(uuid.as_str()).unwrap();
            prop_assert_eq!(parsed.get_version(), Some(uuid::Version::Random));
            prop_assert_eq!(parsed.get_variant(), uuid::Variant::RFC4122);
        }

        #[rstest]
        fn prop_from_bytes_as_bytes_roundtrip(bytes in any::<[u8; 16]>()) {
            // `as_bytes` must reflect exactly the bits `from_bytes` produced: the input
            // bytes after version/variant normalization.
            let mut expected = bytes;
            expected[6] = (expected[6] & 0x0F) | 0x40;
            expected[8] = (expected[8] & 0x3F) | 0x80;
            let uuid = UUID4::from_bytes(bytes);
            prop_assert_eq!(uuid.as_bytes(), expected);
        }
    }
}
