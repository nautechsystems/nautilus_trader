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

//! Display-mode Arrow encoder for [`AccountState`].

use std::sync::Arc;

use arrow::{
    array::{BooleanBuilder, StringBuilder, TimestampNanosecondBuilder},
    datatypes::Schema,
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::events::AccountState;

use super::{bool_field, timestamp_field, unix_nanos_to_i64, utf8_field};

/// Returns the display-mode Arrow schema for [`AccountState`].
#[must_use]
pub fn account_state_schema() -> Schema {
    Schema::new(vec![
        utf8_field("account_id", false),
        utf8_field("account_type", false),
        utf8_field("base_currency", true),
        utf8_field("balances", false),
        utf8_field("margins", false),
        bool_field("is_reported", false),
        utf8_field("event_id", false),
        timestamp_field("ts_event", false),
        timestamp_field("ts_init", false),
    ])
}

fn balances_to_json(state: &AccountState) -> String {
    let entries: Vec<serde_json::Value> = state
        .balances
        .iter()
        .map(|b| {
            serde_json::json!({
                "currency": b.currency.to_string(),
                "total": b.total.as_f64(),
                "locked": b.locked.as_f64(),
                "free": b.free.as_f64(),
            })
        })
        .collect();
    serde_json::to_string(&entries).unwrap_or_default()
}

fn margins_to_json(state: &AccountState) -> String {
    let entries: Vec<serde_json::Value> = state
        .margins
        .iter()
        .map(|m| {
            serde_json::json!({
                "instrument_id": m.instrument_id.map(|id| id.to_string()),
                "currency": m.currency.to_string(),
                "initial": m.initial.as_f64(),
                "maintenance": m.maintenance.as_f64(),
            })
        })
        .collect();
    serde_json::to_string(&entries).unwrap_or_default()
}

/// Encodes account state snapshots as a display-friendly Arrow [`RecordBatch`].
///
/// Emits `Utf8` columns for identifiers and JSON-serialized balances/margins,
/// `Timestamp(Nanosecond)` columns for event and init times, and a `Boolean`
/// column for `is_reported`. Balances and margins are serialized as JSON arrays
/// with `f64` amounts for display readability.
///
/// Returns an empty [`RecordBatch`] with the correct schema when `data` is empty.
///
/// # Errors
///
/// Returns an [`ArrowError`] if the Arrow `RecordBatch` cannot be constructed.
pub fn encode_account_states(data: &[AccountState]) -> Result<RecordBatch, ArrowError> {
    let mut account_id = StringBuilder::new();
    let mut account_type = StringBuilder::new();
    let mut base_currency = StringBuilder::new();
    let mut balances = StringBuilder::new();
    let mut margins = StringBuilder::new();
    let mut is_reported = BooleanBuilder::with_capacity(data.len());
    let mut event_id = StringBuilder::new();
    let mut ts_event = TimestampNanosecondBuilder::with_capacity(data.len());
    let mut ts_init = TimestampNanosecondBuilder::with_capacity(data.len());

    for state in data {
        account_id.append_value(state.account_id);
        account_type.append_value(format!("{}", state.account_type));
        base_currency.append_option(state.base_currency.map(|v| v.to_string()));
        balances.append_value(balances_to_json(state));
        margins.append_value(margins_to_json(state));
        is_reported.append_value(state.is_reported);
        event_id.append_value(state.event_id.to_string());
        ts_event.append_value(unix_nanos_to_i64(state.ts_event.as_u64()));
        ts_init.append_value(unix_nanos_to_i64(state.ts_init.as_u64()));
    }

    RecordBatch::try_new(
        Arc::new(account_state_schema()),
        vec![
            Arc::new(account_id.finish()),
            Arc::new(account_type.finish()),
            Arc::new(base_currency.finish()),
            Arc::new(balances.finish()),
            Arc::new(margins.finish()),
            Arc::new(is_reported.finish()),
            Arc::new(event_id.finish()),
            Arc::new(ts_event.finish()),
            Arc::new(ts_init.finish()),
        ],
    )
}

#[cfg(test)]
mod tests {
    use arrow::{
        array::{Array, BooleanArray, StringArray, TimestampNanosecondArray},
        datatypes::{DataType, TimeUnit},
    };
    use nautilus_core::UUID4;
    use nautilus_model::{
        enums::AccountType,
        identifiers::AccountId,
        types::{AccountBalance, Currency, Money},
    };
    use rstest::rstest;

    use super::*;

    fn make_account_state(ts: u64) -> AccountState {
        let currency = Currency::USD();
        let balance = AccountBalance::new(
            Money::new(10_000.0, currency),
            Money::new(1_000.0, currency),
            Money::new(9_000.0, currency),
        );
        AccountState {
            account_id: AccountId::from("SIM-001"),
            account_type: AccountType::Cash,
            base_currency: Some(currency),
            balances: vec![balance],
            margins: vec![],
            is_reported: false,
            event_id: UUID4::default(),
            ts_event: ts.into(),
            ts_init: (ts + 1).into(),
        }
    }

    #[rstest]
    fn test_encode_account_states_schema() {
        let batch = encode_account_states(&[]).unwrap();
        let schema = batch.schema();
        let fields = schema.fields();
        assert_eq!(fields.len(), 9);
        assert_eq!(fields[0].name(), "account_id");
        assert_eq!(fields[0].data_type(), &DataType::Utf8);
        assert_eq!(fields[5].name(), "is_reported");
        assert_eq!(fields[5].data_type(), &DataType::Boolean);
        assert_eq!(fields[7].name(), "ts_event");
        assert_eq!(
            fields[7].data_type(),
            &DataType::Timestamp(TimeUnit::Nanosecond, None)
        );
    }

    #[rstest]
    fn test_encode_account_states_values() {
        let states = vec![make_account_state(1_000_000)];
        let batch = encode_account_states(&states).unwrap();

        assert_eq!(batch.num_rows(), 1);

        let account_id_col = batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let is_reported_col = batch
            .column(5)
            .as_any()
            .downcast_ref::<BooleanArray>()
            .unwrap();
        let ts_event_col = batch
            .column(7)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let balances_col = batch
            .column(3)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();

        assert_eq!(account_id_col.value(0), "SIM-001");
        assert!(!is_reported_col.value(0));
        assert_eq!(ts_event_col.value(0), 1_000_000);

        let balances: Vec<serde_json::Value> = serde_json::from_str(balances_col.value(0)).unwrap();
        assert_eq!(balances.len(), 1);
        assert_eq!(balances[0]["currency"], "USD");
        assert!((balances[0]["total"].as_f64().unwrap() - 10_000.0).abs() < 1e-9);
        assert!((balances[0]["locked"].as_f64().unwrap() - 1_000.0).abs() < 1e-9);
        assert!((balances[0]["free"].as_f64().unwrap() - 9_000.0).abs() < 1e-9);
    }

    #[rstest]
    fn test_encode_account_states_empty() {
        let batch = encode_account_states(&[]).unwrap();
        assert_eq!(batch.num_rows(), 0);
        assert_eq!(batch.schema().fields().len(), 9);
    }

    #[rstest]
    fn test_encode_account_states_null_base_currency() {
        let mut state = make_account_state(1_000);
        state.base_currency = None;
        let batch = encode_account_states(&[state]).unwrap();

        let base_currency_col = batch
            .column(2)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        assert!(base_currency_col.is_null(0));
    }
}
