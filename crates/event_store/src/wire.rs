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

//! Wire-format adapters for the on-disk event store envelope.
//!
//! `UnixNanos` deserializes through `deserialize_any`, which non-self-describing formats
//! such as bincode reject. The on-disk envelope therefore serializes timestamp fields as
//! raw `u64` and reconstructs the strong type on read.

/// Serializes [`nautilus_core::UnixNanos`] as a raw `u64` so bincode can round-trip it.
pub(crate) mod nanos_as_u64 {
    use nautilus_core::UnixNanos;
    use serde::{Deserialize, Deserializer, Serializer};

    /// Writes the inner `u64` directly.
    ///
    /// # Errors
    ///
    /// Propagates any error from the underlying serializer.
    #[allow(clippy::trivially_copy_pass_by_ref)] // serde contract requires &T
    pub(crate) fn serialize<S: Serializer>(
        value: &UnixNanos,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_u64(value.as_u64())
    }

    /// Reads a `u64` and constructs a [`UnixNanos`].
    ///
    /// # Errors
    ///
    /// Propagates any error from the underlying deserializer.
    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<UnixNanos, D::Error> {
        let raw = u64::deserialize(deserializer)?;
        Ok(UnixNanos::from(raw))
    }
}

/// Serializes `Option<UnixNanos>` as `Option<u64>`.
pub(crate) mod opt_nanos_as_u64 {
    use nautilus_core::UnixNanos;
    use serde::{Deserialize, Deserializer, Serializer};

    /// Writes the value as `Option<u64>`.
    ///
    /// # Errors
    ///
    /// Propagates any error from the underlying serializer.
    #[allow(clippy::ref_option)] // serde contract requires &Option<T>
    pub(crate) fn serialize<S: Serializer>(
        value: &Option<UnixNanos>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        match value {
            Some(v) => serializer.serialize_some(&v.as_u64()),
            None => serializer.serialize_none(),
        }
    }

    /// Reads an `Option<u64>` and constructs the optional [`UnixNanos`].
    ///
    /// # Errors
    ///
    /// Propagates any error from the underlying deserializer.
    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<UnixNanos>, D::Error> {
        let raw: Option<u64> = Option::deserialize(deserializer)?;
        Ok(raw.map(UnixNanos::from))
    }
}
