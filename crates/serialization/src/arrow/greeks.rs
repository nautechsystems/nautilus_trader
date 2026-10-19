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

//! Native Arrow records for calculated Greeks and yield curves.

use std::{borrow::Borrow, collections::HashMap, sync::Arc};

use arrow::{
    array::{
        Array, ArrayRef, BooleanArray, Float64Array, Float64Builder, Int32Array, ListArray,
        ListBuilder, StringArray, UInt64Array,
    },
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{
        CustomData, CustomDataTrait, Data,
        greeks::{GreeksData, OptionGreekValues, PortfolioGreeks, YieldCurve},
    },
    identifiers::InstrumentId,
};

use super::{
    ArrowSchemaProvider, DecodeDataFromRecordBatch, DecodeTypedFromRecordBatch,
    EncodeToRecordBatch, EncodingError, KEY_INSTRUMENT_ID, StringColumnRef, extract_column_by_name,
    extract_column_string, identifier_array_from_display, metadata_with_type_name,
    record_batch_with_timestamps, record_batch_with_u64_timestamps, timestamp_data_type,
};

impl ArrowSchemaProvider for GreeksData {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let mut fields = vec![
            Field::new("ts_init", timestamp_data_type(), false),
            Field::new("ts_event", timestamp_data_type(), false),
            Field::new("instrument_id", DataType::Utf8, false),
            Field::new("is_call", DataType::Boolean, false),
            Field::new("strike", DataType::Float64, false),
            Field::new("expiry", DataType::Int32, false),
            Field::new("expiry_in_days", DataType::Int32, false),
            Field::new("expiry_in_years", DataType::Float64, false),
            Field::new("multiplier", DataType::Float64, false),
            Field::new("quantity", DataType::Float64, false),
            Field::new("underlying_price", DataType::Float64, false),
            Field::new("interest_rate", DataType::Float64, false),
            Field::new("cost_of_carry", DataType::Float64, false),
            Field::new("vol", DataType::Float64, false),
            Field::new("pnl", DataType::Float64, false),
            Field::new("price", DataType::Float64, false),
        ];

        fields.extend(greek_fields());
        fields.push(Field::new("itm_prob", DataType::Float64, false));
        Schema::new_with_metadata(fields, metadata_with_type_name("GreeksData", metadata))
    }
}

impl EncodeToRecordBatch for GreeksData {
    fn encode_batch<T: Borrow<Self>>(
        metadata: &HashMap<String, String>,
        data: &[T],
    ) -> Result<RecordBatch, ArrowError> {
        let mut columns: Vec<ArrayRef> = vec![
            Arc::new(UInt64Array::from_iter_values(
                data.iter().map(|value| value.borrow().ts_init.as_u64()),
            )),
            Arc::new(UInt64Array::from_iter_values(
                data.iter().map(|value| value.borrow().ts_event.as_u64()),
            )),
            Arc::new(identifier_array_from_display(
                data.iter().map(|value| value.borrow().instrument_id),
            )),
            Arc::new(BooleanArray::from(
                data.iter()
                    .map(|value| value.borrow().is_call)
                    .collect::<Vec<_>>(),
            )),
            Arc::new(Float64Array::from_iter_values(
                data.iter().map(|value| value.borrow().strike),
            )),
            Arc::new(Int32Array::from_iter_values(
                data.iter().map(|value| value.borrow().expiry),
            )),
            Arc::new(Int32Array::from_iter_values(
                data.iter().map(|value| value.borrow().expiry_in_days),
            )),
            Arc::new(Float64Array::from_iter_values(
                data.iter().map(|value| value.borrow().expiry_in_years),
            )),
            Arc::new(Float64Array::from_iter_values(
                data.iter().map(|value| value.borrow().multiplier),
            )),
            Arc::new(Float64Array::from_iter_values(
                data.iter().map(|value| value.borrow().quantity),
            )),
            Arc::new(Float64Array::from_iter_values(
                data.iter().map(|value| value.borrow().underlying_price),
            )),
            Arc::new(Float64Array::from_iter_values(
                data.iter().map(|value| value.borrow().interest_rate),
            )),
            Arc::new(Float64Array::from_iter_values(
                data.iter().map(|value| value.borrow().cost_of_carry),
            )),
            Arc::new(Float64Array::from_iter_values(
                data.iter().map(|value| value.borrow().vol),
            )),
            Arc::new(Float64Array::from_iter_values(
                data.iter().map(|value| value.borrow().pnl),
            )),
            Arc::new(Float64Array::from_iter_values(
                data.iter().map(|value| value.borrow().price),
            )),
        ];

        columns.extend(greek_columns(
            data.iter().map(|value| value.borrow().greeks),
        ));
        columns.push(Arc::new(Float64Array::from_iter_values(
            data.iter().map(|value| value.borrow().itm_prob),
        )));
        record_batch_with_timestamps(Arc::new(Self::get_schema(Some(metadata.clone()))), columns)
    }

    fn metadata(&self) -> HashMap<String, String> {
        let mut metadata = metadata_with_type_name("GreeksData", None);
        metadata.insert(
            KEY_INSTRUMENT_ID.to_string(),
            self.instrument_id.to_string(),
        );
        metadata
    }
}

impl DecodeTypedFromRecordBatch for GreeksData {
    fn decode_typed_batch(
        _metadata: &HashMap<String, String>,
        batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let batch = record_batch_with_u64_timestamps(&batch)?;
        let ts_init = required_column::<UInt64Array>(&batch, "ts_init", DataType::UInt64)?;
        let ts_event = required_column::<UInt64Array>(&batch, "ts_event", DataType::UInt64)?;
        let instrument_id = required_string_column(&batch, "instrument_id")?;
        let is_call = required_column::<BooleanArray>(&batch, "is_call", DataType::Boolean)?;
        let strike = required_column::<Float64Array>(&batch, "strike", DataType::Float64)?;
        let expiry = required_column::<Int32Array>(&batch, "expiry", DataType::Int32)?;
        let expiry_in_days =
            required_column::<Int32Array>(&batch, "expiry_in_days", DataType::Int32)?;
        let expiry_in_years =
            required_column::<Float64Array>(&batch, "expiry_in_years", DataType::Float64)?;
        let multiplier = required_column::<Float64Array>(&batch, "multiplier", DataType::Float64)?;
        let quantity = required_column::<Float64Array>(&batch, "quantity", DataType::Float64)?;
        let underlying_price =
            required_column::<Float64Array>(&batch, "underlying_price", DataType::Float64)?;
        let interest_rate =
            required_column::<Float64Array>(&batch, "interest_rate", DataType::Float64)?;
        let cost_of_carry =
            required_column::<Float64Array>(&batch, "cost_of_carry", DataType::Float64)?;
        let vol = required_column::<Float64Array>(&batch, "vol", DataType::Float64)?;
        let pnl = required_column::<Float64Array>(&batch, "pnl", DataType::Float64)?;
        let price = required_column::<Float64Array>(&batch, "price", DataType::Float64)?;
        let itm_prob = required_column::<Float64Array>(&batch, "itm_prob", DataType::Float64)?;
        let greeks = decode_greeks(&batch)?;

        (0..batch.num_rows())
            .map(|row| {
                Ok(Self {
                    instrument_id: instrument_id
                        .value(row)
                        .parse::<InstrumentId>()
                        .map_err(|e| EncodingError::ParseError("instrument_id", e.to_string()))?,
                    is_call: is_call.value(row),
                    strike: strike.value(row),
                    expiry: expiry.value(row),
                    expiry_in_days: expiry_in_days.value(row),
                    expiry_in_years: expiry_in_years.value(row),
                    multiplier: multiplier.value(row),
                    quantity: quantity.value(row),
                    underlying_price: underlying_price.value(row),
                    interest_rate: interest_rate.value(row),
                    cost_of_carry: cost_of_carry.value(row),
                    vol: vol.value(row),
                    pnl: pnl.value(row),
                    price: price.value(row),
                    greeks: greeks[row],
                    itm_prob: itm_prob.value(row),
                    ts_event: ts_event.value(row).into(),
                    ts_init: ts_init.value(row).into(),
                })
            })
            .collect()
    }
}

impl ArrowSchemaProvider for PortfolioGreeks {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let mut fields = vec![
            Field::new("ts_init", timestamp_data_type(), false),
            Field::new("ts_event", timestamp_data_type(), false),
            Field::new("pnl", DataType::Float64, false),
            Field::new("price", DataType::Float64, false),
        ];

        fields.extend(greek_fields());
        Schema::new_with_metadata(fields, metadata_with_type_name("PortfolioGreeks", metadata))
    }
}

impl EncodeToRecordBatch for PortfolioGreeks {
    fn encode_batch<T: Borrow<Self>>(
        metadata: &HashMap<String, String>,
        data: &[T],
    ) -> Result<RecordBatch, ArrowError> {
        let mut columns: Vec<ArrayRef> = vec![
            Arc::new(UInt64Array::from_iter_values(
                data.iter().map(|value| value.borrow().ts_init.as_u64()),
            )),
            Arc::new(UInt64Array::from_iter_values(
                data.iter().map(|value| value.borrow().ts_event.as_u64()),
            )),
            Arc::new(Float64Array::from_iter_values(
                data.iter().map(|value| value.borrow().pnl),
            )),
            Arc::new(Float64Array::from_iter_values(
                data.iter().map(|value| value.borrow().price),
            )),
        ];

        columns.extend(greek_columns(
            data.iter().map(|value| value.borrow().greeks),
        ));
        record_batch_with_timestamps(Arc::new(Self::get_schema(Some(metadata.clone()))), columns)
    }

    fn metadata(&self) -> HashMap<String, String> {
        metadata_with_type_name("PortfolioGreeks", None)
    }
}

impl DecodeTypedFromRecordBatch for PortfolioGreeks {
    fn decode_typed_batch(
        _metadata: &HashMap<String, String>,
        batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let batch = record_batch_with_u64_timestamps(&batch)?;
        let ts_init = required_column::<UInt64Array>(&batch, "ts_init", DataType::UInt64)?;
        let ts_event = required_column::<UInt64Array>(&batch, "ts_event", DataType::UInt64)?;
        let pnl = required_column::<Float64Array>(&batch, "pnl", DataType::Float64)?;
        let price = required_column::<Float64Array>(&batch, "price", DataType::Float64)?;
        let greeks = decode_greeks(&batch)?;

        (0..batch.num_rows())
            .map(|row| {
                Ok(Self {
                    pnl: pnl.value(row),
                    price: price.value(row),
                    greeks: greeks[row],
                    ts_event: ts_event.value(row).into(),
                    ts_init: ts_init.value(row).into(),
                })
            })
            .collect()
    }
}

impl ArrowSchemaProvider for YieldCurve {
    fn get_schema(metadata: Option<HashMap<String, String>>) -> Schema {
        let fields = vec![
            Field::new("ts_init", timestamp_data_type(), false),
            Field::new("ts_event", timestamp_data_type(), false),
            Field::new("curve_name", DataType::Utf8, false),
            Field::new("tenors", float_list_data_type(), false),
            Field::new("interest_rates", float_list_data_type(), false),
        ];
        Schema::new_with_metadata(
            fields,
            metadata_with_type_name(Self::type_name_static(), metadata),
        )
    }
}

impl EncodeToRecordBatch for YieldCurve {
    fn encode_batch<T: Borrow<Self>>(
        metadata: &HashMap<String, String>,
        data: &[T],
    ) -> Result<RecordBatch, ArrowError> {
        let columns: Vec<ArrayRef> = vec![
            Arc::new(UInt64Array::from_iter_values(
                data.iter().map(|value| value.borrow().ts_init.as_u64()),
            )),
            Arc::new(UInt64Array::from_iter_values(
                data.iter().map(|value| value.borrow().ts_event.as_u64()),
            )),
            Arc::new(StringArray::from_iter_values(
                data.iter().map(|value| value.borrow().curve_name.as_str()),
            )),
            Arc::new(float_lists(
                data.iter().map(|value| value.borrow().tenors.as_slice()),
            )),
            Arc::new(float_lists(
                data.iter()
                    .map(|value| value.borrow().interest_rates.as_slice()),
            )),
        ];
        record_batch_with_timestamps(Arc::new(Self::get_schema(Some(metadata.clone()))), columns)
    }

    fn metadata(&self) -> HashMap<String, String> {
        metadata_with_type_name(Self::type_name_static(), None)
    }
}

impl DecodeTypedFromRecordBatch for YieldCurve {
    fn decode_typed_batch(
        _metadata: &HashMap<String, String>,
        batch: RecordBatch,
    ) -> Result<Vec<Self>, EncodingError> {
        let batch = record_batch_with_u64_timestamps(&batch)?;
        let ts_init = required_column::<UInt64Array>(&batch, "ts_init", DataType::UInt64)?;
        let ts_event = required_column::<UInt64Array>(&batch, "ts_event", DataType::UInt64)?;
        let curve_name = required_string_column(&batch, "curve_name")?;
        let tenors = required_column::<ListArray>(&batch, "tenors", float_list_data_type())?;
        let interest_rates =
            required_column::<ListArray>(&batch, "interest_rates", float_list_data_type())?;
        (0..batch.num_rows())
            .map(|row| {
                Ok(Self {
                    curve_name: curve_name.value(row).to_string(),
                    tenors: decode_float_list(tenors, "tenors", row)?,
                    interest_rates: decode_float_list(interest_rates, "interest_rates", row)?,
                    ts_event: ts_event.value(row).into(),
                    ts_init: ts_init.value(row).into(),
                })
            })
            .collect()
    }
}

impl DecodeDataFromRecordBatch for YieldCurve {
    fn decode_data_batch(
        metadata: &HashMap<String, String>,
        record_batch: RecordBatch,
    ) -> Result<Vec<Data>, EncodingError> {
        Ok(Self::decode_typed_batch(metadata, record_batch)?
            .into_iter()
            .map(|curve| Data::Custom(CustomData::from_arc(Arc::new(curve))))
            .collect())
    }
}

fn greek_fields() -> Vec<Field> {
    ["delta", "gamma", "vega", "theta", "rho"]
        .into_iter()
        .map(|name| Field::new(name, DataType::Float64, false))
        .collect()
}

fn greek_columns(values: impl Iterator<Item = OptionGreekValues> + Clone) -> Vec<ArrayRef> {
    vec![
        Arc::new(Float64Array::from_iter_values(
            values.clone().map(|value| value.delta),
        )),
        Arc::new(Float64Array::from_iter_values(
            values.clone().map(|value| value.gamma),
        )),
        Arc::new(Float64Array::from_iter_values(
            values.clone().map(|value| value.vega),
        )),
        Arc::new(Float64Array::from_iter_values(
            values.clone().map(|value| value.theta),
        )),
        Arc::new(Float64Array::from_iter_values(
            values.map(|value| value.rho),
        )),
    ]
}

fn decode_greeks(batch: &RecordBatch) -> Result<Vec<OptionGreekValues>, EncodingError> {
    let delta = required_column::<Float64Array>(batch, "delta", DataType::Float64)?;
    let gamma = required_column::<Float64Array>(batch, "gamma", DataType::Float64)?;
    let vega = required_column::<Float64Array>(batch, "vega", DataType::Float64)?;
    let theta = required_column::<Float64Array>(batch, "theta", DataType::Float64)?;
    let rho = required_column::<Float64Array>(batch, "rho", DataType::Float64)?;
    Ok((0..batch.num_rows())
        .map(|row| OptionGreekValues {
            delta: delta.value(row),
            gamma: gamma.value(row),
            vega: vega.value(row),
            theta: theta.value(row),
            rho: rho.value(row),
        })
        .collect())
}

fn required_column<'a, A: Array + 'static>(
    batch: &'a RecordBatch,
    name: &'static str,
    data_type: DataType,
) -> Result<&'a A, EncodingError> {
    let column = extract_column_by_name::<A>(batch, name, data_type)?;

    if column.null_count() != 0 {
        return Err(EncodingError::ParseError(
            name,
            "required column contains nulls".to_string(),
        ));
    }

    Ok(column)
}

fn required_string_column<'a>(
    batch: &'a RecordBatch,
    name: &'static str,
) -> Result<StringColumnRef<'a>, EncodingError> {
    let index = batch.schema().index_of(name)?;
    let column = extract_column_string(batch.columns(), name, index)?;

    if batch.column(index).logical_null_count() != 0 {
        return Err(EncodingError::ParseError(
            name,
            "required column contains nulls".to_string(),
        ));
    }

    Ok(column)
}

fn float_list_data_type() -> DataType {
    DataType::List(Arc::new(Field::new("item", DataType::Float64, true)))
}

fn float_lists<'a>(values: impl Iterator<Item = &'a [f64]>) -> ListArray {
    let mut builder = ListBuilder::new(Float64Builder::new());
    for values in values {
        builder.values().append_slice(values);
        builder.append(true);
    }

    builder.finish()
}

fn decode_float_list(
    values: &ListArray,
    name: &'static str,
    row: usize,
) -> Result<Vec<f64>, EncodingError> {
    let values = values.value(row);

    let values = values
        .as_any()
        .downcast_ref::<Float64Array>()
        .ok_or_else(|| {
            EncodingError::ParseError(name, "expected Float64 list values".to_string())
        })?;

    if values.null_count() != 0 {
        return Err(EncodingError::ParseError(
            name,
            format!("row {row}: list contains nulls"),
        ));
    }

    Ok(values.values().to_vec())
}

#[cfg(test)]
mod tests {
    use nautilus_model::data::{decode_custom_from_arrow, encode_custom_to_arrow};
    use rstest::rstest;

    use super::*;

    fn sensitivities() -> OptionGreekValues {
        OptionGreekValues {
            delta: 0.41,
            gamma: 0.0012,
            vega: 11.2,
            theta: -2.3,
            rho: 1.7,
        }
    }

    fn assert_sensitivities(actual: OptionGreekValues, expected: OptionGreekValues) {
        assert_eq!(actual.delta.to_bits(), expected.delta.to_bits());
        assert_eq!(actual.gamma.to_bits(), expected.gamma.to_bits());
        assert_eq!(actual.vega.to_bits(), expected.vega.to_bits());
        assert_eq!(actual.theta.to_bits(), expected.theta.to_bits());
        assert_eq!(actual.rho.to_bits(), expected.rho.to_bits());
    }

    #[rstest]
    fn test_greeks_data_round_trip() {
        let first = GreeksData::new(
            "ESZ26 C7000.XCME".parse().unwrap(),
            false,
            7000.5,
            20_261_218,
            -68,
            0.186,
            50.0,
            -3.0,
            6950.25,
            0.0425,
            0.0125,
            0.175,
            12.5,
            98.75,
            sensitivities(),
            0.62,
            11.into(),
            13.into(),
        );
        let mut second = first.clone();
        second.instrument_id = "ESZ26 P6900.XCME".parse().unwrap();
        second.is_call = true;
        second.ts_init = 31.into();
        second.ts_event = 29.into();
        second.greeks.rho = -0.0;
        second.greeks.delta = f64::from_bits(0x3fe0_0000_0000_0001);
        let values = [first, second];
        let batch = GreeksData::encode_batch(&values[0].metadata(), &values).unwrap();
        let decoded =
            GreeksData::decode_typed_batch(batch.schema().metadata(), batch.clone()).unwrap();

        assert_eq!(
            batch
                .schema()
                .field_with_name("expiry")
                .unwrap()
                .data_type(),
            &DataType::Int32
        );
        assert_eq!(
            batch.schema().field_with_name("rho").unwrap().data_type(),
            &DataType::Float64
        );
        assert_eq!(decoded.len(), values.len());

        for (actual, expected) in decoded.iter().zip(&values) {
            assert_eq!(actual.instrument_id, expected.instrument_id);
            assert_eq!(actual.is_call, expected.is_call);
            assert_eq!(actual.expiry, expected.expiry);
            assert_eq!(actual.expiry_in_days, expected.expiry_in_days);
            assert_eq!(actual.ts_init, expected.ts_init);
            assert_eq!(actual.ts_event, expected.ts_event);
            macro_rules! assert_float { ($($field:ident),+) => { $(assert_eq!(actual.$field.to_bits(), expected.$field.to_bits(), stringify!($field));)+ }; }
            assert_float!(
                strike,
                expiry_in_years,
                multiplier,
                quantity,
                underlying_price,
                interest_rate,
                cost_of_carry,
                vol,
                pnl,
                price,
                itm_prob
            );
            assert_sensitivities(actual.greeks, expected.greeks);
        }
    }

    #[rstest]
    fn test_portfolio_greeks_round_trip() {
        let values = [PortfolioGreeks {
            pnl: -125.5,
            price: 4310.25,
            greeks: sensitivities(),
            ts_event: 19.into(),
            ts_init: 17.into(),
        }];

        let batch = PortfolioGreeks::encode_batch(&values[0].metadata(), &values).unwrap();
        let decoded =
            PortfolioGreeks::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded.len(), values.len());
        assert_eq!(decoded[0].ts_init, values[0].ts_init);
        assert_eq!(decoded[0].ts_event, values[0].ts_event);
        assert_eq!(decoded[0].pnl.to_bits(), values[0].pnl.to_bits());
        assert_eq!(decoded[0].price.to_bits(), values[0].price.to_bits());
        assert_sensitivities(decoded[0].greeks, values[0].greeks);
    }

    #[rstest]
    fn test_yield_curve_registered_custom_data_round_trip() {
        let curve = YieldCurve::new(
            "USD-SOFR".to_string(),
            vec![0.25, 1.0, 5.0],
            vec![0.043, 0.041, 0.038],
            23.into(),
            29.into(),
        );
        let item: Arc<dyn CustomDataTrait> = Arc::new(curve.clone());

        crate::ensure_custom_data_registered::<YieldCurve>();
        let batch = encode_custom_to_arrow("YieldCurve", &[item])
            .unwrap()
            .unwrap();
        let metadata = batch.schema().metadata().clone();
        let decoded = decode_custom_from_arrow("YieldCurve", &metadata, batch)
            .unwrap()
            .unwrap();

        let [Data::Custom(custom)] = decoded.as_slice() else {
            panic!("expected one custom data row, was {decoded:?}");
        };

        assert_eq!(custom.data_type.type_name(), "YieldCurve");
        assert_eq!(
            custom.data.as_any().downcast_ref::<YieldCurve>(),
            Some(&curve)
        );
    }

    #[rstest]
    #[case(vec![0.25, 1.0, 5.0], vec![0.043, 0.041, 0.038])]
    #[case(vec![], vec![-0.0, f64::from_bits(0x3fe0_0000_0000_0001)])]
    fn test_yield_curve_round_trip(#[case] tenors: Vec<f64>, #[case] interest_rates: Vec<f64>) {
        let curve = YieldCurve::new(
            "USD-SOFR".to_string(),
            tenors,
            interest_rates,
            23.into(),
            29.into(),
        );
        let batch =
            YieldCurve::encode_batch(&curve.metadata(), std::slice::from_ref(&curve)).unwrap();
        let decoded = YieldCurve::decode_typed_batch(batch.schema().metadata(), batch).unwrap();

        assert_eq!(decoded.len(), 1);
        assert_eq!(decoded[0].ts_init, curve.ts_init);
        assert_eq!(decoded[0].ts_event, curve.ts_event);
        assert_eq!(decoded[0].curve_name, curve.curve_name);
        assert_eq!(
            decoded[0]
                .tenors
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            curve.tenors.iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
        assert_eq!(
            decoded[0]
                .interest_rates
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            curve
                .interest_rates
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>()
        );
    }

    #[rstest]
    fn test_native_greeks_empty_batches() {
        macro_rules! assert_empty {
            ($type:ty) => {{
                let batch = <$type>::encode_batch(&HashMap::new(), &Vec::<$type>::new()).unwrap();
                let decoded =
                    <$type>::decode_typed_batch(batch.schema().metadata(), batch).unwrap();
                assert!(decoded.is_empty());
            }};
        }
        assert_empty!(GreeksData);
        assert_empty!(PortfolioGreeks);
        assert_empty!(YieldCurve);
    }

    #[rstest]
    #[case("pnl")]
    #[case("rho")]
    fn test_portfolio_greeks_rejects_null_values(#[case] name: &'static str) {
        let value = PortfolioGreeks {
            pnl: -125.5,
            price: 4310.25,
            greeks: sensitivities(),
            ts_event: 19.into(),
            ts_init: 17.into(),
        };

        let batch = PortfolioGreeks::encode_batch(&value.metadata(), &[value]).unwrap();
        let index = batch.schema().index_of(name).unwrap();
        let mut fields = batch.schema().fields().to_vec();
        fields[index] = Arc::new(fields[index].as_ref().clone().with_nullable(true));
        let mut columns = batch.columns().to_vec();
        columns[index] = Arc::new(Float64Array::from(vec![None::<f64>]));
        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap();
        let error = PortfolioGreeks::decode_typed_batch(&HashMap::new(), batch).unwrap_err();

        assert!(
            matches!(error, EncodingError::ParseError(field, ref message)
            if field == name && message == "required column contains nulls")
        );
    }

    #[rstest]
    #[case(Some("USD-SOFR"))]
    #[case(None)]
    fn test_yield_curve_dictionary_name(#[case] name: Option<&str>) {
        use arrow::{
            array::{DictionaryArray, Int8Array},
            datatypes::Int8Type,
        };

        let value = YieldCurve::new(
            "USD-SOFR".to_string(),
            vec![0.25],
            vec![0.043],
            23.into(),
            29.into(),
        );
        let batch = YieldCurve::encode_batch(&value.metadata(), &[value]).unwrap();
        let index = batch.schema().index_of("curve_name").unwrap();
        let names = DictionaryArray::<Int8Type>::try_new(
            Int8Array::from(vec![0]),
            Arc::new(StringArray::from(vec![name])),
        )
        .unwrap();
        let mut fields = batch.schema().fields().to_vec();
        fields[index] = Arc::new(Field::new("curve_name", names.data_type().clone(), true));
        let mut columns = batch.columns().to_vec();
        columns[index] = Arc::new(names);
        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap();
        let decoded = YieldCurve::decode_typed_batch(&HashMap::new(), batch);

        match name {
            Some(name) => assert_eq!(decoded.unwrap()[0].curve_name, name),
            None => assert!(
                matches!(decoded.unwrap_err(), EncodingError::ParseError("curve_name", ref message) if message == "required column contains nulls")
            ),
        }
    }

    #[rstest]
    #[case("curve_name")]
    #[case("tenors")]
    #[case("interest_rates")]
    fn test_yield_curve_rejects_null_values(#[case] name: &'static str) {
        let value = YieldCurve::new(
            "USD-SOFR".to_string(),
            vec![0.25],
            vec![0.043],
            23.into(),
            29.into(),
        );
        let batch = YieldCurve::encode_batch(&value.metadata(), &[value]).unwrap();
        let index = batch.schema().index_of(name).unwrap();
        let mut fields = batch.schema().fields().to_vec();
        let mut columns = batch.columns().to_vec();

        let expected = if name == "curve_name" {
            fields[index] = Arc::new(fields[index].as_ref().clone().with_nullable(true));
            columns[index] = Arc::new(StringArray::from(vec![None::<&str>]));
            "required column contains nulls"
        } else {
            let mut builder = ListBuilder::new(Float64Builder::new());
            builder.values().append_null();
            builder.append(true);
            columns[index] = Arc::new(builder.finish());
            "row 0: list contains nulls"
        };

        let batch = RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).unwrap();
        let error = YieldCurve::decode_typed_batch(&HashMap::new(), batch).unwrap_err();

        assert!(
            matches!(error, EncodingError::ParseError(field, ref message)
            if field == name && message == expected)
        );
    }
}
