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

use std::{collections::HashMap, sync::Arc};

use arrow::{
    array::{BooleanArray, StringBuilder, UInt64Array},
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::events::AccountState;

use crate::arrow::{ArrowSchemaProvider, EncodeToRecordBatch};

impl ArrowSchemaProvider for AccountState {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("account_id", DataType::Utf8, false),
            Field::new("account_type", DataType::Utf8, false),
            Field::new("base_currency", DataType::Utf8, true),
            Field::new("balances", DataType::Utf8, false),
            Field::new("margins", DataType::Utf8, false),
            Field::new("is_reported", DataType::Boolean, false),
            Field::new("event_id", DataType::Utf8, false),
            Field::new("ts_event", DataType::UInt64, false),
            Field::new("ts_init", DataType::UInt64, false),
        ];

        match metadata {
            Some(metadata) => Schema::new_with_metadata(fields, metadata),
            None => Schema::new(fields),
        }
    }
}

impl EncodeToRecordBatch for AccountState {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        let mut account_id_builder = StringBuilder::new();
        let mut account_type_builder = StringBuilder::new();
        let mut base_currency_builder = StringBuilder::new();
        let mut balances_builder = StringBuilder::new();
        let mut margins_builder = StringBuilder::new();
        let mut is_reported_builder = BooleanArray::builder(data.len());
        let mut event_id_builder = StringBuilder::new();
        let mut ts_event_builder = UInt64Array::builder(data.len());
        let mut ts_init_builder = UInt64Array::builder(data.len());

        for event in data {
            account_id_builder.append_value(event.account_id.as_str());
            account_type_builder.append_value(format!("{:?}", event.account_type));

            match &event.base_currency {
                Some(currency) => base_currency_builder.append_value(currency.code.as_str()),
                None => base_currency_builder.append_null(),
            }

            let balances_json = serde_json::to_string(&event.balances)
                .map_err(|e| ArrowError::ExternalError(Box::new(e)))?;
            balances_builder.append_value(&balances_json);

            let margins_json = serde_json::to_string(&event.margins)
                .map_err(|e| ArrowError::ExternalError(Box::new(e)))?;
            margins_builder.append_value(&margins_json);

            is_reported_builder.append_value(event.is_reported);
            event_id_builder.append_value(event.event_id.to_string());
            ts_event_builder.append_value(event.ts_event.as_u64());
            ts_init_builder.append_value(event.ts_init.as_u64());
        }

        RecordBatch::try_new(
            Self::get_schema(Some(metadata.clone())).into(),
            vec![
                Arc::new(account_id_builder.finish()),
                Arc::new(account_type_builder.finish()),
                Arc::new(base_currency_builder.finish()),
                Arc::new(balances_builder.finish()),
                Arc::new(margins_builder.finish()),
                Arc::new(is_reported_builder.finish()),
                Arc::new(event_id_builder.finish()),
                Arc::new(ts_event_builder.finish()),
                Arc::new(ts_init_builder.finish()),
            ],
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        HashMap::from([("account_id".to_string(), self.account_id.to_string())])
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{enums::AccountType, identifiers::AccountId, types::Currency};
    use rstest::rstest;

    use super::*;

    fn stub_account_state() -> AccountState {
        AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Cash,
            vec![],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
            Some(Currency::USD()),
        )
    }

    #[rstest]
    fn test_account_state_schema_has_all_fields() {
        let schema = AccountState::get_schema(None);
        let fields = schema.fields();

        assert_eq!(fields.len(), 9);

        assert_eq!(fields[0].name(), "account_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);
        assert!(!fields[0].is_nullable());

        assert_eq!(fields[1].name(), "account_type");
        assert_eq!(fields[2].name(), "base_currency");
        assert!(fields[2].is_nullable());

        assert_eq!(fields[3].name(), "balances");
        assert_eq!(fields[4].name(), "margins");

        assert_eq!(fields[5].name(), "is_reported");
        assert_eq!(fields[5].data_type(), &DataType::Boolean);
        assert!(!fields[5].is_nullable());

        assert_eq!(fields[6].name(), "event_id");

        assert_eq!(fields[7].name(), "ts_event");
        assert_eq!(fields[7].data_type(), &DataType::UInt64);

        assert_eq!(fields[8].name(), "ts_init");
        assert_eq!(fields[8].data_type(), &DataType::UInt64);
    }

    #[rstest]
    fn test_account_state_encode_single() {
        let state = stub_account_state();
        let metadata = state.metadata();
        let record_batch = AccountState::encode_batch(&metadata, &[state]).unwrap();

        assert_eq!(record_batch.num_rows(), 1);
        assert_eq!(record_batch.num_columns(), 9);
    }

    #[rstest]
    fn test_account_state_encode_multiple() {
        let state1 = stub_account_state();
        let state2 = AccountState::new(
            AccountId::new("SIM-001"),
            AccountType::Margin,
            vec![],
            vec![],
            false,
            UUID4::new(),
            UnixNanos::from(3),
            UnixNanos::from(4),
            None,
        );

        let data = vec![state1, state2];
        let metadata = AccountState::chunk_metadata(&data);
        let record_batch = AccountState::encode_batch(&metadata, &data).unwrap();

        assert_eq!(record_batch.num_rows(), 2);
        assert_eq!(record_batch.num_columns(), 9);
    }
}
