// -------------------------------------------------------------------------------------------------
// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// https://nautechsystems.io
//
// Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
// You may not use this file except in compliance with the License.
// You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
// -------------------------------------------------------------------------------------------------

use std::sync::Arc;

use arrow::{
    array::{ArrayBuilder, Float64Builder, ListArray, StructArray, UInt32Builder, UInt64Builder},
    buffer::{OffsetBuffer, ScalarBuffer},
    datatypes::{DataType, Field, Fields, Schema},
    error::ArrowError,
};

use crate::arrow::timestamp_data_type;

/// Returns the nested display schema, independent of the number of book levels.
pub(super) fn schema() -> Schema {
    let side = DataType::List(Arc::new(Field::new(
        "item",
        DataType::Struct(level_fields()),
        false,
    )));
    Schema::new(vec![
        Field::new("instrument_id", DataType::Utf8, false),
        Field::new("bids", side.clone(), false),
        Field::new("asks", side, false),
        Field::new("flags", DataType::UInt8, false),
        Field::new("sequence", DataType::UInt64, false),
        Field::new("ts_event", timestamp_data_type(), false),
        Field::new("ts_init", timestamp_data_type(), false),
    ])
}

fn level_fields() -> Fields {
    vec![
        Field::new("price", DataType::Float64, true),
        Field::new("size", DataType::Float64, true),
        Field::new("count", DataType::UInt32, false),
        Field::new("order_id", DataType::UInt64, false),
    ]
    .into()
}

pub(super) struct DepthSideBuilder {
    offsets: Vec<i32>,
    pub(super) prices: Float64Builder,
    pub(super) sizes: Float64Builder,
    pub(super) counts: UInt32Builder,
    pub(super) order_ids: UInt64Builder,
}

impl DepthSideBuilder {
    pub(super) fn new() -> Self {
        Self {
            offsets: vec![0],
            prices: Float64Builder::new(),
            sizes: Float64Builder::new(),
            counts: UInt32Builder::new(),
            order_ids: UInt64Builder::new(),
        }
    }

    pub(super) fn finish_row(&mut self) -> Result<(), ArrowError> {
        let offset = i32::try_from(self.prices.len())
            .map_err(|e| ArrowError::InvalidArgumentError(e.to_string()))?;
        self.offsets.push(offset);
        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<ListArray, ArrowError> {
        let fields = level_fields();
        let values = StructArray::try_new(
            fields.clone(),
            vec![
                Arc::new(self.prices.finish()),
                Arc::new(self.sizes.finish()),
                Arc::new(self.counts.finish()),
                Arc::new(self.order_ids.finish()),
            ],
            None,
        )?;
        ListArray::try_new(
            Arc::new(Field::new("item", DataType::Struct(fields), false)),
            OffsetBuffer::new(ScalarBuffer::from(self.offsets)),
            Arc::new(values),
            None,
        )
    }
}
