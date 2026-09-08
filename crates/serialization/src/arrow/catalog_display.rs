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

//! Converts raw catalog Arrow batches into display-friendly Arrow batches.
//!
//! Catalog batches store prices and quantities as scale-16 decimals plus schema metadata. This
//! module preserves the raw catalog query path while converting the values to named `Float64`
//! columns suitable for PyArrow/Polars. Migrate legacy catalog files before requesting display output.

use std::{collections::HashMap, str::FromStr, sync::Arc};

use arrow::{
    array::{
        Array, ArrayRef, BooleanArray, Decimal128Array, Float64Array, Float64Builder, Int8Array,
        Int16Array, ListArray, StringArray, StringBuilder, StructArray, TimestampNanosecondArray,
        TimestampNanosecondBuilder, UInt8Array, UInt8Builder, UInt32Array, UInt64Array,
        UInt64Builder,
    },
    compute::cast,
    datatypes::{DataType, Field, Schema},
    record_batch::RecordBatch,
};
use nautilus_model::{
    data::{
        Bar, BarType, FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
        MarkPriceUpdate, NautilusDataType, OptionGreeks, OrderBookDelta, OrderBookDepth, QuoteTick,
        TradeTick, get_arrow_schema,
    },
    enums::{AggressorSide, BookAction, InstrumentCloseType, OrderSide},
    instruments::InstrumentAny,
    types::{Price, Quantity, fixed::MAX_FLOAT_PRECISION, price::PRICE_ERROR},
};
use rust_decimal::{Decimal, prelude::ToPrimitive};

use super::{
    ArrowSchemaProvider, EncodingError, KEY_BAR_TYPE, KEY_IDENTIFIER, KEY_INSTRUMENT_ID,
    KEY_PRICE_PRECISION, KEY_SIZE_PRECISION, U64ColumnRef,
    custom::CustomDataDecoder,
    decode_decimal_price, decode_decimal_quantity,
    depth_display::{DepthSideBuilder, schema as depth_schema},
    extract_column, extract_column_string, fixed_decimal_data_type,
};
use crate::arrow::timestamp_data_type;

const DISPLAY_MAX_PRECISION: u8 = 18;
struct CatalogDisplayFns {
    schema: fn() -> Schema,
    convert: fn(&HashMap<String, String>, &RecordBatch) -> Result<RecordBatch, EncodingError>,
}

trait CatalogDisplay {
    const FUNCTIONS: Option<CatalogDisplayFns>;
}

impl CatalogDisplay for InstrumentAny {
    const FUNCTIONS: Option<CatalogDisplayFns> = None;
}

impl CatalogDisplay for QuoteTick {
    const FUNCTIONS: Option<CatalogDisplayFns> = Some(CatalogDisplayFns {
        schema: quotes_schema,
        convert: convert_quotes,
    });
}

impl CatalogDisplay for TradeTick {
    const FUNCTIONS: Option<CatalogDisplayFns> = Some(CatalogDisplayFns {
        schema: trades_schema,
        convert: convert_trades,
    });
}

impl CatalogDisplay for Bar {
    const FUNCTIONS: Option<CatalogDisplayFns> = Some(CatalogDisplayFns {
        schema: bars_schema,
        convert: convert_bars,
    });
}

impl CatalogDisplay for OrderBookDelta {
    const FUNCTIONS: Option<CatalogDisplayFns> = Some(CatalogDisplayFns {
        schema: deltas_schema,
        convert: convert_deltas,
    });
}

impl CatalogDisplay for OrderBookDepth {
    const FUNCTIONS: Option<CatalogDisplayFns> = Some(CatalogDisplayFns {
        schema: depth_schema,
        convert: convert_depths,
    });
}

impl CatalogDisplay for MarkPriceUpdate {
    const FUNCTIONS: Option<CatalogDisplayFns> = Some(CatalogDisplayFns {
        schema: price_update_schema,
        convert: convert_price_updates,
    });
}

impl CatalogDisplay for IndexPriceUpdate {
    const FUNCTIONS: Option<CatalogDisplayFns> = Some(CatalogDisplayFns {
        schema: price_update_schema,
        convert: convert_price_updates,
    });
}

impl CatalogDisplay for FundingRateUpdate {
    const FUNCTIONS: Option<CatalogDisplayFns> = Some(CatalogDisplayFns {
        schema: funding_rate_schema,
        convert: convert_funding_rates_with_metadata,
    });
}

impl CatalogDisplay for InstrumentStatus {
    const FUNCTIONS: Option<CatalogDisplayFns> = Some(CatalogDisplayFns {
        schema: instrument_status_schema,
        convert: convert_instrument_status_with_metadata,
    });
}

impl CatalogDisplay for OptionGreeks {
    const FUNCTIONS: Option<CatalogDisplayFns> = Some(CatalogDisplayFns {
        schema: option_greeks_schema,
        convert: convert_option_greeks_with_metadata,
    });
}

impl CatalogDisplay for InstrumentClose {
    const FUNCTIONS: Option<CatalogDisplayFns> = Some(CatalogDisplayFns {
        schema: instrument_closes_schema,
        convert: convert_instrument_closes,
    });
}

macro_rules! define_catalog_display_lookup {
    ($(($variant:ident, $type:ident, $data:ident, $batch:ident, $prefix:literal)),+ $(,)?) => {
        fn builtin_catalog_display(data_type: &NautilusDataType) -> Option<CatalogDisplayFns> {
            match data_type {
                $(
                    NautilusDataType::$variant => <$type as CatalogDisplay>::FUNCTIONS,
                )+
                _ => None,
            }
        }
    };
}

nautilus_model::for_each_data_type!(define_catalog_display_lookup);

macro_rules! define_catalog_raw_schema_lookup {
    (
        ($_instrument_variant:ident, $_instrument_type:ident, $_instrument_data:ident, $_instrument_batch:ident, $_instrument_prefix:literal),
        $(($variant:ident, $type:ident, $data:ident, $batch:ident, $prefix:literal)),+ $(,)?
    ) => {
        fn builtin_catalog_raw_schema(data_type: &NautilusDataType) -> Option<Schema> {
            match data_type {
                $(
                    NautilusDataType::$variant => {
                        Some(<$type as ArrowSchemaProvider>::get_schema(None))
                    }
                )+
                _ => None,
            }
        }
    };
}

nautilus_model::for_each_data_type!(define_catalog_raw_schema_lookup);

/// Returns the raw Arrow schema for a catalog data type.
///
/// # Errors
///
/// Returns [`EncodingError::ParseError`] when the data type is unsupported.
pub fn catalog_raw_schema(data_type: &NautilusDataType) -> Result<Schema, EncodingError> {
    if let NautilusDataType::Custom { type_name } = data_type {
        if get_arrow_schema(type_name).is_none() {
            return Err(unsupported_display_type(data_type));
        }
        let metadata = HashMap::from([("type_name".to_string(), type_name.clone())]);
        return Ok(super::schema_with_identifier_column(
            &CustomDataDecoder::get_schema(Some(metadata)),
        ));
    }

    builtin_catalog_raw_schema(data_type).ok_or_else(|| unsupported_display_type(data_type))
}

/// Returns a display-friendly Arrow schema for a catalog data type.
///
/// # Errors
///
/// Returns [`EncodingError::ParseError`] when the data type is unsupported.
#[allow(
    clippy::match_wildcard_for_single_variants,
    unreachable_patterns,
    reason = "NautilusDataType::Defi is controlled by nautilus-model features, not this crate"
)]
pub fn catalog_display_schema(data_type: &NautilusDataType) -> Result<Schema, EncodingError> {
    if matches!(data_type, NautilusDataType::Custom { .. }) {
        let schema = catalog_raw_schema(data_type)?;
        let batch = RecordBatch::new_empty(Arc::new(schema));
        return catalog_record_batch_to_display(data_type, batch.schema().metadata(), &batch)
            .map(|batch| batch.schema().as_ref().clone());
    }

    builtin_catalog_display(data_type)
        .map(|functions| (functions.schema)())
        .ok_or_else(|| unsupported_display_type(data_type))
}

/// Converts a raw catalog record batch into display Arrow without decoding full data objects.
///
/// # Errors
///
/// Returns [`EncodingError`] when required metadata or columns are missing, a
/// columns do not use the current Arrow format, or enum values are invalid.
#[allow(
    clippy::match_wildcard_for_single_variants,
    unreachable_patterns,
    reason = "NautilusDataType::Defi is controlled by nautilus-model features, not this crate"
)]
pub fn catalog_record_batch_to_display(
    data_type: &NautilusDataType,
    metadata: &HashMap<String, String>,
    record_batch: &RecordBatch,
) -> Result<RecordBatch, EncodingError> {
    let display_batch = if matches!(data_type, NautilusDataType::Custom { .. }) {
        convert_custom(record_batch)
    } else if let Some(functions) = builtin_catalog_display(data_type) {
        (functions.convert)(metadata, record_batch)
    } else {
        Err(unsupported_display_type(data_type))
    }?;

    append_identifier_column_if_present(display_batch, record_batch)
}

fn unsupported_display_type(data_type: &NautilusDataType) -> EncodingError {
    EncodingError::ParseError(
        "data_type",
        format!("unsupported display catalog data type `{data_type}`"),
    )
}

fn convert_funding_rates_with_metadata(
    _: &HashMap<String, String>,
    batch: &RecordBatch,
) -> Result<RecordBatch, EncodingError> {
    convert_funding_rates(batch)
}

fn convert_option_greeks_with_metadata(
    _: &HashMap<String, String>,
    batch: &RecordBatch,
) -> Result<RecordBatch, EncodingError> {
    convert_option_greeks(batch)
}

fn convert_instrument_status_with_metadata(
    _: &HashMap<String, String>,
    batch: &RecordBatch,
) -> Result<RecordBatch, EncodingError> {
    convert_instrument_status(batch)
}

fn utf8_field(name: &str, nullable: bool) -> Field {
    Field::new(name, DataType::Utf8, nullable)
}

fn float64_field(name: &str, nullable: bool) -> Field {
    Field::new(name, DataType::Float64, nullable)
}

fn timestamp_field(name: &str, nullable: bool) -> Field {
    Field::new(name, timestamp_data_type(), nullable)
}

fn append_identifier_column_if_present(
    display_batch: RecordBatch,
    catalog_batch: &RecordBatch,
) -> Result<RecordBatch, EncodingError> {
    if display_batch.schema().index_of(KEY_IDENTIFIER).is_ok() {
        return Ok(display_batch);
    }

    let Ok(identifier_index) = catalog_batch.schema().index_of(KEY_IDENTIFIER) else {
        return Ok(display_batch);
    };

    let catalog_schema = catalog_batch.schema();
    let identifier_field = catalog_schema.field(identifier_index);
    let (identifier_field, identifier_column) =
        if identifier_field.data_type() == &DataType::Utf8View {
            (
                Field::new(
                    identifier_field.name(),
                    DataType::Utf8,
                    identifier_field.is_nullable(),
                ),
                cast(
                    catalog_batch.column(identifier_index).as_ref(),
                    &DataType::Utf8,
                )?,
            )
        } else {
            (
                identifier_field.clone(),
                catalog_batch.column(identifier_index).clone(),
            )
        };

    let mut fields = display_batch.schema().fields().to_vec();
    fields.push(Arc::new(identifier_field));
    let mut columns = display_batch.columns().to_vec();
    columns.push(identifier_column);

    RecordBatch::try_new(Arc::new(Schema::new(fields)), columns).map_err(EncodingError::from)
}

fn price_to_f64(price: &Price) -> f64 {
    if price.is_undefined() || price.raw == PRICE_ERROR || price.precision > DISPLAY_MAX_PRECISION {
        return f64::NAN;
    }

    if price.precision <= MAX_FLOAT_PRECISION {
        price.as_f64()
    } else {
        price.as_decimal().to_f64().unwrap_or(f64::NAN)
    }
}

fn quantity_to_f64(quantity: &Quantity) -> f64 {
    if quantity.is_undefined() || quantity.precision > DISPLAY_MAX_PRECISION {
        return f64::NAN;
    }

    if quantity.precision <= MAX_FLOAT_PRECISION {
        quantity.as_f64()
    } else {
        quantity.as_decimal().to_f64().unwrap_or(f64::NAN)
    }
}

fn parse_price_precision(metadata: &HashMap<String, String>) -> Result<u8, EncodingError> {
    parse_precision(metadata, KEY_PRICE_PRECISION)
}

fn parse_size_precision(metadata: &HashMap<String, String>) -> Result<u8, EncodingError> {
    parse_precision(metadata, KEY_SIZE_PRECISION)
}

fn parse_precision(
    metadata: &HashMap<String, String>,
    key: &'static str,
) -> Result<u8, EncodingError> {
    metadata
        .get(key)
        .ok_or(EncodingError::MissingMetadata(key))?
        .parse::<u8>()
        .map_err(|e| EncodingError::ParseError(key, e.to_string()))
}

fn instrument_id(metadata: &HashMap<String, String>) -> Result<&str, EncodingError> {
    metadata
        .get(KEY_INSTRUMENT_ID)
        .map(String::as_str)
        .ok_or_else(|| EncodingError::MissingMetadata(KEY_INSTRUMENT_ID))
}

fn bar_type(metadata: &HashMap<String, String>) -> Result<BarType, EncodingError> {
    let value = metadata
        .get(KEY_BAR_TYPE)
        .ok_or_else(|| EncodingError::MissingMetadata(KEY_BAR_TYPE))?;
    BarType::from_str(value).map_err(|e| EncodingError::ParseError(KEY_BAR_TYPE, e.to_string()))
}

fn constant_string_column(value: &str, len: usize) -> ArrayRef {
    Arc::new(StringArray::from_iter_values(std::iter::repeat_n(
        value, len,
    )))
}

struct FixedPrecisionColumn<'a>(&'a Decimal128Array);

impl FixedPrecisionColumn<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn is_null(&self, row: usize) -> bool {
        self.0.is_null(row)
    }
    fn price(
        &self,
        precision: u8,
        field: &'static str,
        row: usize,
    ) -> Result<Price, EncodingError> {
        decode_decimal_price(self.0, precision, field, row)
    }
    fn quantity(
        &self,
        precision: u8,
        field: &'static str,
        row: usize,
    ) -> Result<Quantity, EncodingError> {
        decode_decimal_quantity(self.0, precision, field, row)
    }
}

struct NanosColumn<'a>(&'a TimestampNanosecondArray);

impl NanosColumn<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }
    fn is_null(&self, row: usize) -> bool {
        self.0.is_null(row)
    }
    fn value(&self, row: usize) -> i64 {
        self.0.value(row)
    }
}

enum U8Column<'a> {
    UInt8(&'a UInt8Array),
    Int8(&'a Int8Array),
    Int16(&'a Int16Array),
}

impl U8Column<'_> {
    fn value(&self, row: usize, field: &'static str) -> Result<u8, EncodingError> {
        match self {
            Self::UInt8(values) => Ok(values.value(row)),
            Self::Int8(values) => u8::try_from(values.value(row)).map_err(|_| {
                EncodingError::ParseError(field, format!("Invalid negative value at row {row}"))
            }),
            Self::Int16(values) => u8::try_from(values.value(row)).map_err(|_| {
                EncodingError::ParseError(field, format!("Value out of u8 range at row {row}"))
            }),
        }
    }
}

fn fixed_col<'a>(
    batch: &'a RecordBatch,
    name: &'static str,
) -> Result<FixedPrecisionColumn<'a>, EncodingError> {
    let index = batch.schema().index_of(name)?;
    let column = batch.column(index);
    let expected = fixed_decimal_data_type();
    if column.data_type() != &expected {
        return Err(EncodingError::InvalidColumnType(
            name,
            index,
            expected,
            column.data_type().clone(),
        ));
    }
    Ok(FixedPrecisionColumn(extract_column::<Decimal128Array>(
        batch.columns(),
        name,
        index,
        fixed_decimal_data_type(),
    )?))
}

fn u8_col<'a>(batch: &'a RecordBatch, name: &'static str) -> Result<U8Column<'a>, EncodingError> {
    let index = batch.schema().index_of(name)?;
    let column = batch.column(index);
    match column.data_type() {
        DataType::UInt8 => Ok(U8Column::UInt8(extract_column::<UInt8Array>(
            batch.columns(),
            name,
            index,
            DataType::UInt8,
        )?)),
        DataType::Int8 => Ok(U8Column::Int8(extract_column::<Int8Array>(
            batch.columns(),
            name,
            index,
            DataType::Int8,
        )?)),
        DataType::Int16 => Ok(U8Column::Int16(extract_column::<Int16Array>(
            batch.columns(),
            name,
            index,
            DataType::Int16,
        )?)),
        data_type => Err(EncodingError::InvalidColumnType(
            name,
            index,
            DataType::UInt8,
            data_type.clone(),
        )),
    }
}

fn u64_value(
    values: &U64ColumnRef<'_>,
    row: usize,
    field: &'static str,
) -> Result<u64, EncodingError> {
    values.value(row).ok_or_else(|| {
        EncodingError::ParseError(field, format!("Invalid negative value at row {row}"))
    })
}

fn u64_col<'a>(
    batch: &'a RecordBatch,
    name: &'static str,
) -> Result<U64ColumnRef<'a>, EncodingError> {
    let index = batch.schema().index_of(name)?;
    let column = batch.column(index);
    U64ColumnRef::try_from_array(column.as_ref()).ok_or_else(|| {
        EncodingError::InvalidColumnType(name, index, DataType::UInt64, column.data_type().clone())
    })
}

fn nanos_col<'a>(
    batch: &'a RecordBatch,
    name: &'static str,
) -> Result<NanosColumn<'a>, EncodingError> {
    let index = batch.schema().index_of(name)?;
    let column = batch.column(index);
    let expected = timestamp_data_type();
    if column.data_type() != &expected {
        return Err(EncodingError::InvalidColumnType(
            name,
            index,
            expected,
            column.data_type().clone(),
        ));
    }
    Ok(NanosColumn(extract_column::<TimestampNanosecondArray>(
        batch.columns(),
        name,
        index,
        expected,
    )?))
}

fn f64_col<'a>(
    batch: &'a RecordBatch,
    name: &'static str,
) -> Result<&'a Float64Array, EncodingError> {
    let index = batch.schema().index_of(name)?;
    extract_column::<Float64Array>(batch.columns(), name, index, DataType::Float64)
}

fn bool_col<'a>(
    batch: &'a RecordBatch,
    name: &'static str,
) -> Result<&'a BooleanArray, EncodingError> {
    let index = batch.schema().index_of(name)?;
    extract_column::<BooleanArray>(batch.columns(), name, index, DataType::Boolean)
}

fn timestamp_array_from_nanos(values: &NanosColumn<'_>) -> ArrayRef {
    let mut builder = TimestampNanosecondBuilder::with_capacity(values.len())
        .with_data_type(timestamp_data_type());

    for row in 0..values.len() {
        append_timestamp(&mut builder, values, row);
    }
    Arc::new(builder.finish())
}

fn append_timestamp(
    builder: &mut TimestampNanosecondBuilder,
    values: &NanosColumn<'_>,
    row: usize,
) {
    if values.is_null(row) {
        builder.append_null();
    } else {
        builder.append_value(values.value(row));
    }
}

fn timestamp_field_from(field: &Field) -> Field {
    Field::new(field.name(), timestamp_data_type(), field.is_nullable())
}

fn fixed_precision_field_to_f64(
    values: &FixedPrecisionColumn<'_>,
    precision: u8,
    is_price: bool,
) -> Result<ArrayRef, EncodingError> {
    let mut builder = Float64Builder::with_capacity(values.len());
    for row in 0..values.len() {
        if values.is_null(row) {
            builder.append_null();
        } else if is_price {
            append_price(&mut builder, values, precision, "custom", row)?;
        } else {
            append_quantity(&mut builder, values, precision, "custom", row)?;
        }
    }

    Ok(Arc::new(builder.finish()))
}

fn custom_fixed_precision(
    field_name: &str,
    metadata: &HashMap<String, String>,
) -> Option<(u8, bool)> {
    let normalized = field_name.to_ascii_lowercase();
    let field_precision = metadata
        .get(&format!("{field_name}_precision"))
        .and_then(|value| value.parse::<u8>().ok());

    match metadata
        .get(&format!("{field_name}_kind"))
        .map(String::as_str)
    {
        Some("price") => return field_precision.map(|precision| (precision, true)),
        Some("quantity") => return field_precision.map(|precision| (precision, false)),
        _ => {}
    }
    let is_quantity = matches!(
        normalized.as_str(),
        "size" | "quantity" | "qty" | "volume" | "bid_size" | "ask_size"
    ) || normalized.ends_with("_size")
        || normalized.ends_with("_quantity")
        || normalized.ends_with("_qty")
        || normalized.ends_with("_volume");

    if is_quantity {
        return field_precision
            .or_else(|| {
                metadata
                    .get(KEY_SIZE_PRECISION)
                    .and_then(|value| value.parse::<u8>().ok())
            })
            .map(|precision| (precision, false));
    }

    if let Some(precision) = field_precision {
        return Some((precision, true));
    }

    let is_price = matches!(
        normalized.as_str(),
        "price"
            | "value"
            | "bid"
            | "ask"
            | "bid_price"
            | "ask_price"
            | "open"
            | "high"
            | "low"
            | "close"
    ) || normalized.ends_with("_price")
        || normalized.ends_with("_value")
        || normalized.ends_with("_bid")
        || normalized.ends_with("_ask")
        || normalized.ends_with("_open")
        || normalized.ends_with("_high")
        || normalized.ends_with("_low")
        || normalized.ends_with("_close");

    if is_price {
        return metadata
            .get(KEY_PRICE_PRECISION)
            .and_then(|value| value.parse::<u8>().ok())
            .map(|precision| (precision, true));
    }

    None
}

fn append_price(
    builder: &mut Float64Builder,
    values: &FixedPrecisionColumn<'_>,
    precision: u8,
    field: &'static str,
    row: usize,
) -> Result<(), EncodingError> {
    let price = values.price(precision, field, row)?;
    builder.append_value(price_to_f64(&price));
    Ok(())
}

fn append_price_with_sentinel(
    builder: &mut Float64Builder,
    values: &FixedPrecisionColumn<'_>,
    precision: u8,
    field: &'static str,
    row: usize,
) -> Result<(), EncodingError> {
    let price = values.price(precision, field, row)?;
    builder.append_value(price_to_f64(&price));
    Ok(())
}

fn append_optional_price_with_sentinel(
    builder: &mut Float64Builder,
    values: &FixedPrecisionColumn<'_>,
    precision: u8,
    field: &'static str,
    row: usize,
) -> Result<(), EncodingError> {
    let price = values.price(precision, field, row)?;

    if price.is_undefined() {
        builder.append_null();
    } else {
        builder.append_value(price_to_f64(&price));
    }
    Ok(())
}

fn append_quantity(
    builder: &mut Float64Builder,
    values: &FixedPrecisionColumn<'_>,
    precision: u8,
    field: &'static str,
    row: usize,
) -> Result<(), EncodingError> {
    let quantity = values.quantity(precision, field, row)?;
    builder.append_value(quantity_to_f64(&quantity));
    Ok(())
}

fn append_quantity_with_sentinel(
    builder: &mut Float64Builder,
    values: &FixedPrecisionColumn<'_>,
    precision: u8,
    field: &'static str,
    row: usize,
) -> Result<(), EncodingError> {
    let quantity = values.quantity(precision, field, row)?;
    builder.append_value(quantity_to_f64(&quantity));
    Ok(())
}

fn append_optional_quantity_with_sentinel(
    builder: &mut Float64Builder,
    values: &FixedPrecisionColumn<'_>,
    precision: u8,
    field: &'static str,
    row: usize,
) -> Result<(), EncodingError> {
    let quantity = values.quantity(precision, field, row)?;

    if quantity.is_undefined() {
        builder.append_null();
    } else {
        builder.append_value(quantity_to_f64(&quantity));
    }
    Ok(())
}

fn bars_schema() -> Schema {
    Schema::new(vec![
        utf8_field("instrument_id", false),
        utf8_field("bar_type", false),
        float64_field("open", false),
        float64_field("high", false),
        float64_field("low", false),
        float64_field("close", false),
        float64_field("volume", false),
        timestamp_field("ts_event", false),
        timestamp_field("ts_init", false),
    ])
}

fn convert_bars(
    metadata: &HashMap<String, String>,
    batch: &RecordBatch,
) -> Result<RecordBatch, EncodingError> {
    let bar_type = bar_type(metadata)?;
    let bar_type_string = bar_type.to_string();
    let instrument_id_string = bar_type.instrument_id().to_string();
    let price_precision = parse_price_precision(metadata)?;
    let size_precision = parse_size_precision(metadata)?;
    let open = fixed_col(batch, "open")?;
    let high = fixed_col(batch, "high")?;
    let low = fixed_col(batch, "low")?;
    let close = fixed_col(batch, "close")?;
    let volume = fixed_col(batch, "volume")?;
    let ts_event = nanos_col(batch, "ts_event")?;
    let ts_init = nanos_col(batch, "ts_init")?;

    let len = batch.num_rows();
    let mut open_builder = Float64Builder::with_capacity(len);
    let mut high_builder = Float64Builder::with_capacity(len);
    let mut low_builder = Float64Builder::with_capacity(len);
    let mut close_builder = Float64Builder::with_capacity(len);
    let mut volume_builder = Float64Builder::with_capacity(len);
    let mut ts_event_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());
    let mut ts_init_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());

    for row in 0..len {
        append_price(&mut open_builder, &open, price_precision, "open", row)?;
        append_price(&mut high_builder, &high, price_precision, "high", row)?;
        append_price(&mut low_builder, &low, price_precision, "low", row)?;
        append_price(&mut close_builder, &close, price_precision, "close", row)?;
        append_quantity(&mut volume_builder, &volume, size_precision, "volume", row)?;
        append_timestamp(&mut ts_event_builder, &ts_event, row);
        append_timestamp(&mut ts_init_builder, &ts_init, row);
    }

    RecordBatch::try_new(
        Arc::new(bars_schema()),
        vec![
            constant_string_column(&instrument_id_string, len),
            constant_string_column(&bar_type_string, len),
            Arc::new(open_builder.finish()),
            Arc::new(high_builder.finish()),
            Arc::new(low_builder.finish()),
            Arc::new(close_builder.finish()),
            Arc::new(volume_builder.finish()),
            Arc::new(ts_event_builder.finish()),
            Arc::new(ts_init_builder.finish()),
        ],
    )
    .map_err(EncodingError::from)
}

fn quotes_schema() -> Schema {
    Schema::new(vec![
        utf8_field("instrument_id", false),
        float64_field("bid_price", false),
        float64_field("ask_price", false),
        float64_field("bid_size", false),
        float64_field("ask_size", false),
        timestamp_field("ts_event", false),
        timestamp_field("ts_init", false),
    ])
}

fn convert_quotes(
    metadata: &HashMap<String, String>,
    batch: &RecordBatch,
) -> Result<RecordBatch, EncodingError> {
    let instrument_id = instrument_id(metadata)?;
    let price_precision = parse_price_precision(metadata)?;
    let size_precision = parse_size_precision(metadata)?;
    let bid_price = fixed_col(batch, "bid_price")?;
    let ask_price = fixed_col(batch, "ask_price")?;
    let bid_size = fixed_col(batch, "bid_size")?;
    let ask_size = fixed_col(batch, "ask_size")?;
    let ts_event = nanos_col(batch, "ts_event")?;
    let ts_init = nanos_col(batch, "ts_init")?;

    let len = batch.num_rows();
    let mut bid_price_builder = Float64Builder::with_capacity(len);
    let mut ask_price_builder = Float64Builder::with_capacity(len);
    let mut bid_size_builder = Float64Builder::with_capacity(len);
    let mut ask_size_builder = Float64Builder::with_capacity(len);
    let mut ts_event_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());
    let mut ts_init_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());

    for row in 0..len {
        append_price(
            &mut bid_price_builder,
            &bid_price,
            price_precision,
            "bid_price",
            row,
        )?;
        append_price(
            &mut ask_price_builder,
            &ask_price,
            price_precision,
            "ask_price",
            row,
        )?;
        append_quantity(
            &mut bid_size_builder,
            &bid_size,
            size_precision,
            "bid_size",
            row,
        )?;
        append_quantity(
            &mut ask_size_builder,
            &ask_size,
            size_precision,
            "ask_size",
            row,
        )?;
        append_timestamp(&mut ts_event_builder, &ts_event, row);
        append_timestamp(&mut ts_init_builder, &ts_init, row);
    }

    RecordBatch::try_new(
        Arc::new(quotes_schema()),
        vec![
            constant_string_column(instrument_id, len),
            Arc::new(bid_price_builder.finish()),
            Arc::new(ask_price_builder.finish()),
            Arc::new(bid_size_builder.finish()),
            Arc::new(ask_size_builder.finish()),
            Arc::new(ts_event_builder.finish()),
            Arc::new(ts_init_builder.finish()),
        ],
    )
    .map_err(EncodingError::from)
}

fn trades_schema() -> Schema {
    Schema::new(vec![
        utf8_field("instrument_id", false),
        float64_field("price", false),
        float64_field("size", false),
        utf8_field("aggressor_side", false),
        utf8_field("trade_id", false),
        timestamp_field("ts_event", false),
        timestamp_field("ts_init", false),
    ])
}

fn convert_trades(
    metadata: &HashMap<String, String>,
    batch: &RecordBatch,
) -> Result<RecordBatch, EncodingError> {
    let instrument_id = instrument_id(metadata)?;
    let price_precision = parse_price_precision(metadata)?;
    let size_precision = parse_size_precision(metadata)?;
    let price = fixed_col(batch, "price")?;
    let size = fixed_col(batch, "size")?;
    let aggressor_side_index = batch.schema().index_of("aggressor_side")?;
    let aggressor_side =
        extract_column_string(batch.columns(), "aggressor_side", aggressor_side_index)?;
    let trade_id_index = batch.schema().index_of("trade_id")?;
    let trade_id = extract_column_string(batch.columns(), "trade_id", trade_id_index)?;
    let ts_event = nanos_col(batch, "ts_event")?;
    let ts_init = nanos_col(batch, "ts_init")?;

    let len = batch.num_rows();
    let mut price_builder = Float64Builder::with_capacity(len);
    let mut size_builder = Float64Builder::with_capacity(len);
    let mut aggressor_side_builder = StringBuilder::new();
    let mut trade_id_builder = StringBuilder::new();
    let mut ts_event_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());
    let mut ts_init_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());

    for row in 0..len {
        let aggressor_side_value = aggressor_side.value(row);
        let side = AggressorSide::from_str(aggressor_side_value)
            .map_err(|e| EncodingError::ParseError(stringify!(AggressorSide), e.to_string()))?;
        append_price(&mut price_builder, &price, price_precision, "price", row)?;
        append_quantity(&mut size_builder, &size, size_precision, "size", row)?;
        aggressor_side_builder.append_value(side.as_ref());
        trade_id_builder.append_value(trade_id.value(row));
        append_timestamp(&mut ts_event_builder, &ts_event, row);
        append_timestamp(&mut ts_init_builder, &ts_init, row);
    }

    RecordBatch::try_new(
        Arc::new(trades_schema()),
        vec![
            constant_string_column(instrument_id, len),
            Arc::new(price_builder.finish()),
            Arc::new(size_builder.finish()),
            Arc::new(aggressor_side_builder.finish()),
            Arc::new(trade_id_builder.finish()),
            Arc::new(ts_event_builder.finish()),
            Arc::new(ts_init_builder.finish()),
        ],
    )
    .map_err(EncodingError::from)
}

fn deltas_schema() -> Schema {
    Schema::new(vec![
        utf8_field("instrument_id", false),
        utf8_field("action", false),
        utf8_field("side", false),
        float64_field("price", false),
        float64_field("size", false),
        utf8_field("order_id", false),
        Field::new("flags", DataType::UInt8, false),
        Field::new("sequence", DataType::UInt64, false),
        timestamp_field("ts_event", false),
        timestamp_field("ts_init", false),
    ])
}

fn convert_deltas(
    metadata: &HashMap<String, String>,
    batch: &RecordBatch,
) -> Result<RecordBatch, EncodingError> {
    let instrument_id = instrument_id(metadata)?;
    let price_precision = parse_price_precision(metadata)?;
    let size_precision = parse_size_precision(metadata)?;
    let action_index = batch.schema().index_of("action")?;
    let action = extract_column_string(batch.columns(), "action", action_index)?;
    let side_index = batch.schema().index_of("side")?;
    let side = extract_column_string(batch.columns(), "side", side_index)?;
    let price = fixed_col(batch, "price")?;
    let size = fixed_col(batch, "size")?;
    let order_id = u64_col(batch, "order_id")?;
    let flags = u8_col(batch, "flags")?;
    let sequence = u64_col(batch, "sequence")?;
    let ts_event = nanos_col(batch, "ts_event")?;
    let ts_init = nanos_col(batch, "ts_init")?;

    let len = batch.num_rows();
    let mut action_builder = StringBuilder::new();
    let mut side_builder = StringBuilder::new();
    let mut price_builder = Float64Builder::with_capacity(len);
    let mut size_builder = Float64Builder::with_capacity(len);
    let mut order_id_builder = StringBuilder::new();
    let mut flags_builder = UInt8Builder::with_capacity(len);
    let mut sequence_builder = UInt64Builder::with_capacity(len);
    let mut ts_event_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());
    let mut ts_init_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());

    for row in 0..len {
        let action_value = action.value(row);
        let action = BookAction::from_str(action_value)
            .map_err(|e| EncodingError::ParseError(stringify!(BookAction), e.to_string()))?;
        let side_value = side.value(row);
        let side = OrderSide::from_str(side_value)
            .map_err(|e| EncodingError::ParseError(stringify!(OrderSide), e.to_string()))?;

        action_builder.append_value(action.as_ref());
        side_builder.append_value(side.as_ref());
        if action == BookAction::Clear {
            price_builder.append_value(f64::NAN);
            size_builder.append_value(f64::NAN);
        } else {
            append_price_with_sentinel(&mut price_builder, &price, price_precision, "price", row)?;
            append_quantity_with_sentinel(&mut size_builder, &size, size_precision, "size", row)?;
        }
        order_id_builder.append_value(u64_value(&order_id, row, "order_id")?.to_string());
        flags_builder.append_value(flags.value(row, "flags")?);
        sequence_builder.append_value(u64_value(&sequence, row, "sequence")?);
        append_timestamp(&mut ts_event_builder, &ts_event, row);
        append_timestamp(&mut ts_init_builder, &ts_init, row);
    }

    RecordBatch::try_new(
        Arc::new(deltas_schema()),
        vec![
            constant_string_column(instrument_id, len),
            Arc::new(action_builder.finish()),
            Arc::new(side_builder.finish()),
            Arc::new(price_builder.finish()),
            Arc::new(size_builder.finish()),
            Arc::new(order_id_builder.finish()),
            Arc::new(flags_builder.finish()),
            Arc::new(sequence_builder.finish()),
            Arc::new(ts_event_builder.finish()),
            Arc::new(ts_init_builder.finish()),
        ],
    )
    .map_err(EncodingError::from)
}

fn convert_depths(
    metadata: &HashMap<String, String>,
    batch: &RecordBatch,
) -> Result<RecordBatch, EncodingError> {
    let instrument_id = instrument_id(metadata)?;
    let price_precision = parse_price_precision(metadata)?;
    let size_precision = parse_size_precision(metadata)?;
    let bids = depth_struct_values(batch, "bids")?;
    let asks = depth_struct_values(batch, "asks")?;
    let flags = u8_col(batch, "flags")?;
    let sequence = u64_col(batch, "sequence")?;
    let ts_event = nanos_col(batch, "ts_event")?;
    let ts_init = nanos_col(batch, "ts_init")?;
    let len = batch.num_rows();
    let mut bids_display = DepthSideBuilder::new();
    let mut asks_display = DepthSideBuilder::new();
    let mut flags_builder = UInt8Builder::with_capacity(len);
    let mut sequence_builder = UInt64Builder::with_capacity(len);
    let mut ts_event_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());
    let mut ts_init_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());
    let bid_prices = FixedPrecisionColumn(&bids.prices);
    let ask_prices = FixedPrecisionColumn(&asks.prices);
    let bid_sizes = FixedPrecisionColumn(&bids.sizes);
    let ask_sizes = FixedPrecisionColumn(&asks.sizes);

    for row in 0..len {
        let (bid_start, bid_end) = depth_struct_range(&bids.list, "bids", row)?;
        let (ask_start, ask_end) = depth_struct_range(&asks.list, "asks", row)?;

        for value_index in bid_start..bid_end {
            append_optional_price_with_sentinel(
                &mut bids_display.prices,
                &bid_prices,
                price_precision,
                "bids.price",
                value_index,
            )?;
            append_optional_quantity_with_sentinel(
                &mut bids_display.sizes,
                &bid_sizes,
                size_precision,
                "bids.size",
                value_index,
            )?;

            if bids.counts.is_null(value_index) || bids.order_ids.is_null(value_index) {
                return Err(EncodingError::ParseError(
                    "bids",
                    "count and order_id must not be null".to_string(),
                ));
            }
            bids_display
                .counts
                .append_value(bids.counts.value(value_index));
            bids_display
                .order_ids
                .append_value(bids.order_ids.value(value_index));
        }

        for value_index in ask_start..ask_end {
            append_optional_price_with_sentinel(
                &mut asks_display.prices,
                &ask_prices,
                price_precision,
                "asks.price",
                value_index,
            )?;
            append_optional_quantity_with_sentinel(
                &mut asks_display.sizes,
                &ask_sizes,
                size_precision,
                "asks.size",
                value_index,
            )?;

            if asks.counts.is_null(value_index) || asks.order_ids.is_null(value_index) {
                return Err(EncodingError::ParseError(
                    "asks",
                    "count and order_id must not be null".to_string(),
                ));
            }
            asks_display
                .counts
                .append_value(asks.counts.value(value_index));
            asks_display
                .order_ids
                .append_value(asks.order_ids.value(value_index));
        }
        bids_display.finish_row()?;
        asks_display.finish_row()?;
        flags_builder.append_value(flags.value(row, "flags")?);
        sequence_builder.append_value(u64_value(&sequence, row, "sequence")?);
        append_timestamp(&mut ts_event_builder, &ts_event, row);
        append_timestamp(&mut ts_init_builder, &ts_init, row);
    }

    finish_depth_display(
        instrument_id,
        len,
        bids_display,
        asks_display,
        flags_builder,
        sequence_builder,
        ts_event_builder,
        ts_init_builder,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "the two nested sides and four scalar builders form one batch"
)]
fn finish_depth_display(
    instrument_id: &str,
    len: usize,
    bids: DepthSideBuilder,
    asks: DepthSideBuilder,
    mut flags: UInt8Builder,
    mut sequence: UInt64Builder,
    mut ts_event: TimestampNanosecondBuilder,
    mut ts_init: TimestampNanosecondBuilder,
) -> Result<RecordBatch, EncodingError> {
    RecordBatch::try_new(
        Arc::new(depth_schema()),
        vec![
            constant_string_column(instrument_id, len),
            Arc::new(bids.finish()?),
            Arc::new(asks.finish()?),
            Arc::new(flags.finish()),
            Arc::new(sequence.finish()),
            Arc::new(ts_event.finish()),
            Arc::new(ts_init.finish()),
        ],
    )
    .map_err(EncodingError::from)
}

struct DepthStructColumns {
    list: ListArray,
    prices: Decimal128Array,
    sizes: Decimal128Array,
    counts: UInt32Array,
    order_ids: UInt64Array,
}

fn depth_struct_values(
    batch: &RecordBatch,
    name: &'static str,
) -> Result<DepthStructColumns, EncodingError> {
    let list = batch
        .column_by_name(name)
        .and_then(|column| column.as_any().downcast_ref::<ListArray>())
        .cloned()
        .ok_or_else(|| EncodingError::ParseError(name, "expected List<Struct>".to_string()))?;
    let levels = list
        .values()
        .as_any()
        .downcast_ref::<StructArray>()
        .ok_or_else(|| EncodingError::ParseError(name, "expected List<Struct>".to_string()))?;
    let child = |field: &'static str| {
        levels.column_by_name(field).cloned().ok_or_else(|| {
            EncodingError::ParseError(name, format!("missing struct field '{field}'"))
        })
    };

    for field in ["price", "size"] {
        if child(field)?.data_type() != &fixed_decimal_data_type() {
            return Err(EncodingError::ParseError(
                name,
                format!("field '{field}' must use Decimal128(38, 16)"),
            ));
        }
    }
    let prices = child("price")?
        .as_any()
        .downcast_ref::<Decimal128Array>()
        .cloned()
        .ok_or_else(|| {
            EncodingError::ParseError(name, "field 'price' must be Decimal128".to_string())
        })?;
    let sizes = child("size")?
        .as_any()
        .downcast_ref::<Decimal128Array>()
        .cloned()
        .ok_or_else(|| {
            EncodingError::ParseError(name, "field 'size' must be Decimal128".to_string())
        })?;
    let counts = child("count")?
        .as_any()
        .downcast_ref::<UInt32Array>()
        .cloned()
        .ok_or_else(|| {
            EncodingError::ParseError(name, "field 'count' must be UInt32".to_string())
        })?;
    let order_ids = child("order_id")?
        .as_any()
        .downcast_ref::<UInt64Array>()
        .cloned()
        .ok_or_else(|| {
            EncodingError::ParseError(name, "field 'order_id' must be UInt64".to_string())
        })?;
    Ok(DepthStructColumns {
        list,
        prices,
        sizes,
        counts,
        order_ids,
    })
}

fn depth_struct_range(
    list: &ListArray,
    name: &'static str,
    row: usize,
) -> Result<(usize, usize), EncodingError> {
    if list.is_null(row) {
        return Err(EncodingError::ParseError(
            name,
            format!("side is null at row {row}"),
        ));
    }
    let offsets = list.value_offsets();
    let start = usize::try_from(offsets[row])
        .map_err(|e| EncodingError::ParseError(name, e.to_string()))?;
    let end = usize::try_from(offsets[row + 1])
        .map_err(|e| EncodingError::ParseError(name, e.to_string()))?;
    Ok((start, end))
}

fn price_update_schema() -> Schema {
    Schema::new(vec![
        utf8_field("instrument_id", false),
        float64_field("value", false),
        timestamp_field("ts_event", false),
        timestamp_field("ts_init", false),
    ])
}

fn convert_price_updates(
    metadata: &HashMap<String, String>,
    batch: &RecordBatch,
) -> Result<RecordBatch, EncodingError> {
    let instrument_id = instrument_id(metadata)?;
    let price_precision = parse_price_precision(metadata)?;
    let value = fixed_col(batch, "value")?;
    let ts_event = nanos_col(batch, "ts_event")?;
    let ts_init = nanos_col(batch, "ts_init")?;
    let len = batch.num_rows();
    let mut value_builder = Float64Builder::with_capacity(len);
    let mut ts_event_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());
    let mut ts_init_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());

    for row in 0..len {
        append_price(&mut value_builder, &value, price_precision, "value", row)?;
        append_timestamp(&mut ts_event_builder, &ts_event, row);
        append_timestamp(&mut ts_init_builder, &ts_init, row);
    }

    RecordBatch::try_new(
        Arc::new(price_update_schema()),
        vec![
            constant_string_column(instrument_id, len),
            Arc::new(value_builder.finish()),
            Arc::new(ts_event_builder.finish()),
            Arc::new(ts_init_builder.finish()),
        ],
    )
    .map_err(EncodingError::from)
}

fn funding_rate_schema() -> Schema {
    Schema::new(vec![
        utf8_field("instrument_id", false),
        float64_field("rate", false),
        Field::new("interval", DataType::UInt64, true),
        timestamp_field("next_funding_ns", true),
        timestamp_field("ts_event", false),
        timestamp_field("ts_init", false),
    ])
}

fn convert_funding_rates(batch: &RecordBatch) -> Result<RecordBatch, EncodingError> {
    let instrument_id_index = batch.schema().index_of("instrument_id")?;
    let instrument_id =
        extract_column_string(batch.columns(), "instrument_id", instrument_id_index)?;
    let rate_index = batch.schema().index_of("rate")?;
    let rate = extract_column_string(batch.columns(), "rate", rate_index)?;
    let interval = u64_col(batch, "interval")?;
    let next_funding_ns = nanos_col(batch, "next_funding_ns")?;
    let ts_event = nanos_col(batch, "ts_event")?;
    let ts_init = nanos_col(batch, "ts_init")?;
    let len = batch.num_rows();

    let mut instrument_id_builder = StringBuilder::new();
    let mut rate_builder = Float64Builder::with_capacity(len);
    let mut interval_builder = UInt64Builder::with_capacity(len);
    let mut next_funding_ns_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());
    let mut ts_event_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());
    let mut ts_init_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());

    for row in 0..len {
        instrument_id_builder.append_value(instrument_id.value(row));
        let value = Decimal::from_str(rate.value(row))
            .map_err(|e| EncodingError::ParseError("rate", e.to_string()))?;
        rate_builder.append_value(value.to_f64().unwrap_or(f64::NAN));
        append_optional_u64(&mut interval_builder, &interval, "interval", row)?;
        append_timestamp(&mut next_funding_ns_builder, &next_funding_ns, row);
        append_timestamp(&mut ts_event_builder, &ts_event, row);
        append_timestamp(&mut ts_init_builder, &ts_init, row);
    }

    RecordBatch::try_new(
        Arc::new(funding_rate_schema()),
        vec![
            Arc::new(instrument_id_builder.finish()),
            Arc::new(rate_builder.finish()),
            Arc::new(interval_builder.finish()),
            Arc::new(next_funding_ns_builder.finish()),
            Arc::new(ts_event_builder.finish()),
            Arc::new(ts_init_builder.finish()),
        ],
    )
    .map_err(EncodingError::from)
}

fn instrument_closes_schema() -> Schema {
    Schema::new(vec![
        utf8_field("instrument_id", false),
        float64_field("close_price", false),
        utf8_field("close_type", false),
        timestamp_field("ts_event", false),
        timestamp_field("ts_init", false),
    ])
}

fn convert_instrument_closes(
    metadata: &HashMap<String, String>,
    batch: &RecordBatch,
) -> Result<RecordBatch, EncodingError> {
    let instrument_id = instrument_id(metadata)?;
    let price_precision = parse_price_precision(metadata)?;
    let close_price = fixed_col(batch, "close_price")?;
    let close_type_index = batch.schema().index_of("close_type")?;
    let close_type = extract_column_string(batch.columns(), "close_type", close_type_index)?;
    let ts_event = nanos_col(batch, "ts_event")?;
    let ts_init = nanos_col(batch, "ts_init")?;
    let len = batch.num_rows();
    let mut close_price_builder = Float64Builder::with_capacity(len);
    let mut close_type_builder = StringBuilder::new();
    let mut ts_event_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());
    let mut ts_init_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());

    for row in 0..len {
        let close_type_value = close_type.value(row);
        let close_type = InstrumentCloseType::from_str(close_type_value).map_err(|e| {
            EncodingError::ParseError(stringify!(InstrumentCloseType), e.to_string())
        })?;
        append_price(
            &mut close_price_builder,
            &close_price,
            price_precision,
            "close_price",
            row,
        )?;
        close_type_builder.append_value(close_type.as_ref());
        append_timestamp(&mut ts_event_builder, &ts_event, row);
        append_timestamp(&mut ts_init_builder, &ts_init, row);
    }

    RecordBatch::try_new(
        Arc::new(instrument_closes_schema()),
        vec![
            constant_string_column(instrument_id, len),
            Arc::new(close_price_builder.finish()),
            Arc::new(close_type_builder.finish()),
            Arc::new(ts_event_builder.finish()),
            Arc::new(ts_init_builder.finish()),
        ],
    )
    .map_err(EncodingError::from)
}

fn option_greeks_schema() -> Schema {
    Schema::new(vec![
        utf8_field("instrument_id", false),
        float64_field("delta", false),
        float64_field("gamma", false),
        float64_field("vega", false),
        float64_field("theta", false),
        float64_field("rho", false),
        float64_field("mark_iv", true),
        float64_field("bid_iv", true),
        float64_field("ask_iv", true),
        float64_field("underlying_price", true),
        float64_field("open_interest", true),
        timestamp_field("ts_event", false),
        timestamp_field("ts_init", false),
        utf8_field("convention", false),
    ])
}

fn append_optional_f64(builder: &mut Float64Builder, values: &Float64Array, row: usize) {
    if values.is_null(row) {
        builder.append_null();
    } else {
        builder.append_value(values.value(row));
    }
}

fn convert_option_greeks(batch: &RecordBatch) -> Result<RecordBatch, EncodingError> {
    let instrument_id_index = batch.schema().index_of("instrument_id")?;
    let instrument_id =
        extract_column_string(batch.columns(), "instrument_id", instrument_id_index)?;
    let delta = f64_col(batch, "delta")?;
    let gamma = f64_col(batch, "gamma")?;
    let vega = f64_col(batch, "vega")?;
    let theta = f64_col(batch, "theta")?;
    let rho = f64_col(batch, "rho")?;
    let mark_iv = f64_col(batch, "mark_iv")?;
    let bid_iv = f64_col(batch, "bid_iv")?;
    let ask_iv = f64_col(batch, "ask_iv")?;
    let underlying_price = f64_col(batch, "underlying_price")?;
    let open_interest = f64_col(batch, "open_interest")?;
    let ts_event = nanos_col(batch, "ts_event")?;
    let ts_init = nanos_col(batch, "ts_init")?;
    let convention_index = batch.schema().index_of("convention")?;
    let convention = extract_column_string(batch.columns(), "convention", convention_index)?;
    let len = batch.num_rows();

    let mut instrument_id_builder = StringBuilder::new();
    let mut delta_builder = Float64Builder::with_capacity(len);
    let mut gamma_builder = Float64Builder::with_capacity(len);
    let mut vega_builder = Float64Builder::with_capacity(len);
    let mut theta_builder = Float64Builder::with_capacity(len);
    let mut rho_builder = Float64Builder::with_capacity(len);
    let mut mark_iv_builder = Float64Builder::with_capacity(len);
    let mut bid_iv_builder = Float64Builder::with_capacity(len);
    let mut ask_iv_builder = Float64Builder::with_capacity(len);
    let mut underlying_price_builder = Float64Builder::with_capacity(len);
    let mut open_interest_builder = Float64Builder::with_capacity(len);
    let mut ts_event_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());
    let mut ts_init_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());
    let mut convention_builder = StringBuilder::new();

    for row in 0..len {
        instrument_id_builder.append_value(instrument_id.value(row));
        delta_builder.append_value(delta.value(row));
        gamma_builder.append_value(gamma.value(row));
        vega_builder.append_value(vega.value(row));
        theta_builder.append_value(theta.value(row));
        rho_builder.append_value(rho.value(row));
        append_optional_f64(&mut mark_iv_builder, mark_iv, row);
        append_optional_f64(&mut bid_iv_builder, bid_iv, row);
        append_optional_f64(&mut ask_iv_builder, ask_iv, row);
        append_optional_f64(&mut underlying_price_builder, underlying_price, row);
        append_optional_f64(&mut open_interest_builder, open_interest, row);
        append_timestamp(&mut ts_event_builder, &ts_event, row);
        append_timestamp(&mut ts_init_builder, &ts_init, row);
        convention_builder.append_value(convention.value(row));
    }

    RecordBatch::try_new(
        Arc::new(option_greeks_schema()),
        vec![
            Arc::new(instrument_id_builder.finish()),
            Arc::new(delta_builder.finish()),
            Arc::new(gamma_builder.finish()),
            Arc::new(vega_builder.finish()),
            Arc::new(theta_builder.finish()),
            Arc::new(rho_builder.finish()),
            Arc::new(mark_iv_builder.finish()),
            Arc::new(bid_iv_builder.finish()),
            Arc::new(ask_iv_builder.finish()),
            Arc::new(underlying_price_builder.finish()),
            Arc::new(open_interest_builder.finish()),
            Arc::new(ts_event_builder.finish()),
            Arc::new(ts_init_builder.finish()),
            Arc::new(convention_builder.finish()),
        ],
    )
    .map_err(EncodingError::from)
}

fn instrument_status_schema() -> Schema {
    Schema::new(vec![
        utf8_field("instrument_id", false),
        utf8_field("action", false),
        timestamp_field("ts_event", false),
        timestamp_field("ts_init", false),
        utf8_field("reason", true),
        utf8_field("trading_event", true),
        Field::new("is_trading", DataType::Boolean, true),
        Field::new("is_quoting", DataType::Boolean, true),
        Field::new("is_short_sell_restricted", DataType::Boolean, true),
    ])
}

fn append_optional_string(
    builder: &mut StringBuilder,
    values: &super::StringColumnRef<'_>,
    row: usize,
    array: &dyn Array,
) {
    if array.is_null(row) {
        builder.append_null();
    } else {
        builder.append_value(values.value(row));
    }
}

fn append_optional_bool(
    builder: &mut arrow::array::BooleanBuilder,
    values: &BooleanArray,
    row: usize,
) {
    if values.is_null(row) {
        builder.append_null();
    } else {
        builder.append_value(values.value(row));
    }
}

fn append_optional_u64(
    builder: &mut UInt64Builder,
    values: &U64ColumnRef<'_>,
    field: &'static str,
    row: usize,
) -> Result<(), EncodingError> {
    if values.is_null(row) {
        builder.append_null();
    } else {
        builder.append_value(u64_value(values, row, field)?);
    }
    Ok(())
}

fn convert_instrument_status(batch: &RecordBatch) -> Result<RecordBatch, EncodingError> {
    let instrument_id_index = batch.schema().index_of("instrument_id")?;
    let instrument_id =
        extract_column_string(batch.columns(), "instrument_id", instrument_id_index)?;
    let action_index = batch.schema().index_of("action")?;
    let action = extract_column_string(batch.columns(), "action", action_index)?;
    let ts_event = nanos_col(batch, "ts_event")?;
    let ts_init = nanos_col(batch, "ts_init")?;
    let reason_index = batch.schema().index_of("reason")?;
    let reason = extract_column_string(batch.columns(), "reason", reason_index)?;
    let trading_event_index = batch.schema().index_of("trading_event")?;
    let trading_event =
        extract_column_string(batch.columns(), "trading_event", trading_event_index)?;
    let is_trading = bool_col(batch, "is_trading")?;
    let is_quoting = bool_col(batch, "is_quoting")?;
    let is_short_sell_restricted = bool_col(batch, "is_short_sell_restricted")?;
    let len = batch.num_rows();

    let mut instrument_id_builder = StringBuilder::new();
    let mut action_builder = StringBuilder::new();
    let mut ts_event_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());
    let mut ts_init_builder =
        TimestampNanosecondBuilder::with_capacity(len).with_data_type(timestamp_data_type());
    let mut reason_builder = StringBuilder::new();
    let mut trading_event_builder = StringBuilder::new();
    let mut is_trading_builder = arrow::array::BooleanBuilder::with_capacity(len);
    let mut is_quoting_builder = arrow::array::BooleanBuilder::with_capacity(len);
    let mut is_short_sell_restricted_builder = arrow::array::BooleanBuilder::with_capacity(len);

    for row in 0..len {
        instrument_id_builder.append_value(instrument_id.value(row));
        action_builder.append_value(action.value(row));
        append_timestamp(&mut ts_event_builder, &ts_event, row);
        append_timestamp(&mut ts_init_builder, &ts_init, row);
        append_optional_string(
            &mut reason_builder,
            &reason,
            row,
            batch.column(reason_index).as_ref(),
        );
        append_optional_string(
            &mut trading_event_builder,
            &trading_event,
            row,
            batch.column(trading_event_index).as_ref(),
        );
        append_optional_bool(&mut is_trading_builder, is_trading, row);
        append_optional_bool(&mut is_quoting_builder, is_quoting, row);
        append_optional_bool(
            &mut is_short_sell_restricted_builder,
            is_short_sell_restricted,
            row,
        );
    }

    RecordBatch::try_new(
        Arc::new(instrument_status_schema()),
        vec![
            Arc::new(instrument_id_builder.finish()),
            Arc::new(action_builder.finish()),
            Arc::new(ts_event_builder.finish()),
            Arc::new(ts_init_builder.finish()),
            Arc::new(reason_builder.finish()),
            Arc::new(trading_event_builder.finish()),
            Arc::new(is_trading_builder.finish()),
            Arc::new(is_quoting_builder.finish()),
            Arc::new(is_short_sell_restricted_builder.finish()),
        ],
    )
    .map_err(EncodingError::from)
}

/// Converts custom data to display types.
///
/// For a registered custom type, the output uses its canonical registered schema metadata.
/// User-provided `DataType` metadata keys are therefore not retained in the display schema.
fn convert_custom(batch: &RecordBatch) -> Result<RecordBatch, EncodingError> {
    let batch_schema = batch.schema();
    let metadata = batch_schema.metadata();
    let mut fields = Vec::with_capacity(batch.num_columns());
    let mut columns = Vec::with_capacity(batch.num_columns());

    for (field, column) in batch_schema.fields().iter().zip(batch.columns()) {
        if matches!(field.name().as_str(), "ts_event" | "ts_init") {
            let name = if field.name() == "ts_event" {
                "ts_event"
            } else {
                "ts_init"
            };
            let values = nanos_col(batch, name)?;
            fields.push(Arc::new(timestamp_field_from(field)));
            columns.push(timestamp_array_from_nanos(&values));
        } else if let Some((precision, is_price)) = custom_fixed_precision(field.name(), metadata)
            && matches!(
                field.data_type(),
                DataType::Decimal128(_, _)
                    | DataType::FixedSizeBinary(_)
                    | DataType::Binary
                    | DataType::BinaryView
            )
        {
            if field.data_type() != &fixed_decimal_data_type() {
                return Err(EncodingError::ParseError(
                    "custom",
                    format!(
                        "{} must use Decimal128(38, 16); migrate legacy data before display",
                        field.name()
                    ),
                ));
            }
            let values = FixedPrecisionColumn(
                column
                    .as_any()
                    .downcast_ref::<Decimal128Array>()
                    .expect("decimal field type checked above"),
            );
            fields.push(Arc::new(Field::new(
                field.name(),
                DataType::Float64,
                field.is_nullable(),
            )));
            columns.push(fixed_precision_field_to_f64(&values, precision, is_price)?);
        } else if field.data_type() == &DataType::Utf8View {
            fields.push(Arc::new(Field::new(
                field.name(),
                DataType::Utf8,
                field.is_nullable(),
            )));
            columns.push(cast(column.as_ref(), &DataType::Utf8)?);
        } else {
            fields.push(field.clone());
            columns.push(column.clone());
        }
    }

    let metadata = if let Some(type_name) = metadata.get("type_name")
        && get_arrow_schema(type_name).is_some()
    {
        let canonical_metadata = HashMap::from([("type_name".to_string(), type_name.clone())]);
        let canonical_schema = super::schema_with_identifier_column(
            &CustomDataDecoder::get_schema(Some(canonical_metadata)),
        );

        for field in &mut fields {
            if let Ok(canonical_field) = canonical_schema.field_with_name(field.name()) {
                *field = Arc::new(
                    field
                        .as_ref()
                        .clone()
                        .with_nullable(canonical_field.is_nullable()),
                );
            }
        }
        canonical_schema.metadata().clone()
    } else {
        batch_schema.metadata().clone()
    };
    let schema = Schema::new_with_metadata(fields, metadata);
    RecordBatch::try_new(Arc::new(schema), columns).map_err(EncodingError::from)
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, str::FromStr};

    use arrow::{
        array::{
            Array, FixedSizeBinaryArray, FixedSizeListArray, Float64Array, StringArray,
            StringViewArray, UInt64Array,
        },
        datatypes::{Field, TimeUnit},
    };
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::{
            FundingRateUpdate, OrderBookDepth, QuoteTick, ensure_arrow_registered,
            stubs::stub_depth10,
        },
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;
    use crate::arrow::{
        EncodeToRecordBatch, KEY_INSTRUMENT_ID, KEY_PRICE_PRECISION, KEY_SIZE_PRECISION,
    };

    #[rstest]
    fn test_catalog_record_batch_to_display_converts_quote_prices_and_metadata() {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "5".to_string()),
            (KEY_SIZE_PRECISION.to_string(), "0".to_string()),
        ]);
        let quote = QuoteTick {
            instrument_id,
            bid_price: Price::from("1.00001"),
            ask_price: Price::from("1.00003"),
            bid_size: Quantity::from(1_000),
            ask_size: Quantity::from(2_000),
            ts_event: 10.into(),
            ts_init: 11.into(),
        };
        let raw_batch = QuoteTick::encode_batch(&metadata, &[quote]).unwrap();

        let display_batch =
            catalog_record_batch_to_display(&NautilusDataType::QuoteTick, &metadata, &raw_batch)
                .unwrap();

        let mut expected_fields = quotes_schema().fields().to_vec();
        expected_fields.push(Arc::new(utf8_field(KEY_IDENTIFIER, true)));
        assert_eq!(
            display_batch.schema(),
            Arc::new(Schema::new(expected_fields))
        );
        let instrument_ids = display_batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let bid_prices = display_batch
            .column(1)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let ask_sizes = display_batch
            .column(4)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let ts_init = display_batch
            .column(6)
            .as_any()
            .downcast_ref::<arrow::array::TimestampNanosecondArray>()
            .unwrap();
        let identifiers = display_batch
            .column(7)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();

        assert_eq!(instrument_ids.value(0), "AUD/USD.SIM");
        assert_eq!(bid_prices.value(0), 1.00001);
        assert_eq!(ask_sizes.value(0), 2_000.0);
        assert_eq!(ts_init.value(0), 11);
        assert_eq!(identifiers.value(0), "AUD/USD.SIM");
    }

    #[rstest]
    #[case(1)]
    #[case(u64::MAX)]
    fn test_catalog_record_batch_to_display_rejects_legacy_timestamps(#[case] timestamp: u64) {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), instrument_id.to_string()),
            (KEY_PRICE_PRECISION.to_string(), "5".to_string()),
            (KEY_SIZE_PRECISION.to_string(), "0".to_string()),
        ]);
        let quote = QuoteTick {
            instrument_id,
            bid_price: Price::from("1.00001"),
            ask_price: Price::from("1.00003"),
            bid_size: Quantity::from(1_000),
            ask_size: Quantity::from(2_000),
            ts_event: 10.into(),
            ts_init: 11.into(),
        };
        let raw_batch = QuoteTick::encode_batch(&metadata, &[quote]).unwrap();
        let mut fields = raw_batch.schema().fields().to_vec();
        let ts_init_index = raw_batch.schema().index_of("ts_init").unwrap();
        fields[ts_init_index] = Arc::new(Field::new("ts_init", DataType::UInt64, false));
        let mut columns = raw_batch.columns().to_vec();
        columns[ts_init_index] = Arc::new(UInt64Array::from(vec![timestamp]));
        let raw_batch = RecordBatch::try_new(
            Arc::new(Schema::new_with_metadata(fields, metadata.clone())),
            columns,
        )
        .unwrap();

        let error =
            catalog_record_batch_to_display(&NautilusDataType::QuoteTick, &metadata, &raw_batch)
                .unwrap_err();

        assert!(matches!(
            error,
            EncodingError::InvalidColumnType(
                "ts_init",
                _,
                DataType::Timestamp(TimeUnit::Nanosecond, Some(timezone)),
                DataType::UInt64
            ) if timezone.as_ref() == "UTC"
        ));
    }

    #[rstest]
    #[case(38, 15)]
    #[case(37, 16)]
    fn test_catalog_display_rejects_noncanonical_decimal_schema(
        #[case] precision: u8,
        #[case] scale: i8,
    ) {
        let quote = QuoteTick::new(
            InstrumentId::from("AUD/USD.SIM"),
            Price::from("1.25"),
            Price::from("1.50"),
            Quantity::from(10),
            Quantity::from(20),
            10.into(),
            11.into(),
        );
        let metadata = quote.metadata();
        let raw = QuoteTick::encode_batch(&metadata, &[quote]).unwrap();
        let index = raw.schema().index_of("bid_price").unwrap();
        let mut fields = raw.schema().fields().to_vec();
        let mut columns = raw.columns().to_vec();
        let values = columns[index]
            .as_any()
            .downcast_ref::<Decimal128Array>()
            .unwrap()
            .clone()
            .with_precision_and_scale(precision, scale)
            .unwrap();
        fields[index] = Arc::new(
            fields[index]
                .as_ref()
                .clone()
                .with_data_type(values.data_type().clone()),
        );
        columns[index] = Arc::new(values);
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new_with_metadata(fields, metadata.clone())),
            columns,
        )
        .unwrap();
        let error =
            catalog_record_batch_to_display(&NautilusDataType::QuoteTick, &metadata, &batch)
                .unwrap_err();

        assert!(
            matches!(error, EncodingError::InvalidColumnType("bid_price", i, DataType::Decimal128(38, 16), actual) if i == index && actual == DataType::Decimal128(precision, scale))
        );
    }

    #[rstest]
    #[case(0)]
    #[case(5)]
    #[case(10)]
    #[case(25)]
    fn test_depth_display_preserves_all_levels(#[case] levels: usize) {
        let mut depth = stub_depth10();
        depth.bids.resize(levels, depth.bids[0]);
        depth.bid_counts.resize(levels, 0);
        depth.asks.clear();
        depth.ask_counts.clear();
        for (i, (order, count)) in depth.bids.iter_mut().zip(&mut depth.bid_counts).enumerate() {
            order.price = format!("{}.25", 100 + i).parse().unwrap();
            order.size = format!("{}.5", 200 + i).parse().unwrap();
            order.order_id = u64::MAX - u64::try_from(i).unwrap();
            *count = 300 + u32::try_from(i).unwrap();
        }
        let metadata = depth.metadata();
        let raw = OrderBookDepth::encode_batch(&metadata, &[depth]).unwrap();
        let display =
            catalog_record_batch_to_display(&NautilusDataType::OrderBookDepth, &metadata, &raw)
                .unwrap();
        let list = display
            .column_by_name("bids")
            .unwrap()
            .as_any()
            .downcast_ref::<ListArray>()
            .unwrap();
        let values = list.value(0);
        let structs = values.as_any().downcast_ref::<StructArray>().unwrap();
        let prices = structs
            .column_by_name("price")
            .unwrap()
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let sizes = structs
            .column_by_name("size")
            .unwrap()
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let counts = structs
            .column_by_name("count")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt32Array>()
            .unwrap();
        let ids = structs
            .column_by_name("order_id")
            .unwrap()
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let asks = display
            .column_by_name("asks")
            .unwrap()
            .as_any()
            .downcast_ref::<ListArray>()
            .unwrap();
        let empty = catalog_record_batch_to_display(
            &NautilusDataType::OrderBookDepth,
            &metadata,
            &raw.slice(0, 0),
        )
        .unwrap();

        assert_eq!(
            display.schema().as_ref(),
            &crate::arrow::schema_with_identifier_column(&depth_schema())
        );
        assert_eq!(empty.schema(), display.schema());
        assert_eq!(empty.num_rows(), 0);
        assert_eq!(display.num_rows(), 1);
        assert_eq!(list.value_length(0), i32::try_from(levels).unwrap());
        assert_eq!(asks.value_length(0), 0);
        assert!(!asks.is_null(0));

        for i in 0..levels {
            assert_eq!(
                prices.value(i),
                f64::from(u32::try_from(100 + i).unwrap()) + 0.25
            );
            assert_eq!(
                sizes.value(i),
                f64::from(u32::try_from(200 + i).unwrap()) + 0.5
            );
            assert_eq!(counts.value(i), 300 + u32::try_from(i).unwrap());
            assert_eq!(ids.value(i), u64::MAX - u64::try_from(i).unwrap());
        }
    }

    #[rstest]
    #[case(false, false)]
    #[case(false, true)]
    #[case(true, false)]
    #[case(true, true)]
    fn test_depth_display_rejects_legacy_shapes(#[case] lists: bool, #[case] with_ids: bool) {
        let depth = stub_depth10();
        let metadata = depth.metadata();
        let raw = OrderBookDepth::encode_batch(&metadata, &[depth]).unwrap();
        let mut columns = Vec::<ArrayRef>::new();
        let mut fields = Vec::<Field>::new();

        for (side, prefix) in [("bids", "bid"), ("asks", "ask")] {
            let list = raw
                .column_by_name(side)
                .unwrap()
                .as_any()
                .downcast_ref::<ListArray>()
                .unwrap();
            let levels = list
                .values()
                .as_any()
                .downcast_ref::<StructArray>()
                .unwrap();

            for name in ["price", "size", "count", "order_id"] {
                if name == "order_id" && !with_ids {
                    continue;
                }
                let values = levels.column_by_name(name).unwrap();
                if lists {
                    let item = Arc::new(Field::new("item", values.data_type().clone(), true));
                    let column =
                        FixedSizeListArray::try_new(item, 10, values.clone(), None).unwrap();
                    fields.push(Field::new(
                        format!("{prefix}_{name}"),
                        column.data_type().clone(),
                        false,
                    ));
                    columns.push(Arc::new(column));
                } else {
                    for i in 0..10 {
                        fields.push(Field::new(
                            format!("{prefix}_{name}_{i}"),
                            values.data_type().clone(),
                            true,
                        ));
                        columns.push(values.slice(i, 1));
                    }
                }
            }
        }

        for name in ["flags", "sequence", "ts_event", "ts_init"] {
            fields.push(raw.schema().field_with_name(name).unwrap().clone());
            columns.push(raw.column_by_name(name).unwrap().clone());
        }
        let legacy = RecordBatch::try_new(
            Arc::new(Schema::new_with_metadata(fields, metadata.clone())),
            columns,
        )
        .unwrap();
        let error =
            catalog_record_batch_to_display(&NautilusDataType::OrderBookDepth, &metadata, &legacy)
                .unwrap_err();
        assert!(
            matches!(error, EncodingError::ParseError("bids", ref message) if message == "expected List<Struct>")
        );
    }

    #[rstest]
    fn test_catalog_display_schema_uses_data_type() {
        let schema = catalog_display_schema(&NautilusDataType::QuoteTick).unwrap();

        assert_eq!(schema.fields()[0].name(), "instrument_id");
        assert_eq!(schema.fields()[1].data_type(), &DataType::Float64);
        assert_eq!(schema.fields().last().unwrap().name(), "ts_init");
    }

    #[rstest]
    fn test_custom_catalog_display_schema_uses_registered_schema() {
        let type_name = "CatalogDisplaySchemaCustom";
        let schema = Schema::new(vec![
            Field::new("value", DataType::Float64, false),
            Field::new(
                "ts_init",
                DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into())),
                false,
            ),
        ]);
        ensure_arrow_registered(
            type_name,
            Arc::new(schema),
            Box::new(|_| unreachable!()),
            Box::new(|_, _| unreachable!()),
        )
        .unwrap();

        let data_type = NautilusDataType::Custom {
            type_name: type_name.to_string(),
        };
        let raw_schema = catalog_raw_schema(&data_type).unwrap();
        let schema = catalog_display_schema(&data_type).unwrap();
        let fields = schema
            .fields()
            .iter()
            .map(|field| (field.name().as_str(), field.data_type()))
            .collect::<Vec<_>>();

        assert_eq!(
            fields,
            [
                ("value", &DataType::Float64),
                (
                    "ts_init",
                    &DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into()))
                ),
                ("data_type", &DataType::Utf8),
                ("identifier", &DataType::Utf8),
            ]
        );
        assert_eq!(raw_schema.fields().last().unwrap().name(), "identifier");
    }

    #[rstest]
    fn test_custom_catalog_schema_rejects_unregistered_type() {
        let data_type = NautilusDataType::Custom {
            type_name: "UnregisteredCatalogDisplayCustom".to_string(),
        };

        assert!(catalog_raw_schema(&data_type).is_err());
        assert!(catalog_display_schema(&data_type).is_err());
    }

    #[rstest]
    fn test_custom_display_normalizes_utf8_view_to_utf8() {
        let batch = RecordBatch::try_new(
            Arc::new(Schema::new(vec![Field::new(
                "label",
                DataType::Utf8View,
                false,
            )])),
            vec![Arc::new(StringViewArray::from(vec!["value"]))],
        )
        .unwrap();
        let display = catalog_record_batch_to_display(
            &NautilusDataType::Custom {
                type_name: "ViewCustom".to_string(),
            },
            batch.schema().metadata(),
            &batch,
        )
        .unwrap();

        assert_eq!(display.schema().field(0).data_type(), &DataType::Utf8);
        assert_eq!(
            display
                .column(0)
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap()
                .value(0),
            "value"
        );
    }

    #[rstest]
    fn test_catalog_record_batch_to_display_converts_funding_rates() {
        let instrument_id = InstrumentId::from("BTCUSDT-PERP.BINANCE");
        let funding_rate = FundingRateUpdate::new(
            instrument_id,
            Decimal::from_str("0.000125").unwrap(),
            Some(480),
            None,
            UnixNanos::from(10),
            UnixNanos::from(11),
        );
        let metadata = funding_rate.metadata();
        let raw_batch = FundingRateUpdate::encode_batch(&metadata, &[funding_rate]).unwrap();

        let display_batch = catalog_record_batch_to_display(
            &NautilusDataType::FundingRateUpdate,
            &metadata,
            &raw_batch,
        )
        .unwrap();

        let mut expected_fields = funding_rate_schema().fields().to_vec();
        expected_fields.push(Arc::new(utf8_field(KEY_IDENTIFIER, true)));
        assert_eq!(
            display_batch.schema(),
            Arc::new(Schema::new(expected_fields))
        );
        let instrument_ids = display_batch
            .column(0)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();
        let rates = display_batch
            .column(1)
            .as_any()
            .downcast_ref::<Float64Array>()
            .unwrap();
        let intervals = display_batch
            .column(2)
            .as_any()
            .downcast_ref::<UInt64Array>()
            .unwrap();
        let next_funding_ns = display_batch
            .column(3)
            .as_any()
            .downcast_ref::<TimestampNanosecondArray>()
            .unwrap();
        let identifiers = display_batch
            .column(6)
            .as_any()
            .downcast_ref::<StringArray>()
            .unwrap();

        assert_eq!(instrument_ids.value(0), "BTCUSDT-PERP.BINANCE");
        assert_eq!(rates.value(0), 0.000_125);
        assert_eq!(intervals.value(0), 480);
        assert!(next_funding_ns.is_null(0));
        assert_eq!(identifiers.value(0), "BTCUSDT-PERP.BINANCE");
    }

    #[rstest]
    fn test_catalog_record_batch_to_display_rejects_precision_mismatch() {
        let metadata = HashMap::from([
            (KEY_INSTRUMENT_ID.to_string(), "AUD/USD.SIM".to_string()),
            (KEY_PRICE_PRECISION.to_string(), "5".to_string()),
            (KEY_SIZE_PRECISION.to_string(), "0".to_string()),
        ]);
        let schema = Arc::new(Schema::new_with_metadata(
            vec![
                Field::new("bid_price", DataType::FixedSizeBinary(1), false),
                Field::new("ask_price", DataType::FixedSizeBinary(1), false),
                Field::new("bid_size", DataType::FixedSizeBinary(1), false),
                Field::new("ask_size", DataType::FixedSizeBinary(1), false),
                Field::new("ts_event", DataType::UInt64, false),
                Field::new("ts_init", DataType::UInt64, false),
            ],
            metadata.clone(),
        ));
        let batch = RecordBatch::try_new(
            schema,
            vec![
                Arc::new(
                    FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                        vec![Some([0_u8].as_slice())].into_iter(),
                        1,
                    )
                    .unwrap(),
                ),
                Arc::new(
                    FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                        vec![Some([0_u8].as_slice())].into_iter(),
                        1,
                    )
                    .unwrap(),
                ),
                Arc::new(
                    FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                        vec![Some([0_u8].as_slice())].into_iter(),
                        1,
                    )
                    .unwrap(),
                ),
                Arc::new(
                    FixedSizeBinaryArray::try_from_sparse_iter_with_size(
                        vec![Some([0_u8].as_slice())].into_iter(),
                        1,
                    )
                    .unwrap(),
                ),
                Arc::new(UInt64Array::from(vec![1])),
                Arc::new(UInt64Array::from(vec![1])),
            ],
        )
        .unwrap();

        let error =
            catalog_record_batch_to_display(&NautilusDataType::QuoteTick, &metadata, &batch)
                .unwrap_err();

        assert!(matches!(
            error,
            EncodingError::InvalidColumnType(
                "bid_price",
                _,
                DataType::Decimal128(38, 16),
                DataType::FixedSizeBinary(1)
            )
        ));
    }
}
