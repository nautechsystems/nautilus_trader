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

use nautilus_model::events::AccountState;

use super::json::{JsonFieldSpec, impl_json_arrow};

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
    JsonFieldSpec::utf8_json("info", true),
];

impl_json_arrow!(typed AccountState, "AccountState", ACCOUNT_STATE_FIELDS, &["info"]);

#[cfg(test)]
mod tests {
    use nautilus_core::Params;
    use nautilus_model::events::account::stubs::cash_account_state;
    use rstest::rstest;
    use serde_json::json;

    use super::*;
    use crate::arrow::{DecodeTypedFromRecordBatch, EncodeToRecordBatch, json::encode_batch};

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
