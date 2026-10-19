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

//! Known payload storage charged by owned callback routes.

#![allow(
    dead_code,
    reason = "owned callback routes remain inactive pending runtime integration"
)]

use indexmap::IndexMap;
use nautilus_core::Params;
#[cfg(feature = "defi")]
use nautilus_model::defi::{Block, Pool};
use nautilus_model::{
    data::{
        BookOrder, CustomData, DataType,
        option_chain::{OptionChainSlice, OptionStrikeData},
    },
    events::OrderEventAny,
    identifiers::ClientOrderId,
    orderbook::{OrderBook, level::BookLevel},
    types::Price,
};
use serde_json::Value;
use ustr::Ustr;

/// Measures known storage for a custom-data envelope, excluding the opaque payload.
#[must_use]
pub(super) fn custom_data(data: &CustomData) -> usize {
    data_type(&data.data_type)
}

/// Measures string and metadata contents retained by a data type.
#[must_use]
pub(super) fn data_type(data: &DataType) -> usize {
    data.type_name()
        .len()
        .saturating_add(data.topic().len())
        .saturating_add(data.identifier().map_or(0, str::len))
        .saturating_add(data.metadata().map_or(0, params))
}

/// Measures accessible key, value, and element storage in request metadata.
#[must_use]
pub(super) fn params(params: &Params) -> usize {
    params.iter().fold(
        params
            .capacity()
            .saturating_mul(size_of::<(usize, String, Value)>()),
        |bytes, (key, value)| {
            bytes
                .saturating_add(key.capacity())
                .saturating_add(json(value))
        },
    )
}

/// Measures variable-sized contents of an order event.
#[must_use]
pub(super) fn order_event(event: &OrderEventAny) -> usize {
    match event {
        OrderEventAny::Initialized(event) => event
            .linked_order_ids
            .as_ref()
            .map_or(0, |ids| {
                ids.capacity().saturating_mul(size_of::<ClientOrderId>())
            })
            .saturating_add(
                event
                    .tags
                    .as_ref()
                    .map_or(0, |tags| tags.capacity().saturating_mul(size_of::<Ustr>())),
            )
            .saturating_add(event.exec_algorithm_params.as_ref().map_or(0, interned_map)),
        OrderEventAny::Filled(event) => event.info.as_ref().map_or(0, interned_map),
        OrderEventAny::FillVoided(event) => event.info.as_ref().map_or(0, interned_map),
        _ => 0,
    }
}

fn interned_map(map: &IndexMap<Ustr, Ustr>) -> usize {
    map.capacity()
        .saturating_mul(size_of::<(usize, Ustr, Ustr)>())
}

fn json(value: &Value) -> usize {
    match value {
        Value::String(value) => value.capacity(),
        Value::Array(values) => values.iter().fold(
            values.capacity().saturating_mul(size_of::<Value>()),
            |bytes, value| bytes.saturating_add(json(value)),
        ),
        Value::Object(values) => values.iter().fold(0usize, |bytes, (key, value)| {
            bytes
                .saturating_add(size_of::<(String, Value)>())
                .saturating_add(key.capacity())
                .saturating_add(json(value))
        }),
        _ => 0,
    }
}

/// Measures visible levels and orders retained by an order-book snapshot.
#[must_use]
pub(super) fn book(book: &OrderBook) -> usize {
    book.bids(None)
        .chain(book.asks(None))
        .fold(0usize, |bytes, level| {
            bytes
                .saturating_add(size_of::<BookLevel>())
                .saturating_add(level.len().saturating_mul(size_of::<(u64, BookOrder)>()))
        })
}

/// Measures option strike entries retained by a chain slice.
#[must_use]
pub(super) fn option_chain(slice: &OptionChainSlice) -> usize {
    slice
        .calls
        .len()
        .saturating_add(slice.puts.len())
        .saturating_mul(size_of::<(Price, OptionStrikeData)>())
}

/// Measures directly owned strings in a blockchain block.
#[cfg(feature = "defi")]
#[must_use]
pub(super) fn block(block: &Block) -> usize {
    block
        .hash
        .capacity()
        .saturating_add(block.parent_hash.capacity())
}

/// Measures directly owned token strings in a pool definition.
#[cfg(feature = "defi")]
#[must_use]
pub(super) fn pool(pool: &Pool) -> usize {
    pool.token0
        .name
        .capacity()
        .saturating_add(pool.token0.symbol.capacity())
        .saturating_add(pool.token1.name.capacity())
        .saturating_add(pool.token1.symbol.capacity())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn variable_payload_capacity_is_charged_recursively() {
        let mut text = String::with_capacity(137);
        text.push_str("value");
        let capacity = text.capacity();
        let mut values = Vec::with_capacity(7);
        values.push(Value::String(text));
        let expected = values.capacity() * size_of::<Value>() + capacity;
        assert_eq!(json(&Value::Array(values)), expected);
    }

    #[rstest]
    fn metadata_charges_keys_and_nested_values() {
        let mut values = Params::default();
        let mut key = String::with_capacity(41);
        key.push_str("key");
        let mut value = String::with_capacity(83);
        value.push_str("payload");
        let contents = key.capacity() + value.capacity();
        values.insert(key, Value::String(value));
        assert_eq!(
            params(&values),
            values.capacity() * size_of::<(usize, String, Value)>() + contents
        );
    }
}
