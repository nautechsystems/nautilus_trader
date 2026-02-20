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

//! Python bindings for dYdX ClientOrderId encoder.

#![allow(clippy::missing_errors_doc)]

use std::sync::Arc;

use nautilus_core::python::to_pyruntime_err;
use nautilus_model::identifiers::ClientOrderId;
use pyo3::prelude::*;

use crate::execution::encoder::ClientOrderIdEncoder;

/// Python wrapper for the ClientOrderIdEncoder.
///
/// Provides bidirectional encoding of Nautilus ClientOrderId strings to
/// dYdX's (client_id, client_metadata) u32 pair.
#[pyclass(name = "DydxClientOrderIdEncoder")]
#[derive(Debug)]
pub struct PyDydxClientOrderIdEncoder {
    inner: Arc<ClientOrderIdEncoder>,
}

impl PyDydxClientOrderIdEncoder {
    /// Creates a Python encoder wrapping an existing shared `Arc<ClientOrderIdEncoder>`.
    pub fn from_arc(inner: Arc<ClientOrderIdEncoder>) -> Self {
        Self { inner }
    }
}

#[pymethods]
impl PyDydxClientOrderIdEncoder {
    /// Create a new ClientOrderIdEncoder.
    #[new]
    fn new() -> Self {
        Self {
            inner: Arc::new(ClientOrderIdEncoder::new()),
        }
    }

    /// Encode a ClientOrderId string to (client_id, client_metadata) tuple.
    ///
    /// # Encoding Rules
    ///
    /// 1. Numeric IDs (e.g., "12345"): Returns `(12345, 4)` for backward compatibility
    /// 2. O-format IDs (e.g., "O-20260131-174827-001-001-1"): Deterministically encoded
    /// 3. Other formats: Sequential allocation with in-memory mapping
    ///
    /// # Errors
    ///
    /// Returns an error if the encoder's sequential counter overflows.
    #[pyo3(name = "encode")]
    fn py_encode(&self, client_order_id: &str) -> PyResult<(u32, u32)> {
        let id = ClientOrderId::from(client_order_id);
        let encoded = self.inner.encode(id).map_err(to_pyruntime_err)?;
        Ok((encoded.client_id, encoded.client_metadata))
    }

    /// Decode (client_id, client_metadata) back to the original ClientOrderId string.
    ///
    /// # Decoding Rules
    ///
    /// 1. If `client_metadata == 4`: Returns numeric string (legacy format)
    /// 2. If sequential allocation marker: Looks up in reverse mapping
    /// 3. Otherwise: Decodes as O-format using timestamp + identity bits
    ///
    /// Returns `None` if decoding fails (e.g., sequential ID not in cache after restart).
    #[pyo3(name = "decode")]
    fn py_decode(&self, client_id: u32, client_metadata: u32) -> Option<String> {
        self.inner
            .decode(client_id, client_metadata)
            .map(|id| id.to_string())
    }

    /// Get the encoded pair for a ClientOrderId without allocating a new mapping.
    ///
    /// Returns `None` if the ID is not in the cache and is not a deterministic format
    /// (numeric or O-format).
    #[pyo3(name = "get")]
    fn py_get(&self, client_order_id: &str) -> Option<(u32, u32)> {
        let id = ClientOrderId::from(client_order_id);
        self.inner
            .get(&id)
            .map(|encoded| (encoded.client_id, encoded.client_metadata))
    }

    /// Remove the mapping for a given encoded pair.
    ///
    /// Returns the original ClientOrderId string if it was mapped.
    /// For deterministic formats, this returns the decoded value but doesn't
    /// actually remove anything (since they don't use in-memory mappings).
    #[pyo3(name = "remove")]
    fn py_remove(&self, client_id: u32, client_metadata: u32) -> Option<String> {
        self.inner
            .remove(client_id, client_metadata)
            .map(|id| id.to_string())
    }

    /// Update the mapping for a ClientOrderId after order modification.
    ///
    /// Returns the current sequential counter value (for debugging/monitoring).
    #[pyo3(name = "current_counter")]
    fn py_current_counter(&self) -> u32 {
        self.inner.current_counter()
    }

    /// Returns the number of non-deterministic mappings currently stored.
    #[pyo3(name = "len")]
    fn py_len(&self) -> usize {
        self.inner.len()
    }

    /// Returns true if no non-deterministic mappings are stored.
    #[pyo3(name = "is_empty")]
    fn py_is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn __repr__(&self) -> String {
        format!(
            "DydxClientOrderIdEncoder(counter={}, mappings={})",
            self.inner.current_counter(),
            self.inner.len()
        )
    }
}
