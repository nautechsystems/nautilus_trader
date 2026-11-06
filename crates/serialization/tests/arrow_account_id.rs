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

#![cfg(feature = "arrow")]

use std::sync::Arc;

use arrow::{array::StringArray, datatypes::DataType as ArrowDataType, record_batch::RecordBatch};
use nautilus_core::UnixNanos;
use nautilus_model::{
    custom_data,
    data::{Data, DataType},
    identifiers::AccountId,
};
use nautilus_serialization::{
    arrow::{DecodeDataFromRecordBatch, EncodeToRecordBatch, EncodingError},
    arrow_custom_data,
};
use rstest::rstest;

#[arrow_custom_data]
#[custom_data]
struct AccountData {
    account_id: AccountId,
    optional_account_id: Option<AccountId>,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
}

#[rstest]
#[case::empty(vec![])]
#[case::single(vec![Some("OPTIONAL-001")])]
#[case::runs_with_nulls(vec![Some("OPTIONAL-001"), Some("OPTIONAL-001"), None, Some("OPTIONAL-001"), Some("OPTIONAL-002"), None, Some("OPTIONAL-002")])]
#[case::alternating(vec![Some("OPTIONAL-001"), Some("OPTIONAL-002"), Some("OPTIONAL-001"), Some("OPTIONAL-002")])]
fn account_ids_round_trip(#[case] optional: Vec<Option<&str>>) {
    let original: Vec<_> = optional
        .into_iter()
        .enumerate()
        .map(|(i, account)| account_data(i, account))
        .collect();
    let batch = AccountData::encode_batch(&Default::default(), &original).unwrap();
    let decoded = AccountData::decode_data_batch(batch.schema().metadata(), batch.clone()).unwrap();
    let expected_type = serde_json::to_value(DataType::new("AccountData", None, None)).unwrap();

    assert_eq!(batch.schema().field(0).data_type(), &ArrowDataType::Utf8);
    assert!(!batch.schema().field(0).is_nullable());
    assert_eq!(batch.schema().field(1).data_type(), &ArrowDataType::Utf8);
    assert!(batch.schema().field(1).is_nullable());
    assert_eq!(decoded.len(), original.len());

    for (actual, expected) in decoded.iter().zip(&original) {
        let Data::Custom(custom) = actual else {
            panic!("expected custom data");
        };

        assert_eq!(
            custom.data.as_any().downcast_ref::<AccountData>().unwrap(),
            expected
        );
        assert_eq!(
            serde_json::to_value(&custom.data_type).unwrap(),
            expected_type
        );
    }
}

#[rstest]
#[case(0, "account_id")]
#[case(1, "optional_account_id")]
fn invalid_account_id_returns_parse_error(
    #[case] index: usize,
    #[case] name: &'static str,
    #[values("MISSING_SEPARATOR", "BROKER-")] invalid: &str,
) {
    let original = [
        account_data(0, Some("OPTIONAL-001")),
        account_data(1, None),
        account_data(2, Some("OPTIONAL-002")),
    ];

    let batch = AccountData::encode_batch(&Default::default(), &original).unwrap();
    let values = batch
        .column(index)
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    let mut values: Vec<_> = values.iter().collect();
    values[2] = Some(invalid);
    let mut columns = batch.columns().to_vec();
    columns[index] = Arc::new(StringArray::from(values));
    let batch = RecordBatch::try_new(batch.schema(), columns).unwrap();
    let expected = AccountId::new_checked(invalid).unwrap_err();
    let error = AccountData::decode_data_batch(batch.schema().metadata(), batch).unwrap_err();

    assert_eq!(
        error.to_string(),
        EncodingError::ParseError(
            name,
            format!("expected valid identifier/type, was '{expected}'")
        )
        .to_string()
    );
}

fn account_data(i: usize, optional: Option<&str>) -> AccountData {
    AccountData {
        account_id: AccountId::from(format!("REQUIRED-{:03}", i % 2 + 1)),
        optional_account_id: optional.map(AccountId::from),
        ts_event: (i as u64 * 10 + 1).into(),
        ts_init: (i as u64 * 10 + 3).into(),
    }
}
