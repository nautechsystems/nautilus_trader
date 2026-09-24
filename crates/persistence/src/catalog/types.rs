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

//! Catalog query values, type prefixes, and shared type-specific filtering.

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    fmt::Display,
};

use nautilus_core::{Params, UnixNanos, string::conversions::to_snake_case};
use nautilus_model::{
    data::{
        Bar, FundingRateUpdate, HasTsInit, IndexPriceUpdate, InstrumentClose, InstrumentStatus,
        MarkPriceUpdate, NautilusDataType, NautilusRecordType, OptionGreeks, OrderBookDelta,
        OrderBookDepth, QuoteTick, TradeTick,
    },
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied,
        OrderEmulated, OrderExpired, OrderFillVoided, OrderFilled, OrderInitialized,
        OrderModifyRejected, OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderReleased,
        OrderSnapshot, OrderSubmitted, OrderTriggered, OrderUpdated, PortfolioSnapshot,
        PositionAdjusted, PositionChanged, PositionClosed, PositionOpened, PositionSnapshot,
    },
    instruments::{Instrument, InstrumentAny, NautilusInstrumentType},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
};

use super::traits::{NautilusDataTypePrefix, NautilusRecordTypePrefix};
use crate::common::paths::CatalogPathPrefix;

/// Identifies the stored family a catalog operation targets: a data type, a record type, or an
/// instrument class.
///
/// `Data(Instrument)` is the aggregate instrument family: it addresses every instrument class the
/// backend stores, and each backend resolves the per-class fan-out itself. `Instrument(class)`
/// addresses one class. Every value of the three families is a valid selector, so `From` and
/// `Into` are the only construction paths.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum CatalogDataType {
    Data(NautilusDataType),
    Record(NautilusRecordType),
    Instrument(NautilusInstrumentType),
}

impl Display for CatalogDataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Data(data_type) => Display::fmt(data_type, f),
            Self::Record(record_type) => Display::fmt(record_type, f),
            Self::Instrument(instrument_type) => Display::fmt(instrument_type, f),
        }
    }
}

impl From<NautilusDataType> for CatalogDataType {
    fn from(value: NautilusDataType) -> Self {
        Self::Data(value)
    }
}

impl From<NautilusRecordType> for CatalogDataType {
    fn from(value: NautilusRecordType) -> Self {
        Self::Record(value)
    }
}

impl From<NautilusInstrumentType> for CatalogDataType {
    fn from(value: NautilusInstrumentType) -> Self {
        Self::Instrument(value)
    }
}

/// Backend-native point in catalog history used by a query or restore.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CatalogAsOf {
    /// Read the current catalog head.
    #[default]
    Latest,
    /// Read the last commit at or before this UTC timestamp.
    Timestamp(UnixNanos),
    /// Read the backend-native table version or catalog snapshot.
    Version(i64),
}

/// Common catalog commit-history row, ordered newest first by callers.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogCommit {
    /// Backend-native table version or catalog snapshot id.
    pub version: i64,
    /// Commit timestamp in nanoseconds since the Unix epoch.
    pub timestamp: UnixNanos,
    /// Writer or higher-level source recorded by the commit.
    pub source: String,
    /// Backend operation recorded by the commit.
    pub operation: String,
}

/// Filters selecting catalog data rows.
///
/// Data queries share this shape across the reader trait, the catalog worker, and the PyO3
/// bindings, so a filter can be added without changing every signature and neither `start` and
/// `end` nor `where_clause` and `params` can be transposed at a call site.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogQuery {
    /// The data family to read.
    pub data_type: NautilusDataType,
    /// Row identifiers to match, or every identifier when `None`.
    pub identifiers: Option<Vec<String>>,
    /// Inclusive first `ts_init` to read.
    pub start: Option<UnixNanos>,
    /// Inclusive last `ts_init` to read.
    pub end: Option<UnixNanos>,
    /// Backend SQL predicate applied alongside the identifier and range filters.
    pub where_clause: Option<String>,
    /// Backend-specific query parameters.
    pub params: Option<Params>,
    /// Catalog point to read.
    pub as_of: CatalogAsOf,
    /// Instrument class to read when `data_type` is the instrument family, or every class when
    /// `None`.
    pub instrument_type: Option<NautilusInstrumentType>,
}

impl CatalogQuery {
    /// Creates an unfiltered query for `data_type`.
    #[must_use]
    pub const fn new(data_type: NautilusDataType) -> Self {
        Self {
            data_type,
            identifiers: None,
            start: None,
            end: None,
            where_clause: None,
            params: None,
            as_of: CatalogAsOf::Latest,
            instrument_type: None,
        }
    }

    /// Returns the query restricted to one instrument class.
    #[must_use]
    pub const fn with_instrument_type(
        mut self,
        instrument_type: Option<NautilusInstrumentType>,
    ) -> Self {
        self.instrument_type = instrument_type;
        self
    }

    /// Returns the query restricted to `identifiers`.
    #[must_use]
    pub fn with_identifiers(mut self, identifiers: Option<Vec<String>>) -> Self {
        self.identifiers = identifiers;
        self
    }

    /// Returns the query restricted to the inclusive `[start, end]` range.
    #[must_use]
    pub const fn with_range(mut self, start: Option<UnixNanos>, end: Option<UnixNanos>) -> Self {
        self.start = start;
        self.end = end;
        self
    }

    /// Returns the query with an additional backend SQL predicate.
    #[must_use]
    pub fn with_where_clause(mut self, where_clause: Option<String>) -> Self {
        self.where_clause = where_clause;
        self
    }

    /// Returns the query with backend-specific parameters.
    #[must_use]
    pub fn with_params(mut self, params: Option<Params>) -> Self {
        self.params = params;
        self
    }

    /// Returns the query pinned to a catalog point.
    #[must_use]
    pub const fn with_as_of(mut self, as_of: CatalogAsOf) -> Self {
        self.as_of = as_of;
        self
    }
}

/// Filters selecting catalog record rows.
///
/// Records are keyed by a single identifier because a record family stores one identifier column.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogRecordQuery {
    /// The record family to read.
    pub record_type: NautilusRecordType,
    /// Row identifier to match, or every identifier when `None`.
    pub identifier: Option<String>,
    /// Inclusive first `ts_init` to read.
    pub start: Option<UnixNanos>,
    /// Inclusive last `ts_init` to read.
    pub end: Option<UnixNanos>,
    /// Backend SQL predicate applied alongside the identifier and range filters.
    pub where_clause: Option<String>,
    /// Backend-specific query parameters.
    pub params: Option<Params>,
    /// Catalog point to read.
    pub as_of: CatalogAsOf,
}

impl CatalogRecordQuery {
    /// Creates an unfiltered query for `record_type`.
    #[must_use]
    pub const fn new(record_type: NautilusRecordType) -> Self {
        Self {
            record_type,
            identifier: None,
            start: None,
            end: None,
            where_clause: None,
            params: None,
            as_of: CatalogAsOf::Latest,
        }
    }

    /// Returns the query restricted to `identifier`.
    #[must_use]
    pub fn with_identifier(mut self, identifier: Option<String>) -> Self {
        self.identifier = identifier;
        self
    }

    /// Returns the query restricted to the inclusive `[start, end]` range.
    #[must_use]
    pub const fn with_range(mut self, start: Option<UnixNanos>, end: Option<UnixNanos>) -> Self {
        self.start = start;
        self.end = end;
        self
    }

    /// Returns the query with an additional backend SQL predicate.
    #[must_use]
    pub fn with_where_clause(mut self, where_clause: Option<String>) -> Self {
        self.where_clause = where_clause;
        self
    }

    /// Returns the query with backend-specific parameters.
    #[must_use]
    pub fn with_params(mut self, params: Option<Params>) -> Self {
        self.params = params;
        self
    }

    /// Returns the query pinned to a catalog point.
    #[must_use]
    pub const fn with_as_of(mut self, as_of: CatalogAsOf) -> Self {
        self.as_of = as_of;
        self
    }
}

/// Filters selecting catalog instrument definitions.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct CatalogInstrumentQuery {
    /// Instrument ids to match, or every instrument when `None`.
    pub instrument_ids: Option<Vec<String>>,
    /// Inclusive first `ts_init` to read.
    pub start: Option<UnixNanos>,
    /// Inclusive last `ts_init` to read.
    pub end: Option<UnixNanos>,
    /// Backend SQL predicate applied alongside the identifier and range filters.
    pub where_clause: Option<String>,
    /// Instrument class to read, or every class when `None`.
    pub instrument_type: Option<NautilusInstrumentType>,
}

impl CatalogInstrumentQuery {
    /// Creates an unfiltered instrument query.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            instrument_ids: None,
            start: None,
            end: None,
            where_clause: None,
            instrument_type: None,
        }
    }

    /// Returns the query restricted to `instrument_ids`.
    #[must_use]
    pub fn with_instrument_ids(mut self, instrument_ids: Option<Vec<String>>) -> Self {
        self.instrument_ids = instrument_ids;
        self
    }

    /// Returns the query restricted to the inclusive `[start, end]` range.
    #[must_use]
    pub const fn with_range(mut self, start: Option<UnixNanos>, end: Option<UnixNanos>) -> Self {
        self.start = start;
        self.end = end;
        self
    }

    /// Returns the query with an additional backend SQL predicate.
    #[must_use]
    pub fn with_where_clause(mut self, where_clause: Option<String>) -> Self {
        self.where_clause = where_clause;
        self
    }

    /// Returns the query restricted to one instrument class.
    #[must_use]
    pub const fn with_instrument_type(
        mut self,
        instrument_type: Option<NautilusInstrumentType>,
    ) -> Self {
        self.instrument_type = instrument_type;
        self
    }
}

/// Returns the Parquet catalog prefix for a data type.
#[must_use]
pub fn parquet_data_path_prefix(data_type: &NautilusDataType) -> Cow<'static, str> {
    data_path_prefix(data_type)
}

macro_rules! data_path_prefix_match {
    (
        ($data_type:ident);
        $(($variant:ident, $type:ident, $data:ident, $batch:ident, $prefix:literal)),+ $(,)?
    ) => {
        match $data_type {
            $(NautilusDataType::$variant => Cow::Borrowed($prefix),)+
            NautilusDataType::Custom { type_name } => Cow::Owned(format!("custom/{type_name}")),
            #[cfg(feature = "defi")]
            NautilusDataType::Defi => Cow::Borrowed("defi"),
        }
    };
}

/// Returns the shared-table catalog prefix for a data type.
#[must_use]
pub fn data_path_prefix(data_type: &NautilusDataType) -> Cow<'static, str> {
    nautilus_model::for_each_data_type!(data_path_prefix_match, data_type)
}

/// Parses a catalog data type name or path prefix into its semantic data type.
///
/// This is the single entry point for catalog callers, including the PyO3 bindings. Storage path
/// prefixes stay here because they are catalog names rather than model names; every other spelling
/// resolves through [`NautilusDataType`]'s own parser.
///
/// # Errors
///
/// Returns an error if `type_name` is not a known catalog data type name or path prefix.
pub fn data_type_from_data_path_prefix(type_name: &str) -> anyhow::Result<NautilusDataType> {
    if let Some(custom_type_name) = type_name.strip_prefix("custom/") {
        return Ok(NautilusDataType::Custom {
            type_name: custom_type_name.to_string(),
        });
    }

    if matches!(type_name, "custom" | "CustomData") {
        anyhow::bail!("custom data queries require custom/<type_name> or Custom:<type_name>");
    }

    if type_name == "order_book_depth10" {
        return Ok(NautilusDataType::OrderBookDepth);
    }

    type_name.parse()
}

pub const INSTRUMENT_PATH_PREFIXES: &[&str] = &[
    "betting_instrument",
    "binary_option",
    "cfd",
    "commodity",
    "crypto_future",
    "crypto_futures_spread",
    "crypto_option",
    "crypto_option_spread",
    "crypto_perpetual",
    "currency_pair",
    "equity",
    "futures_contract",
    "futures_spread",
    "index_instrument",
    "option_contract",
    "option_spread",
    "perpetual_contract",
    "tokenized_asset",
];

/// Returns the catalog folder prefix for an instrument type.
#[must_use]
pub const fn instrument_path_prefix(instrument_type: &NautilusInstrumentType) -> &'static str {
    match instrument_type {
        NautilusInstrumentType::BettingInstrument => "betting_instrument",
        NautilusInstrumentType::BinaryOption => "binary_option",
        NautilusInstrumentType::Cfd => "cfd",
        NautilusInstrumentType::Commodity => "commodity",
        NautilusInstrumentType::CryptoFuture => "crypto_future",
        NautilusInstrumentType::CryptoFuturesSpread => "crypto_futures_spread",
        NautilusInstrumentType::CryptoOption => "crypto_option",
        NautilusInstrumentType::CryptoOptionSpread => "crypto_option_spread",
        NautilusInstrumentType::CryptoPerpetual => "crypto_perpetual",
        NautilusInstrumentType::CurrencyPair => "currency_pair",
        NautilusInstrumentType::Equity => "equity",
        NautilusInstrumentType::FuturesContract => "futures_contract",
        NautilusInstrumentType::FuturesSpread => "futures_spread",
        NautilusInstrumentType::IndexInstrument => "index_instrument",
        NautilusInstrumentType::OptionContract => "option_contract",
        NautilusInstrumentType::OptionSpread => "option_spread",
        NautilusInstrumentType::PerpetualContract => "perpetual_contract",
        NautilusInstrumentType::TokenizedAsset => "tokenized_asset",
    }
}

/// Returns the semantic instrument type for an instrument enum value.
#[must_use]
pub const fn instrument_any_type(instrument: &InstrumentAny) -> NautilusInstrumentType {
    match instrument {
        InstrumentAny::Betting(_) => NautilusInstrumentType::BettingInstrument,
        InstrumentAny::BinaryOption(_) => NautilusInstrumentType::BinaryOption,
        InstrumentAny::Cfd(_) => NautilusInstrumentType::Cfd,
        InstrumentAny::Commodity(_) => NautilusInstrumentType::Commodity,
        InstrumentAny::CryptoFuture(_) => NautilusInstrumentType::CryptoFuture,
        InstrumentAny::CryptoFuturesSpread(_) => NautilusInstrumentType::CryptoFuturesSpread,
        InstrumentAny::CryptoOption(_) => NautilusInstrumentType::CryptoOption,
        InstrumentAny::CryptoOptionSpread(_) => NautilusInstrumentType::CryptoOptionSpread,
        InstrumentAny::CryptoPerpetual(_) => NautilusInstrumentType::CryptoPerpetual,
        InstrumentAny::CurrencyPair(_) => NautilusInstrumentType::CurrencyPair,
        InstrumentAny::Equity(_) => NautilusInstrumentType::Equity,
        InstrumentAny::FuturesContract(_) => NautilusInstrumentType::FuturesContract,
        InstrumentAny::FuturesSpread(_) => NautilusInstrumentType::FuturesSpread,
        InstrumentAny::IndexInstrument(_) => NautilusInstrumentType::IndexInstrument,
        InstrumentAny::OptionContract(_) => NautilusInstrumentType::OptionContract,
        InstrumentAny::OptionSpread(_) => NautilusInstrumentType::OptionSpread,
        InstrumentAny::PerpetualContract(_) => NautilusInstrumentType::PerpetualContract,
        InstrumentAny::TokenizedAsset(_) => NautilusInstrumentType::TokenizedAsset,
    }
}

/// Returns the catalog prefix for non-data record types.
#[must_use]
pub fn record_path_prefix(record_type: &NautilusRecordType) -> Cow<'static, str> {
    match record_type {
        NautilusRecordType::AccountState => Cow::Borrowed("account_state"),
        NautilusRecordType::OrderInitialized => Cow::Borrowed("order_initialized"),
        NautilusRecordType::OrderDenied => Cow::Borrowed("order_denied"),
        NautilusRecordType::OrderEmulated => Cow::Borrowed("order_emulated"),
        NautilusRecordType::OrderSubmitted => Cow::Borrowed("order_submitted"),
        NautilusRecordType::OrderAccepted => Cow::Borrowed("order_accepted"),
        NautilusRecordType::OrderRejected => Cow::Borrowed("order_rejected"),
        NautilusRecordType::OrderPendingCancel => Cow::Borrowed("order_pending_cancel"),
        NautilusRecordType::OrderCanceled => Cow::Borrowed("order_canceled"),
        NautilusRecordType::OrderCancelRejected => Cow::Borrowed("order_cancel_rejected"),
        NautilusRecordType::OrderExpired => Cow::Borrowed("order_expired"),
        NautilusRecordType::OrderTriggered => Cow::Borrowed("order_triggered"),
        NautilusRecordType::OrderPendingUpdate => Cow::Borrowed("order_pending_update"),
        NautilusRecordType::OrderReleased => Cow::Borrowed("order_released"),
        NautilusRecordType::OrderModifyRejected => Cow::Borrowed("order_modify_rejected"),
        NautilusRecordType::OrderUpdated => Cow::Borrowed("order_updated"),
        NautilusRecordType::OrderFilled => Cow::Borrowed("order_filled"),
        NautilusRecordType::OrderFillVoided => Cow::Borrowed("order_fill_voided"),
        NautilusRecordType::PositionOpened => Cow::Borrowed("position_opened"),
        NautilusRecordType::PositionChanged => Cow::Borrowed("position_changed"),
        NautilusRecordType::PositionClosed => Cow::Borrowed("position_closed"),
        NautilusRecordType::PositionAdjusted => Cow::Borrowed("position_adjusted"),
        NautilusRecordType::OrderSnapshot => Cow::Borrowed("order_snapshot"),
        NautilusRecordType::PositionSnapshot => Cow::Borrowed("position_snapshot"),
        NautilusRecordType::OrderStatusReport => Cow::Borrowed("order_status_report"),
        NautilusRecordType::FillReport => Cow::Borrowed("fill_report"),
        NautilusRecordType::PositionStatusReport => Cow::Borrowed("position_status_report"),
        NautilusRecordType::ExecutionMassStatus => Cow::Borrowed("execution_mass_status"),
        #[cfg(feature = "defi")]
        NautilusRecordType::Defi => Cow::Borrowed("defi"),
    }
}

/// Returns the SQL-safe table-name stem identifying a catalog type.
///
/// The aggregate instrument family spans several class directories, so its stem is the shared
/// `instruments` name rather than any one of them. The stem names registered query tables and
/// never addresses storage; use [`parquet_catalog_data_type_path_prefixes`] for directories.
#[must_use]
pub fn parquet_catalog_data_type_table_stem(data_type: &CatalogDataType) -> Cow<'static, str> {
    match data_type {
        CatalogDataType::Data(data_type) => parquet_data_path_prefix(data_type),
        CatalogDataType::Record(record_type) => record_path_prefix(record_type),
        CatalogDataType::Instrument(instrument_type) => {
            Cow::Borrowed(instrument_path_prefix(instrument_type))
        }
    }
}

/// Returns every Parquet directory prefix a catalog type covers.
///
/// Parquet stores each instrument class in its own top-level directory, so the aggregate
/// instrument family covers every class directory and an instrument class covers one. Every
/// other family covers exactly one directory.
#[must_use]
pub fn parquet_catalog_data_type_path_prefixes(
    data_type: &CatalogDataType,
) -> Vec<Cow<'static, str>> {
    match data_type {
        CatalogDataType::Data(NautilusDataType::Instrument) => INSTRUMENT_PATH_PREFIXES
            .iter()
            .map(|prefix| Cow::Borrowed(*prefix))
            .collect(),
        CatalogDataType::Data(data_type) => vec![parquet_data_path_prefix(data_type)],
        CatalogDataType::Record(record_type) => vec![record_path_prefix(record_type)],
        CatalogDataType::Instrument(instrument_type) => {
            vec![Cow::Borrowed(instrument_path_prefix(instrument_type))]
        }
    }
}

/// Returns the custom type name when the catalog type is custom data.
#[must_use]
pub fn custom_type_name(data_type: &CatalogDataType) -> Option<&str> {
    match data_type {
        CatalogDataType::Data(NautilusDataType::Custom { type_name }) => Some(type_name),
        _ => None,
    }
}

/// Returns the Parquet directory prefixes a custom type covers for reads.
///
/// Reads fan out across the current `custom/{TypeName}` layout and the legacy Python-written
/// `custom_<snake_case>` layout (e.g. `custom_binance_bar`). Maintenance operations that map
/// intervals back to filenames (`delete_data_range`, period consolidation) stay on the canonical
/// prefix only; migrate legacy layouts with `migrate-parquet` for full maintenance support.
#[must_use]
pub fn custom_data_read_prefixes(type_name: &str) -> [Cow<'static, str>; 2] {
    [
        Cow::Owned(format!("custom/{type_name}")),
        Cow::Owned(format!("custom_{}", to_snake_case(type_name))),
    ]
}

/// Maps a Rust built-in data type to its semantic catalog data type.
pub trait HasCatalogDataType {
    fn catalog_data_type() -> NautilusDataType;
}

macro_rules! impl_catalog_data_families {
    ($(($variant:ident, $type:ident, $data:ident, $batch:ident, $prefix:literal)),+ $(,)?) => {
        $(
            impl HasCatalogDataType for $type {
                fn catalog_data_type() -> NautilusDataType {
                    NautilusDataType::$variant
                }
            }

            impl CatalogPathPrefix for $type {
                fn path_prefix() -> &'static str {
                    $prefix
                }
            }
        )+
    };
}

nautilus_model::for_each_data_type!(impl_catalog_data_families);

macro_rules! impl_catalog_path_prefix {
    ($type:ty, $path:expr) => {
        impl CatalogPathPrefix for $type {
            fn path_prefix() -> &'static str {
                $path
            }
        }
    };
}

impl_catalog_path_prefix!(AccountState, "account_state");
impl_catalog_path_prefix!(OrderInitialized, "order_initialized");
impl_catalog_path_prefix!(OrderDenied, "order_denied");
impl_catalog_path_prefix!(OrderEmulated, "order_emulated");
impl_catalog_path_prefix!(OrderSubmitted, "order_submitted");
impl_catalog_path_prefix!(OrderAccepted, "order_accepted");
impl_catalog_path_prefix!(OrderRejected, "order_rejected");
impl_catalog_path_prefix!(OrderPendingCancel, "order_pending_cancel");
impl_catalog_path_prefix!(OrderCanceled, "order_canceled");
impl_catalog_path_prefix!(OrderCancelRejected, "order_cancel_rejected");
impl_catalog_path_prefix!(OrderExpired, "order_expired");
impl_catalog_path_prefix!(OrderTriggered, "order_triggered");
impl_catalog_path_prefix!(OrderPendingUpdate, "order_pending_update");
impl_catalog_path_prefix!(OrderReleased, "order_released");
impl_catalog_path_prefix!(OrderModifyRejected, "order_modify_rejected");
impl_catalog_path_prefix!(OrderUpdated, "order_updated");
impl_catalog_path_prefix!(OrderFilled, "order_filled");
impl_catalog_path_prefix!(OrderFillVoided, "order_fill_voided");
impl_catalog_path_prefix!(PositionOpened, "position_opened");
impl_catalog_path_prefix!(PositionChanged, "position_changed");
impl_catalog_path_prefix!(PositionClosed, "position_closed");
impl_catalog_path_prefix!(PositionAdjusted, "position_adjusted");
impl_catalog_path_prefix!(OrderSnapshot, "order_snapshot");
impl_catalog_path_prefix!(PositionSnapshot, "position_snapshot");
impl_catalog_path_prefix!(PortfolioSnapshot, "portfolio_snapshot");

impl_catalog_path_prefix!(FillReport, "fill_report");
impl_catalog_path_prefix!(OrderStatusReport, "order_status_report");
impl_catalog_path_prefix!(PositionStatusReport, "position_status_report");
impl_catalog_path_prefix!(ExecutionMassStatus, "execution_mass_status");

impl NautilusDataTypePrefix for NautilusDataType {
    fn path_prefix(&self) -> Cow<'static, str> {
        data_path_prefix(self)
    }
}

impl NautilusRecordTypePrefix for NautilusRecordType {
    fn path_prefix(&self) -> Cow<'static, str> {
        record_path_prefix(self)
    }
}

pub(crate) fn filter_instruments_for_request_range(
    mut instruments: Vec<InstrumentAny>,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) -> Vec<InstrumentAny> {
    if start.is_none() && end.is_none() {
        instruments.sort_by_key(HasTsInit::ts_init);
        return instruments;
    }

    let start = start.map(|ts| ts.as_u64());
    let end = end.map(|ts| ts.as_u64());
    let mut in_range = Vec::new();
    let mut in_range_ids = BTreeSet::new();
    let mut latest_before_start = BTreeMap::<String, InstrumentAny>::new();

    for instrument in instruments {
        let ts_init = HasTsInit::ts_init(&instrument).as_u64();
        let instrument_id = instrument.id().to_string();
        if start.is_none_or(|value| ts_init >= value) && end.is_none_or(|value| ts_init <= value) {
            in_range_ids.insert(instrument_id);
            in_range.push(instrument);
        } else if let Some(start) = start
            && ts_init < start
            && end.is_none_or(|value| ts_init <= value)
            && latest_before_start
                .get(&instrument_id)
                .is_none_or(|existing| HasTsInit::ts_init(existing).as_u64() < ts_init)
        {
            latest_before_start.insert(instrument_id, instrument);
        }
    }

    for (instrument_id, instrument) in latest_before_start {
        if !in_range_ids.contains(&instrument_id) {
            in_range.push(instrument);
        }
    }

    in_range.sort_by_key(HasTsInit::ts_init);
    in_range
}

pub(crate) fn filter_instrument_query_result(
    mut instruments: Vec<InstrumentAny>,
    start: Option<UnixNanos>,
    params: Option<&Params>,
) -> Vec<InstrumentAny> {
    if params.and_then(|params| params.get_bool("only_last")) == Some(false)
        && let Some(start) = start
    {
        instruments.retain(|instrument| HasTsInit::ts_init(instrument) >= start);
    }

    instruments
}

#[cfg(test)]
mod tests {
    use nautilus_model::instruments::stubs::{audusd_sim, crypto_perpetual_ethusdt, equity_aapl};
    use rstest::rstest;
    use strum::IntoEnumIterator;

    use super::*;

    macro_rules! assert_data_type_prefixes {
        ($(($variant:ident, $type:ident, $data:ident, $batch:ident, $prefix:literal)),+ $(,)?) => {
            $(
                let data_type = NautilusDataType::$variant;
                assert_eq!(
                    data_type_from_data_path_prefix($prefix).unwrap(),
                    data_type,
                );
                assert_eq!(
                    data_type.to_string().parse::<NautilusDataType>().unwrap(),
                    data_type,
                );
            )+
        };
    }

    #[rstest]
    fn built_in_data_type_prefixes_and_display_round_trip() {
        nautilus_model::for_each_data_type!(assert_data_type_prefixes);
    }

    #[rstest]
    fn catalog_path_prefix_maps_legacy_depth_directory() {
        assert_eq!(
            data_type_from_data_path_prefix("order_book_depth10").unwrap(),
            NautilusDataType::OrderBookDepth
        );
        assert_eq!(
            data_path_prefix(&NautilusDataType::OrderBookDepth).as_ref(),
            "order_book_depths"
        );
    }

    #[rstest]
    fn catalog_data_type_converts_from_every_selector_family() {
        assert_eq!(
            CatalogDataType::from(NautilusDataType::QuoteTick),
            CatalogDataType::Data(NautilusDataType::QuoteTick)
        );
        assert_eq!(
            CatalogDataType::from(NautilusDataType::Instrument),
            CatalogDataType::Data(NautilusDataType::Instrument)
        );
        assert_eq!(
            CatalogDataType::from(NautilusDataType::Custom {
                type_name: "RustTestCustomData".to_string(),
            }),
            CatalogDataType::Data(NautilusDataType::Custom {
                type_name: "RustTestCustomData".to_string(),
            })
        );
        assert_eq!(
            CatalogDataType::from(NautilusRecordType::AccountState),
            CatalogDataType::Record(NautilusRecordType::AccountState)
        );
        assert_eq!(
            CatalogDataType::from(NautilusInstrumentType::Equity),
            CatalogDataType::Instrument(NautilusInstrumentType::Equity)
        );
    }

    #[rstest]
    fn parquet_prefixes_fan_out_only_for_the_aggregate_instrument_family() {
        assert_eq!(
            parquet_catalog_data_type_path_prefixes(&CatalogDataType::Data(
                NautilusDataType::Instrument
            )),
            INSTRUMENT_PATH_PREFIXES
                .iter()
                .map(|prefix| Cow::Borrowed(*prefix))
                .collect::<Vec<Cow<'static, str>>>()
        );
        assert_eq!(
            parquet_catalog_data_type_path_prefixes(&CatalogDataType::Instrument(
                NautilusInstrumentType::Equity
            )),
            vec![Cow::Borrowed("equity")]
        );
        assert_eq!(
            parquet_catalog_data_type_path_prefixes(&CatalogDataType::Data(
                NautilusDataType::QuoteTick
            )),
            vec![Cow::Borrowed("quotes")]
        );
        assert_eq!(
            parquet_catalog_data_type_path_prefixes(&CatalogDataType::Record(
                NautilusRecordType::AccountState
            )),
            vec![Cow::Borrowed("account_state")]
        );
    }

    #[rstest]
    fn instrument_path_prefixes_match_all_instrument_types() {
        let prefixes = NautilusInstrumentType::iter()
            .map(|instrument_type| instrument_path_prefix(&instrument_type))
            .collect::<Vec<_>>();

        assert_eq!(prefixes, INSTRUMENT_PATH_PREFIXES);
    }

    #[rstest]
    fn custom_data_read_prefixes_cover_canonical_and_legacy_layouts() {
        assert_eq!(
            custom_data_read_prefixes("BinanceBar").as_slice(),
            [
                Cow::Owned::<str>("custom/BinanceBar".to_string()),
                Cow::Owned::<str>("custom_binance_bar".to_string()),
            ]
            .as_slice(),
        );
        assert_eq!(
            custom_data_read_prefixes("RustTestCustomData")[1].as_ref(),
            "custom_rust_test_custom_data",
        );
    }

    macro_rules! assert_data_type_path_prefixes_match_writer {
        ($(($variant:ident, $type:ident, $data:ident, $batch:ident, $prefix:literal)),+ $(,)?) => {
            $(
                assert_eq!(
                    NautilusDataType::$variant.path_prefix(),
                    <$type as CatalogPathPrefix>::path_prefix(),
                );
            )+
        };
    }

    macro_rules! assert_record_type_path_prefixes_match_writer {
        ($($variant:ident),+ $(,)?) => {
            $(
                assert_eq!(
                    NautilusRecordType::$variant.path_prefix(),
                    <$variant as CatalogPathPrefix>::path_prefix(),
                );
            )+

            let checked = vec![$(NautilusRecordType::$variant),+];
            #[cfg(feature = "defi")]
            let expected = NautilusRecordType::iter()
                .filter(|record_type| *record_type != NautilusRecordType::Defi)
                .collect::<Vec<_>>();
            #[cfg(not(feature = "defi"))]
            let expected = NautilusRecordType::iter().collect::<Vec<_>>();
            assert_eq!(checked, expected);
        };
    }

    #[rstest]
    fn data_type_path_prefixes_match_writer_prefixes() {
        nautilus_model::for_each_data_type!(assert_data_type_path_prefixes_match_writer);
    }

    #[rstest]
    fn record_type_path_prefixes_match_writer_prefixes() {
        assert_record_type_path_prefixes_match_writer!(
            AccountState,
            OrderInitialized,
            OrderDenied,
            OrderEmulated,
            OrderSubmitted,
            OrderAccepted,
            OrderRejected,
            OrderPendingCancel,
            OrderCanceled,
            OrderCancelRejected,
            OrderExpired,
            OrderTriggered,
            OrderPendingUpdate,
            OrderReleased,
            OrderModifyRejected,
            OrderUpdated,
            OrderFilled,
            OrderFillVoided,
            PositionOpened,
            PositionChanged,
            PositionClosed,
            PositionAdjusted,
            OrderSnapshot,
            PositionSnapshot,
            OrderStatusReport,
            FillReport,
            PositionStatusReport,
            ExecutionMassStatus,
        );
    }

    #[rstest]
    #[case(CatalogDataType::Data(NautilusDataType::Instrument), "instruments")]
    #[case(CatalogDataType::Data(NautilusDataType::QuoteTick), "quotes")]
    #[case(CatalogDataType::Record(NautilusRecordType::FillReport), "fill_report")]
    #[case(CatalogDataType::Instrument(NautilusInstrumentType::Equity), "equity")]
    fn parquet_table_stems_name_each_catalog_type(
        #[case] data_type: CatalogDataType,
        #[case] expected: &str,
    ) {
        assert_eq!(parquet_catalog_data_type_table_stem(&data_type), expected);
    }

    #[rstest]
    fn data_type_from_data_path_prefix_parses_custom_prefix() {
        assert_eq!(
            data_type_from_data_path_prefix("custom/SensorReading").unwrap(),
            NautilusDataType::Custom {
                type_name: "SensorReading".to_string(),
            },
        );
    }

    #[rstest]
    #[case("custom")]
    #[case("CustomData")]
    fn data_type_from_data_path_prefix_rejects_custom_without_type_name(#[case] type_name: &str) {
        let error = data_type_from_data_path_prefix(type_name).unwrap_err();

        assert_eq!(
            error.to_string(),
            "custom data queries require custom/<type_name> or Custom:<type_name>",
        );
    }

    #[rstest]
    fn instrument_and_record_query_builders_set_every_field() {
        let mut params = Params::new();
        params.insert("only_last".to_string(), false.into());

        let instrument_query = CatalogInstrumentQuery::new()
            .with_instrument_ids(Some(vec!["AUD/USD.SIM".to_string()]))
            .with_range(Some(UnixNanos::from(1)), Some(UnixNanos::from(2)))
            .with_where_clause(Some("venue = 'SIM'".to_string()))
            .with_instrument_type(Some(NautilusInstrumentType::CurrencyPair));
        let record_query = CatalogRecordQuery::new(NautilusRecordType::FillReport)
            .with_identifier(Some("O-1".to_string()))
            .with_range(Some(UnixNanos::from(3)), Some(UnixNanos::from(4)))
            .with_where_clause(Some("trade_id = 'T-1'".to_string()))
            .with_params(Some(params.clone()))
            .with_as_of(CatalogAsOf::Version(7));

        assert_eq!(
            instrument_query,
            CatalogInstrumentQuery {
                instrument_ids: Some(vec!["AUD/USD.SIM".to_string()]),
                start: Some(UnixNanos::from(1)),
                end: Some(UnixNanos::from(2)),
                where_clause: Some("venue = 'SIM'".to_string()),
                instrument_type: Some(NautilusInstrumentType::CurrencyPair),
            },
        );
        assert_eq!(
            record_query,
            CatalogRecordQuery {
                record_type: NautilusRecordType::FillReport,
                identifier: Some("O-1".to_string()),
                start: Some(UnixNanos::from(3)),
                end: Some(UnixNanos::from(4)),
                where_clause: Some("trade_id = 'T-1'".to_string()),
                params: Some(params),
                as_of: CatalogAsOf::Version(7),
            },
        );
    }

    fn ethusdt_at(ts: u64) -> InstrumentAny {
        let mut instrument = crypto_perpetual_ethusdt();
        instrument.ts_event = UnixNanos::from(ts);
        instrument.ts_init = UnixNanos::from(ts);
        InstrumentAny::CryptoPerpetual(instrument)
    }

    fn audusd_at(ts: u64) -> InstrumentAny {
        let mut instrument = audusd_sim();
        instrument.ts_event = UnixNanos::from(ts);
        instrument.ts_init = UnixNanos::from(ts);
        InstrumentAny::CurrencyPair(instrument)
    }

    fn aapl_at(ts: u64) -> InstrumentAny {
        let mut instrument = equity_aapl();
        instrument.ts_event = UnixNanos::from(ts);
        instrument.ts_init = UnixNanos::from(ts);
        InstrumentAny::Equity(instrument)
    }

    fn instrument_versions(instruments: &[InstrumentAny]) -> Vec<(String, u64)> {
        instruments
            .iter()
            .map(|instrument| {
                (
                    instrument.id().to_string(),
                    HasTsInit::ts_init(instrument).as_u64(),
                )
            })
            .collect()
    }

    #[rstest]
    #[case::latest_before_start_listed_last(
        vec![ethusdt_at(1), ethusdt_at(3), audusd_at(2), audusd_at(6), aapl_at(12)],
        Some(5),
        Some(10),
        &[("ETHUSDT-PERP.BINANCE", 3), ("AUD/USD.SIM", 6)],
    )]
    #[case::latest_before_start_listed_first(
        vec![ethusdt_at(3), ethusdt_at(1), audusd_at(6), audusd_at(2), aapl_at(12)],
        Some(5),
        Some(10),
        &[("ETHUSDT-PERP.BINANCE", 3), ("AUD/USD.SIM", 6)],
    )]
    #[case::end_only(
        vec![ethusdt_at(3), audusd_at(6), aapl_at(12), ethusdt_at(1)],
        None,
        Some(10),
        &[("ETHUSDT-PERP.BINANCE", 1), ("ETHUSDT-PERP.BINANCE", 3), ("AUD/USD.SIM", 6)],
    )]
    #[case::start_only(
        vec![ethusdt_at(1), ethusdt_at(3), audusd_at(6), aapl_at(12)],
        Some(5),
        None,
        &[("ETHUSDT-PERP.BINANCE", 3), ("AUD/USD.SIM", 6), ("AAPL.XNAS", 12)],
    )]
    #[case::unbounded(
        vec![aapl_at(12), ethusdt_at(3), audusd_at(6)],
        None,
        None,
        &[("ETHUSDT-PERP.BINANCE", 3), ("AUD/USD.SIM", 6), ("AAPL.XNAS", 12)],
    )]
    fn filter_instruments_for_request_range_selects_versions(
        #[case] instruments: Vec<InstrumentAny>,
        #[case] start: Option<u64>,
        #[case] end: Option<u64>,
        #[case] expected: &[(&str, u64)],
    ) {
        let filtered = filter_instruments_for_request_range(
            instruments,
            start.map(UnixNanos::from),
            end.map(UnixNanos::from),
        );

        let expected = expected
            .iter()
            .map(|&(id, ts)| (id.to_string(), ts))
            .collect::<Vec<_>>();
        assert_eq!(instrument_versions(&filtered), expected);
    }

    #[rstest]
    #[case::only_last_absent(None, &[("ETHUSDT-PERP.BINANCE", 3), ("AUD/USD.SIM", 6)])]
    #[case::only_last_true(Some(true), &[("ETHUSDT-PERP.BINANCE", 3), ("AUD/USD.SIM", 6)])]
    #[case::only_last_false(Some(false), &[("AUD/USD.SIM", 6)])]
    fn filter_instrument_query_result_drops_pre_start_versions_only_when_requested(
        #[case] only_last: Option<bool>,
        #[case] expected: &[(&str, u64)],
    ) {
        let params = only_last.map(|only_last| {
            let mut params = Params::new();
            params.insert("only_last".to_string(), only_last.into());
            params
        });

        let filtered = filter_instrument_query_result(
            vec![ethusdt_at(3), audusd_at(6)],
            Some(UnixNanos::from(5)),
            params.as_ref(),
        );

        let expected = expected
            .iter()
            .map(|&(id, ts)| (id.to_string(), ts))
            .collect::<Vec<_>>();
        assert_eq!(instrument_versions(&filtered), expected);
    }
}
