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

//! The bus capture allow-list.
//!
//! [`EncoderRegistry`] is the allow-list the [`crate::capture::BusCaptureAdapter`] consults
//! at every dispatch boundary. A message whose Rust type has no registered encoder is not
//! captured: the SPEC names a closed list of state-affecting topics, and silent capture of
//! out-of-allow-list types would produce entries the verifier process cannot decode.
//!
//! Registration binds three things:
//!
//! 1. The Rust type the encoder consumes (used as the [`std::any::TypeId`] lookup key).
//! 2. The canonical [`crate::PayloadType`] tag stamped on every entry the encoder produces.
//! 3. The encoder closure that produces the payload bytes plus sidecar [`crate::IndexKey`]s.

use std::{
    any::{Any, TypeId},
    collections::HashMap,
    fmt::Debug,
    sync::Arc,
};

use crate::{
    capture::encoder::{Encode, EncodeError, EncodedPayload, TypedEncoder},
    entry::PayloadType,
};

/// One allow-list entry: the canonical payload type tag and the encoder for the message.
#[derive(Clone)]
struct Registered {
    payload_type: PayloadType,
    encoder: Arc<dyn Encode>,
}

impl Debug for Registered {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Registered))
            .field("payload_type", &self.payload_type.as_str())
            .finish_non_exhaustive()
    }
}

/// Allow-list of capturable Rust message types and their encoders.
///
/// Registration is keyed by [`std::any::TypeId`]; the adapter dispatches by `TypeId` so
/// callers can capture a typed message without naming the encoder concretely. A type
/// registered twice replaces the prior entry: the SPEC's encoder rules require one
/// canonical mapping per Rust type, and the registry would otherwise hide the conflict.
#[derive(Clone, Debug, Default)]
pub struct EncoderRegistry {
    by_type: HashMap<TypeId, Registered>,
}

impl EncoderRegistry {
    /// Creates a new empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers `func` as the encoder for `T` and stamps every captured entry with
    /// `payload_type`.
    ///
    /// Replaces any encoder previously registered for `T`; capture flows hold the
    /// registry as `Arc<EncoderRegistry>` so registration must happen before the adapter
    /// is constructed.
    pub fn register<T, F>(&mut self, payload_type: PayloadType, func: F)
    where
        T: 'static,
        F: Fn(&T) -> Result<EncodedPayload, EncodeError> + Send + Sync + 'static,
    {
        let encoder: Arc<dyn Encode> = Arc::new(TypedEncoder::<T, F>::new(func));
        self.by_type.insert(
            TypeId::of::<T>(),
            Registered {
                payload_type,
                encoder,
            },
        );
    }

    /// Registers an already-built [`Encode`] implementer for `T`.
    ///
    /// Useful when the encoder owns state (e.g., a schema cache) the closure form cannot
    /// express ergonomically.
    pub fn register_encoder<T: 'static>(
        &mut self,
        payload_type: PayloadType,
        encoder: Arc<dyn Encode>,
    ) {
        self.by_type.insert(
            TypeId::of::<T>(),
            Registered {
                payload_type,
                encoder,
            },
        );
    }

    /// Returns the number of registered encoders.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_type.len()
    }

    /// Returns whether the registry is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_type.is_empty()
    }

    /// Returns whether an encoder is registered for `T`.
    #[must_use]
    pub fn contains<T: 'static>(&self) -> bool {
        self.by_type.contains_key(&TypeId::of::<T>())
    }

    /// Encodes `message` if a typed encoder is registered for `T`.
    ///
    /// Returns `Ok(None)` when no encoder is registered for `T` so the adapter can drop
    /// the message at the dispatch boundary without surfacing it as an error: the
    /// allow-list is the source of truth for the captured surface, and out-of-list
    /// messages are non-state-affecting by definition.
    ///
    /// # Errors
    ///
    /// Returns the encoder's [`EncodeError`] when an encoder is registered but rejects
    /// the message.
    pub fn encode<T: 'static>(
        &self,
        message: &T,
    ) -> Result<Option<(PayloadType, EncodedPayload)>, EncodeError> {
        let Some(reg) = self.by_type.get(&TypeId::of::<T>()) else {
            return Ok(None);
        };

        let encoded = reg.encoder.encode(message as &dyn Any)?;
        let payload_type = encoded.payload_type.unwrap_or(reg.payload_type);
        Ok(Some((payload_type, encoded)))
    }

    /// Encodes a type-erased `message` if an encoder is registered for the concrete type.
    ///
    /// Mirror of [`Self::encode`] for `&dyn Any` callers; the bus tap reaches the
    /// capture path through this entry point because dispatch returns a `&dyn Any` and
    /// the static type is not in scope.
    ///
    /// # Errors
    ///
    /// Returns the encoder's [`EncodeError`] when an encoder is registered for the
    /// concrete type but rejects the message.
    pub fn encode_any(
        &self,
        message: &dyn Any,
    ) -> Result<Option<(PayloadType, EncodedPayload)>, EncodeError> {
        let Some(reg) = self.by_type.get(&message.type_id()) else {
            return Ok(None);
        };

        let encoded = reg.encoder.encode(message)?;
        let payload_type = encoded.payload_type.unwrap_or(reg.payload_type);
        Ok(Some((payload_type, encoded)))
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

    #[derive(Debug)]
    struct Sample(u8);

    #[derive(Debug)]
    struct Other;

    #[rstest]
    fn unknown_type_returns_none() {
        let registry = EncoderRegistry::new();

        assert!(registry.encode(&Sample(1)).expect("encode").is_none());
        assert!(!registry.contains::<Sample>());
    }

    #[rstest]
    fn registered_type_returns_payload_type_and_payload() {
        let mut registry = EncoderRegistry::new();
        registry.register::<Sample, _>(Ustr::from("Sample"), |s| {
            Ok(EncodedPayload::without_indices(Bytes::copy_from_slice(&[
                s.0,
            ])))
        });

        let (tag, encoded) = registry.encode(&Sample(9)).expect("encode").expect("hit");

        assert_eq!(tag.as_str(), "Sample");
        assert_eq!(encoded.payload.as_ref(), &[9]);
        assert!(registry.contains::<Sample>());
        assert_eq!(registry.len(), 1);
    }

    #[rstest]
    fn re_registering_replaces_prior_encoder() {
        let mut registry = EncoderRegistry::new();
        registry.register::<Sample, _>(Ustr::from("Old"), |s| {
            Ok(EncodedPayload::without_indices(Bytes::copy_from_slice(&[
                s.0,
            ])))
        });
        registry.register::<Sample, _>(Ustr::from("New"), |s| {
            Ok(EncodedPayload::without_indices(Bytes::copy_from_slice(&[
                s.0, s.0,
            ])))
        });

        let (tag, encoded) = registry.encode(&Sample(3)).expect("encode").expect("hit");

        assert_eq!(tag.as_str(), "New");
        assert_eq!(encoded.payload.as_ref(), &[3, 3]);
        assert_eq!(registry.len(), 1);
    }

    #[rstest]
    fn registry_is_empty_by_default() {
        let registry = EncoderRegistry::new();

        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert!(!registry.contains::<Other>());
    }

    #[rstest]
    fn encode_any_dispatches_by_concrete_type_id() {
        // The bus tap reaches the registry through `encode_any` because the static
        // type is not in scope at the dispatch site. Verify the &dyn Any lookup
        // resolves to the same registration as the typed `encode<T>` path.
        let mut registry = EncoderRegistry::new();
        registry.register::<Sample, _>(Ustr::from("Sample"), |s| {
            Ok(EncodedPayload::without_indices(Bytes::copy_from_slice(&[
                s.0,
            ])))
        });

        let sample = Sample(5);
        let (tag, encoded) = registry
            .encode_any(&sample as &dyn Any)
            .expect("encode_any")
            .expect("hit");

        assert_eq!(tag.as_str(), "Sample");
        assert_eq!(encoded.payload.as_ref(), &[5]);
    }

    #[rstest]
    fn encode_any_returns_none_for_unregistered_type() {
        // Out-of-allow-list messages must surface as `Ok(None)` so the adapter can
        // skip them silently at the dispatch boundary rather than treating them as
        // capture failures.
        let registry = EncoderRegistry::new();

        let unregistered = Other;
        let outcome = registry
            .encode_any(&unregistered as &dyn Any)
            .expect("encode_any");

        assert!(outcome.is_none());
    }

    #[rstest]
    fn encoder_payload_type_override_overrides_registered_tag() {
        // Envelope encoders (TradingCommand, OrderEventAny) need to stamp the
        // inner-variant tag on captured entries so forensics scans see the same
        // payload_type as the bare-type capture path. The override mechanism is
        // tested here independent of the bus surface so the registry contract
        // is self-documenting.
        let mut registry = EncoderRegistry::new();
        registry.register::<Sample, _>(Ustr::from("Wrapper"), |s| {
            Ok(EncodedPayload::with_payload_type(
                Ustr::from("Inner"),
                Bytes::copy_from_slice(&[s.0]),
                Vec::new(),
            ))
        });

        let (tag, _) = registry.encode(&Sample(1)).expect("encode").expect("hit");
        assert_eq!(tag.as_str(), "Inner");

        let (any_tag, _) = registry
            .encode_any(&Sample(1) as &dyn Any)
            .expect("encode_any")
            .expect("hit");
        assert_eq!(any_tag.as_str(), "Inner");
    }
}
