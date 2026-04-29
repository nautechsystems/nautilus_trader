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
    json::{JsonFieldSpec, decode_batch, encode_batch, metadata_for_type, schema_for_type},
};

const ACCOUNT_STATE_FIELDS: &[JsonFieldSpec] = &[
    JsonFieldSpec::utf8("account_id", false),
    JsonFieldSpec::utf8("account_type", false),
    JsonFieldSpec::utf8("base_currency", true),
    JsonFieldSpec::utf8_json("balances", false),
    JsonFieldSpec::utf8_json("margins", false),
    JsonFieldSpec::boolean("is_reported", false),
    JsonFieldSpec::utf8("event_id", false),
    JsonFieldSpec::u64("ts_event", false),
    JsonFieldSpec::u64("ts_init", false),
];

impl ArrowSchemaProvider for AccountState {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        schema_for_type("AccountState", metadata, ACCOUNT_STATE_FIELDS)
    }
}

impl EncodeToRecordBatch for AccountState {
    fn encode_batch(
        metadata: &HashMap<String, String>,
        data: &[Self],
    ) -> Result<RecordBatch, ArrowError> {
        encode_batch("AccountState", metadata, data, ACCOUNT_STATE_FIELDS)
    }

    fn metadata(&self) -> HashMap<String, String> {
        metadata_for_type("AccountState")
    }
}

impl DecodeTypedFromRecordBatch for AccountState {
    fn decode_typed_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        decode_batch(
            metadata,
            &record_batch,
            ACCOUNT_STATE_FIELDS,
            Some("AccountState"),
        )
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::events::account::stubs::cash_account_state;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_account_state_round_trip(cash_account_state: AccountState) {
        let state = cash_account_state;
        let metadata = state.metadata();
        let batch = AccountState::encode_batch(&metadata, std::slice::from_ref(&state)).unwrap();
        let decoded = AccountState::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].account_id, state.account_id);
        assert_eq!(decoded[0].balances, state.balances);
        assert_eq!(decoded[0].margins, state.margins);
        assert_eq!(decoded[0].base_currency, state.base_currency);
    }
}
