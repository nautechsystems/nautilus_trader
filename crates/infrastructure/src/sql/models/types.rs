// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use bytes::Bytes;
use indexmap::IndexMap;
use nautilus_common::{custom::CustomData, signal::Signal};
use nautilus_core::UnixNanos;
use nautilus_model::{data::DataType, types::Currency};
use sqlx::{FromRow, Row, postgres::PgRow};
use ustr::Ustr;

use crate::sql::models::enums::CurrencyTypeModel;

pub struct CurrencyModel(pub Currency);
pub struct SignalModel(pub Signal);
pub struct CustomDataModel(pub CustomData);

impl<'r> FromRow<'r, PgRow> for CurrencyModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let id = row.try_get::<String, _>("id")?;
        let precision = row.try_get::<i32, _>("precision")?;
        let iso4217 = row.try_get::<i32, _>("iso4217")?;
        let name = row.try_get::<String, _>("name")?;
        let currency_type_model = row.try_get::<CurrencyTypeModel, _>("currency_type")?;
        let currency = Currency::new(
            id.as_str(),
            precision as u8,
            iso4217 as u16,
            name.as_str(),
            currency_type_model.0,
        );
        Ok(CurrencyModel(currency))
    }
}

impl<'r> FromRow<'r, PgRow> for SignalModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let name = row.try_get::<&str, _>("name").map(Ustr::from)?;
        let value = row.try_get::<String, _>("value")?;
        let ts_event = row.try_get::<&str, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<&str, _>("ts_init").map(UnixNanos::from)?;
        let signal = Signal::new(name, value, ts_event, ts_init);
        Ok(SignalModel(signal))
    }
}

impl<'r> FromRow<'r, PgRow> for CustomDataModel {
    fn from_row(row: &'r PgRow) -> Result<Self, sqlx::Error> {
        let type_name = row.try_get::<&str, _>("data_type")?;
        let metadata_json: Option<serde_json::Value> =
            row.try_get::<Option<serde_json::Value>, _>("metadata")?;
        let metadata: Option<IndexMap<String, String>> = match metadata_json {
            Some(json_value) => serde_json::from_value(json_value).unwrap_or(None), // Handle deserialization
            None => None,
        };
        let data_type = DataType::new(type_name, metadata);
        let value = row.try_get::<Vec<u8>, _>("value").map(Bytes::from)?;
        let ts_event = row.try_get::<&str, _>("ts_event").map(UnixNanos::from)?;
        let ts_init = row.try_get::<&str, _>("ts_init").map(UnixNanos::from)?;
        let custom = CustomData::new(data_type, value, ts_event, ts_init);
        Ok(CustomDataModel(custom))
    }
}
