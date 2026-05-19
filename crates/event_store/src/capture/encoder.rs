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

//! Encoders that convert captured bus messages into canonical payload bytes.
//!
//! Each registered encoder is responsible for two outputs:
//!
//! - The canonical payload bytes, written verbatim into the entry's `payload` field. The
//!   reader pairs the bytes with [`crate::PayloadType`] to dispatch the matching decoder.
//! - The sidecar [`IndexKey`]s the writer commits in the same backend transaction so
//!   forensics scans by `client_order_id` or `venue_order_id` resolve to a committed
//!   `seq` rather than missing entries the reader can observe before the indices catch
//!   up.
//!
//! The trait is type-erased so the registry can lookup by [`std::any::TypeId`]; concrete
//! encoders are typed via the [`TypedEncoder`] adapter and avoid downcasting at the call
//! site.

use std::{any::Any, marker::PhantomData};

use bytes::Bytes;

use crate::{backend::IndexKey, entry::PayloadType};

/// Errors returned by an [`Encode`] implementation.
#[derive(Debug, thiserror::Error)]
pub enum EncodeError {
    /// The encoder received a value of an unexpected type.
    ///
    /// Surfaces only when the registry is wired incorrectly. The adapter rejects callers
    /// before invoking the encoder, so this variant exists to keep [`TypedEncoder`]'s
    /// downcast safe under future refactors.
    #[error("encoder type mismatch: expected {expected}")]
    TypeMismatch {
        /// The Rust type name the encoder was registered for.
        expected: &'static str,
    },
    /// The encoder failed to serialize the message.
    #[error("encode failure: {0}")]
    Serialize(String),
}

/// The canonical payload plus sidecar indices an encoder produces for one captured message.
///
/// The encoder does not stamp `seq`, `ts_publish`, or `entry_hash`: those are writer-side
/// fields. It also does not stamp `headers`; the bus capture adapter carries headers from
/// the dispatch boundary so encoders stay focused on payload identity.
///
/// `payload_type` is `None` for the typical bare-type encoder (e.g. `SubmitOrder`): the
/// registry's registered tag is used. Envelope encoders that dispatch on a wrapper enum
/// (e.g. `TradingCommand`, `OrderEventAny`) set it to the inner-variant's canonical tag
/// so forensics scans see entries identical to the bare-type capture path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedPayload {
    /// The canonical encoded bytes the writer commits as the entry payload.
    pub payload: Bytes,
    /// Sidecar index keys produced for this entry. May be empty.
    pub index_keys: Vec<IndexKey>,
    /// Optional override for the registry's registered payload type tag. Set by envelope
    /// encoders to stamp the inner-variant tag on captured entries.
    pub payload_type: Option<PayloadType>,
}

impl EncodedPayload {
    /// Creates a new [`EncodedPayload`] that inherits the registry's registered tag.
    #[must_use]
    pub const fn new(payload: Bytes, index_keys: Vec<IndexKey>) -> Self {
        Self {
            payload,
            index_keys,
            payload_type: None,
        }
    }

    /// Creates a new [`EncodedPayload`] with no sidecar indices.
    #[must_use]
    pub const fn without_indices(payload: Bytes) -> Self {
        Self {
            payload,
            index_keys: Vec::new(),
            payload_type: None,
        }
    }

    /// Creates a new [`EncodedPayload`] that stamps `payload_type` on the captured entry,
    /// overriding the registry's registered tag.
    #[must_use]
    pub const fn with_payload_type(
        payload_type: PayloadType,
        payload: Bytes,
        index_keys: Vec<IndexKey>,
    ) -> Self {
        Self {
            payload,
            index_keys,
            payload_type: Some(payload_type),
        }
    }
}

/// A type-erased encoder used by the registry.
///
/// Implementors take the captured message as `&dyn Any` so the registry can dispatch by
/// [`std::any::TypeId`] without naming the concrete type at the call site. Most callers
/// build encoders with [`TypedEncoder`] rather than implementing this trait directly.
pub trait Encode: Send + Sync {
    /// Encodes the supplied message into canonical payload bytes plus sidecar indices.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::TypeMismatch`] when `message` does not match the encoder's
    /// expected type, and [`EncodeError::Serialize`] for any encoder-internal failure.
    fn encode(&self, message: &dyn Any) -> Result<EncodedPayload, EncodeError>;
}

/// A typed wrapper that adapts a `Fn(&T) -> Result<EncodedPayload, EncodeError>` to
/// [`Encode`].
///
/// Constructed by [`crate::capture::EncoderRegistry::register`]; callers rarely instantiate
/// directly. The downcast is the only `Any`-handling site in the capture path.
pub struct TypedEncoder<T: 'static, F> {
    func: F,
    _phantom: PhantomData<fn(&T)>,
}

impl<T: 'static, F> TypedEncoder<T, F>
where
    F: Fn(&T) -> Result<EncodedPayload, EncodeError> + Send + Sync,
{
    /// Wraps `func` as a typed encoder for `T`.
    #[must_use]
    pub const fn new(func: F) -> Self {
        Self {
            func,
            _phantom: PhantomData,
        }
    }
}

impl<T: 'static, F> std::fmt::Debug for TypedEncoder<T, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(TypedEncoder))
            .field("type", &std::any::type_name::<T>())
            .finish_non_exhaustive()
    }
}

impl<T: 'static, F> Encode for TypedEncoder<T, F>
where
    F: Fn(&T) -> Result<EncodedPayload, EncodeError> + Send + Sync,
{
    fn encode(&self, message: &dyn Any) -> Result<EncodedPayload, EncodeError> {
        let typed = message
            .downcast_ref::<T>()
            .ok_or(EncodeError::TypeMismatch {
                expected: std::any::type_name::<T>(),
            })?;
        (self.func)(typed)
    }
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use rstest::rstest;

    use super::*;
    use crate::backend::IndexKind;

    #[derive(Debug)]
    struct Sample(u8);

    #[derive(Debug)]
    struct Other;

    fn sample_encoder()
    -> TypedEncoder<Sample, impl Fn(&Sample) -> Result<EncodedPayload, EncodeError> + Send + Sync>
    {
        TypedEncoder::<Sample, _>::new(|s: &Sample| {
            Ok(EncodedPayload::new(
                Bytes::copy_from_slice(&[s.0]),
                vec![IndexKey::new(
                    IndexKind::ClientOrderId,
                    format!("CLI-{}", s.0),
                )],
            ))
        })
    }

    #[rstest]
    fn typed_encoder_encodes_matching_value() {
        let encoder = sample_encoder();
        let encoded = encoder.encode(&Sample(7)).expect("encode");

        assert_eq!(encoded.payload.as_ref(), &[7]);
        assert_eq!(encoded.index_keys.len(), 1);
        assert_eq!(encoded.index_keys[0].kind, IndexKind::ClientOrderId);
        assert_eq!(encoded.index_keys[0].key, "CLI-7");
    }

    #[rstest]
    fn typed_encoder_rejects_other_type() {
        let encoder = sample_encoder();
        let err = encoder.encode(&Other).expect_err("type mismatch");

        match err {
            EncodeError::TypeMismatch { expected } => {
                assert!(expected.ends_with("Sample"), "expected was: {expected}");
            }
            EncodeError::Serialize(_) => panic!("expected TypeMismatch, was Serialize"),
        }
    }

    #[rstest]
    fn encoded_payload_without_indices_has_empty_indices() {
        let payload = EncodedPayload::without_indices(Bytes::from_static(b"abc"));

        assert_eq!(payload.payload.as_ref(), b"abc");
        assert!(payload.index_keys.is_empty());
    }
}
