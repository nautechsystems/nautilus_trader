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

//! Shared writer sink trait declarations.

use std::{any::Any, fmt::Debug};

use nautilus_model::data::Data;

/// Object-safe streaming sink boundary substitutable by streaming backends.
pub trait StreamingDataSink: Debug {
    /// Writes a single `Data` enum value.
    /// # Errors
    ///
    /// Returns an error if the sink cannot serialize or persist the value.
    fn write_data(&mut self, data: Data) -> anyhow::Result<()>;

    /// Writes a batch of `Data` values.
    /// # Errors
    ///
    /// Returns an error if any item cannot be serialized or persisted.
    fn write_batch(&mut self, data: Vec<Data>) -> anyhow::Result<()>;

    /// Writes any supported Nautilus message value.
    /// # Errors
    ///
    /// Returns an error if the supported message cannot be serialized or persisted.
    fn write_any(&mut self, message: &dyn Any) -> anyhow::Result<bool>;

    /// Flushes buffered data to durable storage.
    /// # Errors
    ///
    /// Returns an error if buffered data cannot be flushed.
    fn flush(&mut self) -> anyhow::Result<()>;

    /// Closes sink after flushing buffered data.
    /// # Errors
    ///
    /// Returns an error if flushing or closing the sink fails.
    fn close(&mut self) -> anyhow::Result<()>;
}

/// Boxed streaming sink trait object.
pub type StreamingSinkBox = Box<dyn StreamingDataSink>;
