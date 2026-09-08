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
};

use nautilus_core::{Params, UnixNanos};
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
        }
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
}

/// Returns the shared catalog prefix for a Parquet data type.
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
            NautilusDataType::OrderBook => Cow::Borrowed("order_book"),
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

/// Maps a Rust built-in data type to its semantic catalog data type.
pub trait CatalogDataType {
    fn catalog_data_type() -> NautilusDataType;
}

macro_rules! impl_catalog_data_families {
    ($(($variant:ident, $type:ident, $data:ident, $batch:ident, $prefix:literal)),+ $(,)?) => {
        $(
            impl CatalogDataType for $type {
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
    instruments: Vec<InstrumentAny>,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) -> Vec<InstrumentAny> {
    if start.is_none() && end.is_none() {
        let mut instruments = instruments;
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
        {
            match latest_before_start.get(&instrument_id) {
                Some(existing)
                    if HasTsInit::ts_init(existing) >= HasTsInit::ts_init(&instrument) => {}
                _ => {
                    latest_before_start.insert(instrument_id, instrument);
                }
            }
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
    fn instrument_path_prefixes_match_all_instrument_types() {
        let prefixes = NautilusInstrumentType::iter()
            .map(|instrument_type| instrument_path_prefix(&instrument_type))
            .collect::<Vec<_>>();

        assert_eq!(prefixes, INSTRUMENT_PATH_PREFIXES);
    }
}
