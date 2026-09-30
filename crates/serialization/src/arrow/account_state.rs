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

use std::collections::HashMap;

use arrow::{datatypes::Schema, error::ArrowError, record_batch::RecordBatch};
use nautilus_model::events::AccountState;

use super::{
    ArrowSchemaProvider, DecodeTypedFromRecordBatch, EncodeToRecordBatch, EncodingError,
    KEY_ACCOUNT_ID,
    json::{
        JsonFieldSpec, decode_batch_with_metadata_fields, encode_batch_with_identifier,
        metadata_for_type, schema_for_type_with_identifier,
    },
};

const ACCOUNT_STATE_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("account_type", false),
    JsonFieldSpec::utf8("base_currency", true),
    JsonFieldSpec::utf8_json("balances", false),
    JsonFieldSpec::utf8_json("margins", false),
    JsonFieldSpec::boolean("is_reported", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::timestamp("ts_event", false),
    JsonFieldSpec::timestamp("ts_init", false),
    JsonFieldSpec::utf8_json("info", true),
];

impl ArrowSchemaProvider for AccountState {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        schema_for_type_with_identifier("AccountState", metadata, ACCOUNT_STATE_FIELDS)
    }
}

impl EncodeToRecordBatch for AccountState {
    fn encode_batch<T>(
        metadata: &HashMap<String, String>,
        data: &[T],
    ) -> Result<RecordBatch, ArrowError>
    where
        T: std::borrow::Borrow<Self>,
    {
        encode_batch_with_identifier(
            "AccountState",
            metadata,
            data.iter().map(std::borrow::Borrow::borrow),
            ACCOUNT_STATE_FIELDS,
            data.iter()
                .map(std::borrow::Borrow::borrow)
                .map(|state| state.account_id),
        )
    }

    fn metadata(&self) -> HashMap<String, String> {
        let mut metadata = metadata_for_type("AccountState");
        metadata.insert(KEY_ACCOUNT_ID.to_string(), self.account_id.to_string());
        metadata
    }

    fn identifier(&self) -> Option<String> {
        Some(self.account_id.to_string())
    }
}

impl DecodeTypedFromRecordBatch for AccountState {
    fn decode_typed_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let fields = if record_batch.schema().index_of("info").is_ok() {
            ACCOUNT_STATE_FIELDS
        } else {
            &ACCOUNT_STATE_FIELDS[..ACCOUNT_STATE_FIELDS.len() - 1]
        };
        decode_batch_with_metadata_fields(
            metadata,
            &record_batch,
            fields,
            &[KEY_ACCOUNT_ID],
            Some("AccountState"),
        )
    }
}

#[cfg(test)]
mod tests {
    use arrow::array::Array;
    use nautilus_core::Params;
    use nautilus_model::events::account::stubs::cash_account_state;
    use rstest::rstest;
    use serde_json::json;

    use super::*;
    use crate::arrow::{KEY_IDENTIFIER, json::encode_batch};

    #[rstest]
    fn test_account_state_round_trip(cash_account_state: AccountState) {
        let mut info = Params::new();
        info.insert(
            "total_wallet_balance".to_string(),
            json!("1525000.00000001"),
        );
        info.insert("can_trade".to_string(), json!(true));
        let state = cash_account_state.with_info(Some(info));
        let metadata = state.metadata();
        let batch = AccountState::encode_batch(&metadata, std::slice::from_ref(&state)).unwrap();
        let decoded = AccountState::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].account_id, state.account_id);
        assert_eq!(decoded[0].balances, state.balances);
        assert_eq!(decoded[0].margins, state.margins);
        assert_eq!(decoded[0].base_currency, state.base_currency);
        assert_eq!(decoded[0].info, state.info);
    }

    #[rstest]
    fn test_account_state_stores_account_id_once_as_identifier(cash_account_state: AccountState) {
        let metadata = cash_account_state.metadata();

        let batch =
            AccountState::encode_batch(&metadata, std::slice::from_ref(&cash_account_state))
                .unwrap();
        let identifiers = batch
            .column_by_name(KEY_IDENTIFIER)
            .unwrap()
            .as_any()
            .downcast_ref::<arrow::array::StringArray>()
            .unwrap();
        let decoded =
            AccountState::decode_typed_batch(batch.schema().metadata(), batch.clone()).unwrap();

        assert_eq!(identifiers.len(), 1);
        assert_eq!(
            identifiers.value(0),
            cash_account_state.account_id.to_string()
        );
        assert!(batch.column_by_name("account_id").is_none());
        assert_eq!(decoded[0].account_id, cash_account_state.account_id);
    }

    #[rstest]
    fn test_account_state_decodes_legacy_batch_without_info(cash_account_state: AccountState) {
        let metadata = cash_account_state.metadata();
        let legacy_fields = &ACCOUNT_STATE_FIELDS[..ACCOUNT_STATE_FIELDS.len() - 1];
        let batch = encode_batch(
            "AccountState",
            &metadata,
            std::slice::from_ref(&cash_account_state),
            legacy_fields,
        )
        .unwrap();
        let decoded = AccountState::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded.len(), 1);
        assert!(decoded[0].info.is_none());
    }
}
