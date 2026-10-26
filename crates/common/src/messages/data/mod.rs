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

//! Data specific messages such as subscriptions and requests.

use std::{any::Any, sync::Arc};

use nautilus_core::{Params, UUID4, UnixNanos};
use nautilus_model::{
    data::{BarType, NautilusDataType},
    identifiers::{ClientId, InstrumentId, Venue},
};
use serde::{Deserialize, Serialize};

use crate::cache::Cache;

pub mod request;
pub mod response;
pub mod subscribe;
pub mod unsubscribe;

mod identifiers;

/// Params key used to flag a book subscription as targeting a parent symbol.
///
/// When the boolean value is `true`, the subscription fans out across all
/// instruments that resolve from the parent components (see
/// [`InstrumentId::parse_parent_components`]). When absent or `false`, the
/// subscription is routed to the concrete instrument id only.
///
/// [`InstrumentId::parse_parent_components`]: nautilus_model::identifiers::InstrumentId::parse_parent_components
pub const PARAMS_IS_PARENT: &str = "is_parent";

// Re-exports
pub use identifiers::{
    IdentifierCountError, IntoBarTypes, IntoInstrumentIds, QuoteRouting, ensure_routable_bar_types,
    ensure_routable_instrument_ids, ensure_subscription_routes, quote_routing,
};
pub use request::{
    RequestBars, RequestBookDeltas, RequestBookDepth, RequestBookSnapshot, RequestCustomData,
    RequestFundingRates, RequestInstrument, RequestInstruments, RequestJoin,
    RequestOptionChainReferencePrice, RequestQuotes, RequestTrades,
};
pub use response::{
    BarsResponse, BookDeltasResponse, BookDepthResponse, BookResponse, CustomDataResponse,
    FundingRatesResponse, InstrumentResponse, InstrumentsResponse,
    OptionChainReferencePriceResponse, QuotesResponse, TradesResponse,
};
pub use subscribe::{
    SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth, SubscribeBookSnapshots,
    SubscribeCustomData, SubscribeFundingRates, SubscribeIndexPrices, SubscribeInstrument,
    SubscribeInstrumentClose, SubscribeInstrumentStatus, SubscribeInstruments, SubscribeMarkPrices,
    SubscribeOptionChain, SubscribeOptionGreeks, SubscribeQuotes, SubscribeTrades,
    SubscriptionError,
};
pub use unsubscribe::{
    UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookDepth, UnsubscribeBookSnapshots,
    UnsubscribeCustomData, UnsubscribeFundingRates, UnsubscribeIndexPrices, UnsubscribeInstrument,
    UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus, UnsubscribeInstruments,
    UnsubscribeMarkPrices, UnsubscribeOptionChain, UnsubscribeOptionGreeks, UnsubscribeQuotes,
    UnsubscribeTrades,
};

#[cfg(feature = "defi")]
use crate::messages::defi::{DefiRequestCommand, DefiSubscribeCommand, DefiUnsubscribeCommand};

#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum DataCommand {
    Request(RequestCommand),
    Subscribe(SubscribeCommand),
    Unsubscribe(UnsubscribeCommand),
    #[cfg(feature = "defi")]
    DefiRequest(DefiRequestCommand),
    #[cfg(feature = "defi")]
    DefiSubscribe(DefiSubscribeCommand),
    #[cfg(feature = "defi")]
    DefiUnsubscribe(DefiUnsubscribeCommand),
}

impl DataCommand {
    /// Converts the command to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Converts a subscribe variant into its matching unsubscribe variant.
    ///
    /// Returns `None` for request and unsubscribe variants.
    pub(crate) fn into_unsubscribe(self, command_id: UUID4, ts_init: UnixNanos) -> Option<Self> {
        match self {
            Self::Subscribe(command) => {
                let correlation_id = matches!(
                    command,
                    SubscribeCommand::BookDeltas(_)
                        | SubscribeCommand::BookDepth(_)
                        | SubscribeCommand::BookSnapshots(_)
                )
                .then(|| command.command_id());
                Some(Self::Unsubscribe(command.into_unsubscribe(
                    command_id,
                    ts_init,
                    correlation_id,
                )))
            }
            #[cfg(feature = "defi")]
            Self::DefiSubscribe(command) => Some(Self::DefiUnsubscribe(
                command.into_unsubscribe(command_id, ts_init),
            )),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SubscribeCommand {
    Data(SubscribeCustomData),
    Instrument(SubscribeInstrument),
    Instruments(SubscribeInstruments),
    BookDeltas(SubscribeBookDeltas),
    BookDepth(SubscribeBookDepth),
    BookSnapshots(SubscribeBookSnapshots),
    Quotes(SubscribeQuotes),
    Trades(SubscribeTrades),
    Bars(SubscribeBars),
    MarkPrices(SubscribeMarkPrices),
    IndexPrices(SubscribeIndexPrices),
    FundingRates(SubscribeFundingRates),
    InstrumentStatus(SubscribeInstrumentStatus),
    InstrumentClose(SubscribeInstrumentClose),
    OptionGreeks(SubscribeOptionGreeks),
    OptionChain(SubscribeOptionChain),
}

impl PartialEq for SubscribeCommand {
    fn eq(&self, other: &Self) -> bool {
        self.command_id() == other.command_id()
    }
}

impl SubscribeCommand {
    /// Validates list routing before subscription ownership is retained.
    ///
    /// # Errors
    ///
    /// Returns an error if an identifier or bar list is invalid or trade routes are mixed.
    /// For non-external clients, also rejects mixed quote routes and unsupported lists with
    /// multiple IDs containing synthetic members.
    pub fn validate_routing(&self, cache: &Cache) -> anyhow::Result<()> {
        if let Some(bar_types) = self.bar_types() {
            ensure_routable_bar_types(bar_types, self.client_id().is_some())?;
        }

        let Some(ids) = self.instrument_ids() else {
            return Ok(());
        };

        ensure_routable_instrument_ids(ids, self.client_id().is_some())?;

        match self {
            Self::Quotes(command) => {
                quote_routing(ids, self.client_id(), command.params.as_ref(), cache)?;
            }
            Self::Trades(_) => {
                anyhow::ensure!(
                    ids.iter()
                        .all(|id| id.is_synthetic() == ids[0].is_synthetic()),
                    "trade identifier lists cannot mix client and synthetic routes"
                );
            }
            Self::Instrument(_)
            | Self::BookDeltas(_)
            | Self::BookDepth(_)
            | Self::BookSnapshots(_)
            | Self::InstrumentStatus(_)
            | Self::InstrumentClose(_)
            | Self::OptionGreeks(_) => {
                ensure_subscription_routes(ids, self.client_id(), cache)?;
            }
            _ => {}
        }

        Ok(())
    }

    /// Returns the instrument IDs for an instrument subscription, if applicable.
    #[must_use]
    pub fn instrument_ids(&self) -> Option<&[InstrumentId]> {
        match self {
            Self::Instrument(command) => Some(&command.instrument_ids),
            Self::BookDeltas(command) => Some(&command.instrument_ids),
            Self::BookDepth(command) => Some(&command.instrument_ids),
            Self::BookSnapshots(command) => Some(&command.instrument_ids),
            Self::Quotes(command) => Some(&command.instrument_ids),
            Self::Trades(command) => Some(&command.instrument_ids),
            Self::MarkPrices(command) => Some(&command.instrument_ids),
            Self::IndexPrices(command) => Some(&command.instrument_ids),
            Self::FundingRates(command) => Some(&command.instrument_ids),
            Self::InstrumentStatus(command) => Some(&command.instrument_ids),
            Self::InstrumentClose(command) => Some(&command.instrument_ids),
            Self::OptionGreeks(command) => Some(&command.instrument_ids),
            _ => None,
        }
    }

    /// Projects an instrument subscription onto the supplied IDs, preserving its metadata.
    ///
    /// # Panics
    ///
    /// Panics if an instrument subscription receives an empty identifier list.
    #[must_use]
    pub fn with_instrument_ids(&self, instrument_ids: impl IntoInstrumentIds) -> Option<Self> {
        match self {
            Self::Instrument(command) => Some(Self::Instrument(
                command.with_instrument_ids(instrument_ids),
            )),
            Self::BookDeltas(command) => Some(Self::BookDeltas(
                command.with_instrument_ids(instrument_ids),
            )),
            Self::BookDepth(command) => {
                Some(Self::BookDepth(command.with_instrument_ids(instrument_ids)))
            }
            Self::BookSnapshots(command) => Some(Self::BookSnapshots(
                command.with_instrument_ids(instrument_ids),
            )),
            Self::Quotes(command) => {
                Some(Self::Quotes(command.with_instrument_ids(instrument_ids)))
            }
            Self::Trades(command) => {
                Some(Self::Trades(command.with_instrument_ids(instrument_ids)))
            }
            Self::MarkPrices(command) => Some(Self::MarkPrices(
                command.with_instrument_ids(instrument_ids),
            )),
            Self::IndexPrices(command) => Some(Self::IndexPrices(
                command.with_instrument_ids(instrument_ids),
            )),
            Self::FundingRates(command) => Some(Self::FundingRates(
                command.with_instrument_ids(instrument_ids),
            )),
            Self::InstrumentStatus(command) => Some(Self::InstrumentStatus(
                command.with_instrument_ids(instrument_ids),
            )),
            Self::InstrumentClose(command) => Some(Self::InstrumentClose(
                command.with_instrument_ids(instrument_ids),
            )),
            Self::OptionGreeks(command) => Some(Self::OptionGreeks(
                command.with_instrument_ids(instrument_ids),
            )),
            _ => None,
        }
    }

    /// Returns the bar types for a bar subscription, if applicable.
    #[must_use]
    pub fn bar_types(&self) -> Option<&[BarType]> {
        match self {
            Self::Bars(command) => Some(&command.bar_types),
            _ => None,
        }
    }

    /// Projects a bar subscription onto the supplied types, preserving its metadata.
    ///
    /// # Panics
    ///
    /// Panics if a bar subscription receives an empty list.
    #[must_use]
    pub fn with_bar_types(&self, bar_types: impl IntoBarTypes) -> Option<Self> {
        match self {
            Self::Bars(command) => Some(Self::Bars(command.with_bar_types(bar_types))),
            _ => None,
        }
    }

    /// Converts the command to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    /// Converts this subscribe command into its matching unsubscribe command.
    ///
    /// Preserves the subscribed data identity and client route while replacing the command ID and
    /// initialization timestamp. It also preserves parameters and sets the supplied correlation ID
    /// when the matching unsubscribe command supports those fields.
    #[must_use]
    pub fn into_unsubscribe(
        self,
        command_id: UUID4,
        ts_init: UnixNanos,
        correlation_id: Option<UUID4>,
    ) -> UnsubscribeCommand {
        match self {
            Self::Data(cmd) => UnsubscribeCommand::Data(UnsubscribeCustomData::new(
                cmd.client_id,
                cmd.venue,
                cmd.data_type,
                command_id,
                ts_init,
                correlation_id,
                cmd.params,
            )),
            Self::Instrument(cmd) => UnsubscribeCommand::Instrument(UnsubscribeInstrument::new(
                cmd.instrument_ids,
                cmd.client_id,
                cmd.venue,
                command_id,
                ts_init,
                correlation_id,
                cmd.params,
            )),
            Self::Instruments(cmd) => UnsubscribeCommand::Instruments(UnsubscribeInstruments::new(
                cmd.client_id,
                cmd.venue,
                command_id,
                ts_init,
                correlation_id,
                cmd.params,
            )),
            Self::BookDeltas(cmd) => UnsubscribeCommand::BookDeltas(UnsubscribeBookDeltas::new(
                cmd.instrument_ids,
                cmd.client_id,
                cmd.venue,
                command_id,
                ts_init,
                correlation_id,
                cmd.params,
            )),
            Self::BookDepth(cmd) => UnsubscribeCommand::BookDepth(UnsubscribeBookDepth::new(
                cmd.instrument_ids,
                cmd.client_id,
                cmd.venue,
                command_id,
                ts_init,
                correlation_id,
                cmd.params,
            )),
            Self::BookSnapshots(cmd) => {
                UnsubscribeCommand::BookSnapshots(UnsubscribeBookSnapshots::new(
                    cmd.instrument_ids,
                    cmd.interval_ms,
                    cmd.client_id,
                    cmd.venue,
                    command_id,
                    ts_init,
                    correlation_id,
                    cmd.params,
                ))
            }
            Self::Quotes(cmd) => UnsubscribeCommand::Quotes(UnsubscribeQuotes::new(
                cmd.instrument_ids,
                cmd.client_id,
                cmd.venue,
                command_id,
                ts_init,
                correlation_id,
                cmd.params,
            )),
            Self::Trades(cmd) => UnsubscribeCommand::Trades(UnsubscribeTrades::new(
                cmd.instrument_ids,
                cmd.client_id,
                cmd.venue,
                command_id,
                ts_init,
                correlation_id,
                cmd.params,
            )),
            Self::Bars(cmd) => UnsubscribeCommand::Bars(UnsubscribeBars::new(
                cmd.bar_types,
                cmd.client_id,
                cmd.venue,
                command_id,
                ts_init,
                correlation_id,
                cmd.params,
            )),
            Self::MarkPrices(cmd) => UnsubscribeCommand::MarkPrices(UnsubscribeMarkPrices::new(
                cmd.instrument_ids,
                cmd.client_id,
                cmd.venue,
                command_id,
                ts_init,
                correlation_id,
                cmd.params,
            )),
            Self::IndexPrices(cmd) => UnsubscribeCommand::IndexPrices(UnsubscribeIndexPrices::new(
                cmd.instrument_ids,
                cmd.client_id,
                cmd.venue,
                command_id,
                ts_init,
                correlation_id,
                cmd.params,
            )),
            Self::FundingRates(cmd) => {
                UnsubscribeCommand::FundingRates(UnsubscribeFundingRates::new(
                    cmd.instrument_ids,
                    cmd.client_id,
                    cmd.venue,
                    command_id,
                    ts_init,
                    correlation_id,
                    cmd.params,
                ))
            }
            Self::InstrumentStatus(cmd) => {
                UnsubscribeCommand::InstrumentStatus(UnsubscribeInstrumentStatus::new(
                    cmd.instrument_ids,
                    cmd.client_id,
                    cmd.venue,
                    command_id,
                    ts_init,
                    correlation_id,
                    cmd.params,
                ))
            }
            Self::InstrumentClose(cmd) => {
                UnsubscribeCommand::InstrumentClose(UnsubscribeInstrumentClose::new(
                    cmd.instrument_ids,
                    cmd.client_id,
                    cmd.venue,
                    command_id,
                    ts_init,
                    correlation_id,
                    cmd.params,
                ))
            }
            Self::OptionGreeks(cmd) => {
                UnsubscribeCommand::OptionGreeks(UnsubscribeOptionGreeks::new(
                    cmd.instrument_ids,
                    cmd.client_id,
                    cmd.venue,
                    command_id,
                    ts_init,
                    correlation_id,
                    cmd.params,
                ))
            }
            Self::OptionChain(cmd) => {
                let mut unsubscribe = UnsubscribeOptionChain::new(
                    cmd.series_id,
                    command_id,
                    ts_init,
                    cmd.client_id,
                    cmd.venue,
                );
                unsubscribe.params = cmd.params;
                UnsubscribeCommand::OptionChain(unsubscribe)
            }
        }
    }

    pub fn command_id(&self) -> UUID4 {
        match self {
            Self::Data(cmd) => cmd.command_id,
            Self::Instrument(cmd) => cmd.command_id,
            Self::Instruments(cmd) => cmd.command_id,
            Self::BookDeltas(cmd) => cmd.command_id,
            Self::BookDepth(cmd) => cmd.command_id,
            Self::BookSnapshots(cmd) => cmd.command_id,
            Self::Quotes(cmd) => cmd.command_id,
            Self::Trades(cmd) => cmd.command_id,
            Self::Bars(cmd) => cmd.command_id,
            Self::MarkPrices(cmd) => cmd.command_id,
            Self::IndexPrices(cmd) => cmd.command_id,
            Self::FundingRates(cmd) => cmd.command_id,
            Self::InstrumentStatus(cmd) => cmd.command_id,
            Self::InstrumentClose(cmd) => cmd.command_id,
            Self::OptionGreeks(cmd) => cmd.command_id,
            Self::OptionChain(cmd) => cmd.command_id,
        }
    }

    pub fn client_id(&self) -> Option<&ClientId> {
        match self {
            Self::Data(cmd) => cmd.client_id.as_ref(),
            Self::Instrument(cmd) => cmd.client_id.as_ref(),
            Self::Instruments(cmd) => cmd.client_id.as_ref(),
            Self::BookDeltas(cmd) => cmd.client_id.as_ref(),
            Self::BookDepth(cmd) => cmd.client_id.as_ref(),
            Self::BookSnapshots(cmd) => cmd.client_id.as_ref(),
            Self::Quotes(cmd) => cmd.client_id.as_ref(),
            Self::Trades(cmd) => cmd.client_id.as_ref(),
            Self::MarkPrices(cmd) => cmd.client_id.as_ref(),
            Self::IndexPrices(cmd) => cmd.client_id.as_ref(),
            Self::FundingRates(cmd) => cmd.client_id.as_ref(),
            Self::Bars(cmd) => cmd.client_id.as_ref(),
            Self::InstrumentStatus(cmd) => cmd.client_id.as_ref(),
            Self::InstrumentClose(cmd) => cmd.client_id.as_ref(),
            Self::OptionGreeks(cmd) => cmd.client_id.as_ref(),
            Self::OptionChain(cmd) => cmd.client_id.as_ref(),
        }
    }

    pub fn venue(&self) -> Option<&Venue> {
        match self {
            Self::Data(cmd) => cmd.venue.as_ref(),
            Self::Instrument(cmd) => cmd.venue.as_ref(),
            Self::Instruments(cmd) => Some(&cmd.venue),
            Self::BookDeltas(cmd) => cmd.venue.as_ref(),
            Self::BookDepth(cmd) => cmd.venue.as_ref(),
            Self::BookSnapshots(cmd) => cmd.venue.as_ref(),
            Self::Quotes(cmd) => cmd.venue.as_ref(),
            Self::Trades(cmd) => cmd.venue.as_ref(),
            Self::MarkPrices(cmd) => cmd.venue.as_ref(),
            Self::IndexPrices(cmd) => cmd.venue.as_ref(),
            Self::FundingRates(cmd) => cmd.venue.as_ref(),
            Self::Bars(cmd) => cmd.venue.as_ref(),
            Self::InstrumentStatus(cmd) => cmd.venue.as_ref(),
            Self::InstrumentClose(cmd) => cmd.venue.as_ref(),
            Self::OptionGreeks(cmd) => cmd.venue.as_ref(),
            Self::OptionChain(cmd) => cmd.venue.as_ref(),
        }
    }

    pub fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Data(cmd) => cmd.ts_init,
            Self::Instrument(cmd) => cmd.ts_init,
            Self::Instruments(cmd) => cmd.ts_init,
            Self::BookDeltas(cmd) => cmd.ts_init,
            Self::BookDepth(cmd) => cmd.ts_init,
            Self::BookSnapshots(cmd) => cmd.ts_init,
            Self::Quotes(cmd) => cmd.ts_init,
            Self::Trades(cmd) => cmd.ts_init,
            Self::MarkPrices(cmd) => cmd.ts_init,
            Self::IndexPrices(cmd) => cmd.ts_init,
            Self::FundingRates(cmd) => cmd.ts_init,
            Self::Bars(cmd) => cmd.ts_init,
            Self::InstrumentStatus(cmd) => cmd.ts_init,
            Self::InstrumentClose(cmd) => cmd.ts_init,
            Self::OptionGreeks(cmd) => cmd.ts_init,
            Self::OptionChain(cmd) => cmd.ts_init,
        }
    }

    pub fn correlation_id(&self) -> Option<UUID4> {
        match self {
            Self::Data(cmd) => cmd.correlation_id,
            Self::Instrument(cmd) => cmd.correlation_id,
            Self::Instruments(cmd) => cmd.correlation_id,
            Self::BookDeltas(cmd) => cmd.correlation_id,
            Self::BookDepth(cmd) => cmd.correlation_id,
            Self::BookSnapshots(cmd) => cmd.correlation_id,
            Self::Quotes(cmd) => cmd.correlation_id,
            Self::Trades(cmd) => cmd.correlation_id,
            Self::MarkPrices(cmd) => cmd.correlation_id,
            Self::IndexPrices(cmd) => cmd.correlation_id,
            Self::FundingRates(cmd) => cmd.correlation_id,
            Self::Bars(cmd) => cmd.correlation_id,
            Self::InstrumentStatus(cmd) => cmd.correlation_id,
            Self::InstrumentClose(cmd) => cmd.correlation_id,
            Self::OptionGreeks(cmd) => cmd.correlation_id,
            Self::OptionChain(cmd) => cmd.correlation_id,
        }
    }

    pub fn params(&self) -> Option<&Params> {
        match self {
            Self::Data(cmd) => cmd.params.as_ref(),
            Self::Instrument(cmd) => cmd.params.as_ref(),
            Self::Instruments(cmd) => cmd.params.as_ref(),
            Self::BookDeltas(cmd) => cmd.params.as_ref(),
            Self::BookDepth(cmd) => cmd.params.as_ref(),
            Self::BookSnapshots(cmd) => cmd.params.as_ref(),
            Self::Quotes(cmd) => cmd.params.as_ref(),
            Self::Trades(cmd) => cmd.params.as_ref(),
            Self::Bars(cmd) => cmd.params.as_ref(),
            Self::MarkPrices(cmd) => cmd.params.as_ref(),
            Self::IndexPrices(cmd) => cmd.params.as_ref(),
            Self::FundingRates(cmd) => cmd.params.as_ref(),
            Self::InstrumentStatus(cmd) => cmd.params.as_ref(),
            Self::InstrumentClose(cmd) => cmd.params.as_ref(),
            Self::OptionGreeks(cmd) => cmd.params.as_ref(),
            Self::OptionChain(cmd) => cmd.params.as_ref(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum UnsubscribeCommand {
    Data(UnsubscribeCustomData),
    Instrument(UnsubscribeInstrument),
    Instruments(UnsubscribeInstruments),
    BookDeltas(UnsubscribeBookDeltas),
    BookDepth(UnsubscribeBookDepth),
    BookSnapshots(UnsubscribeBookSnapshots),
    Quotes(UnsubscribeQuotes),
    Trades(UnsubscribeTrades),
    Bars(UnsubscribeBars),
    MarkPrices(UnsubscribeMarkPrices),
    IndexPrices(UnsubscribeIndexPrices),
    FundingRates(UnsubscribeFundingRates),
    InstrumentStatus(UnsubscribeInstrumentStatus),
    InstrumentClose(UnsubscribeInstrumentClose),
    OptionGreeks(UnsubscribeOptionGreeks),
    OptionChain(UnsubscribeOptionChain),
}

impl PartialEq for UnsubscribeCommand {
    fn eq(&self, other: &Self) -> bool {
        self.command_id() == other.command_id()
    }
}

impl UnsubscribeCommand {
    /// Returns the instrument IDs for an instrument subscription, if applicable.
    #[must_use]
    pub fn instrument_ids(&self) -> Option<&[InstrumentId]> {
        match self {
            Self::Instrument(command) => Some(&command.instrument_ids),
            Self::BookDeltas(command) => Some(&command.instrument_ids),
            Self::BookDepth(command) => Some(&command.instrument_ids),
            Self::BookSnapshots(command) => Some(&command.instrument_ids),
            Self::Quotes(command) => Some(&command.instrument_ids),
            Self::Trades(command) => Some(&command.instrument_ids),
            Self::MarkPrices(command) => Some(&command.instrument_ids),
            Self::IndexPrices(command) => Some(&command.instrument_ids),
            Self::FundingRates(command) => Some(&command.instrument_ids),
            Self::InstrumentStatus(command) => Some(&command.instrument_ids),
            Self::InstrumentClose(command) => Some(&command.instrument_ids),
            Self::OptionGreeks(command) => Some(&command.instrument_ids),
            _ => None,
        }
    }

    /// Projects an instrument subscription onto the supplied IDs, preserving its metadata.
    ///
    /// # Panics
    ///
    /// Panics if an instrument subscription receives an empty identifier list.
    #[must_use]
    pub fn with_instrument_ids(&self, instrument_ids: impl IntoInstrumentIds) -> Option<Self> {
        match self {
            Self::Instrument(command) => Some(Self::Instrument(
                command.with_instrument_ids(instrument_ids),
            )),
            Self::BookDeltas(command) => Some(Self::BookDeltas(
                command.with_instrument_ids(instrument_ids),
            )),
            Self::BookDepth(command) => {
                Some(Self::BookDepth(command.with_instrument_ids(instrument_ids)))
            }
            Self::BookSnapshots(command) => Some(Self::BookSnapshots(
                command.with_instrument_ids(instrument_ids),
            )),
            Self::Quotes(command) => {
                Some(Self::Quotes(command.with_instrument_ids(instrument_ids)))
            }
            Self::Trades(command) => {
                Some(Self::Trades(command.with_instrument_ids(instrument_ids)))
            }
            Self::MarkPrices(command) => Some(Self::MarkPrices(
                command.with_instrument_ids(instrument_ids),
            )),
            Self::IndexPrices(command) => Some(Self::IndexPrices(
                command.with_instrument_ids(instrument_ids),
            )),
            Self::FundingRates(command) => Some(Self::FundingRates(
                command.with_instrument_ids(instrument_ids),
            )),
            Self::InstrumentStatus(command) => Some(Self::InstrumentStatus(
                command.with_instrument_ids(instrument_ids),
            )),
            Self::InstrumentClose(command) => Some(Self::InstrumentClose(
                command.with_instrument_ids(instrument_ids),
            )),
            Self::OptionGreeks(command) => Some(Self::OptionGreeks(
                command.with_instrument_ids(instrument_ids),
            )),
            _ => None,
        }
    }

    /// Returns the bar types for a bar subscription, if applicable.
    #[must_use]
    pub fn bar_types(&self) -> Option<&[BarType]> {
        match self {
            Self::Bars(command) => Some(&command.bar_types),
            _ => None,
        }
    }

    /// Projects a bar subscription onto the supplied types, preserving its metadata.
    ///
    /// # Panics
    ///
    /// Panics if a bar subscription receives an empty list.
    #[must_use]
    pub fn with_bar_types(&self, bar_types: impl IntoBarTypes) -> Option<Self> {
        match self {
            Self::Bars(command) => Some(Self::Bars(command.with_bar_types(bar_types))),
            _ => None,
        }
    }

    /// Groups instrument releases with matching routing and command metadata.
    ///
    /// Bar releases with matching metadata merge into one command, with duplicate types removed.
    /// Other commands remain separate.
    ///
    /// # Panics
    ///
    /// Panics if an instrument or bar release group has no members.
    #[must_use]
    pub fn group(commands: Vec<Self>) -> Vec<Self> {
        let mut groups: Vec<(Self, Vec<InstrumentId>)> = Vec::new();

        for command in commands {
            let Some(ids) = command.instrument_ids() else {
                if groups
                    .iter_mut()
                    .any(|(batch, _)| batch.merge_bars(&command))
                {
                    continue;
                }

                groups.push((command, Vec::new()));
                continue;
            };

            if let Some((_, grouped_ids)) = groups.iter_mut().find(|(batch, _)| {
                batch.command_id() == command.command_id()
                    && batch.ts_init() == command.ts_init()
                    && batch.correlation_id() == command.correlation_id()
                    && batch.client_id() == command.client_id()
                    && batch.venue() == command.venue()
                    && match (batch, &command) {
                        (Self::Instrument(batch), Self::Instrument(member)) => {
                            batch.params == member.params
                        }
                        (Self::BookDeltas(batch), Self::BookDeltas(member)) => {
                            batch.params == member.params
                        }
                        (Self::BookDepth(batch), Self::BookDepth(member)) => {
                            batch.params == member.params
                        }
                        (Self::BookSnapshots(batch), Self::BookSnapshots(member)) => {
                            batch.params == member.params && batch.interval_ms == member.interval_ms
                        }
                        (Self::Quotes(batch), Self::Quotes(member)) => {
                            batch.params == member.params
                        }
                        (Self::Trades(batch), Self::Trades(member)) => {
                            batch.params == member.params
                        }
                        (Self::MarkPrices(batch), Self::MarkPrices(member)) => {
                            batch.params == member.params
                        }
                        (Self::IndexPrices(batch), Self::IndexPrices(member)) => {
                            batch.params == member.params
                        }
                        (Self::FundingRates(batch), Self::FundingRates(member)) => {
                            batch.params == member.params
                        }
                        (Self::InstrumentStatus(batch), Self::InstrumentStatus(member)) => {
                            batch.params == member.params
                        }
                        (Self::InstrumentClose(batch), Self::InstrumentClose(member)) => {
                            batch.params == member.params
                        }
                        (Self::OptionGreeks(batch), Self::OptionGreeks(member)) => {
                            batch.params == member.params
                        }
                        _ => false,
                    }
            }) {
                grouped_ids.extend_from_slice(ids);
                continue;
            }

            let ids = ids.to_vec();
            groups.push((command, ids));
        }

        groups
            .into_iter()
            .map(|(mut command, ids)| {
                if let Self::Bars(batch) = &mut command {
                    batch.bar_types = std::mem::take(&mut batch.bar_types)
                        .into_bar_types()
                        .expect("merged bar releases are nonempty");
                    command
                } else {
                    command.with_instrument_ids(ids).unwrap_or(command)
                }
            })
            .collect()
    }

    fn merge_bars(&mut self, other: &Self) -> bool {
        let (Self::Bars(batch), Self::Bars(member)) = (self, other) else {
            return false;
        };

        if batch.command_id != member.command_id
            || batch.ts_init != member.ts_init
            || batch.correlation_id != member.correlation_id
            || batch.client_id != member.client_id
            || batch.venue != member.venue
            || batch.params != member.params
        {
            return false;
        }

        batch.bar_types.extend_from_slice(&member.bar_types);
        true
    }

    /// Converts the command to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    pub fn command_id(&self) -> UUID4 {
        match self {
            Self::Data(cmd) => cmd.command_id,
            Self::Instrument(cmd) => cmd.command_id,
            Self::Instruments(cmd) => cmd.command_id,
            Self::BookDeltas(cmd) => cmd.command_id,
            Self::BookDepth(cmd) => cmd.command_id,
            Self::BookSnapshots(cmd) => cmd.command_id,
            Self::Quotes(cmd) => cmd.command_id,
            Self::Trades(cmd) => cmd.command_id,
            Self::Bars(cmd) => cmd.command_id,
            Self::MarkPrices(cmd) => cmd.command_id,
            Self::IndexPrices(cmd) => cmd.command_id,
            Self::FundingRates(cmd) => cmd.command_id,
            Self::InstrumentStatus(cmd) => cmd.command_id,
            Self::InstrumentClose(cmd) => cmd.command_id,
            Self::OptionGreeks(cmd) => cmd.command_id,
            Self::OptionChain(cmd) => cmd.command_id,
        }
    }

    pub fn client_id(&self) -> Option<&ClientId> {
        match self {
            Self::Data(cmd) => cmd.client_id.as_ref(),
            Self::Instrument(cmd) => cmd.client_id.as_ref(),
            Self::Instruments(cmd) => cmd.client_id.as_ref(),
            Self::BookDeltas(cmd) => cmd.client_id.as_ref(),
            Self::BookDepth(cmd) => cmd.client_id.as_ref(),
            Self::BookSnapshots(cmd) => cmd.client_id.as_ref(),
            Self::Quotes(cmd) => cmd.client_id.as_ref(),
            Self::Trades(cmd) => cmd.client_id.as_ref(),
            Self::Bars(cmd) => cmd.client_id.as_ref(),
            Self::MarkPrices(cmd) => cmd.client_id.as_ref(),
            Self::IndexPrices(cmd) => cmd.client_id.as_ref(),
            Self::FundingRates(cmd) => cmd.client_id.as_ref(),
            Self::InstrumentStatus(cmd) => cmd.client_id.as_ref(),
            Self::InstrumentClose(cmd) => cmd.client_id.as_ref(),
            Self::OptionGreeks(cmd) => cmd.client_id.as_ref(),
            Self::OptionChain(cmd) => cmd.client_id.as_ref(),
        }
    }

    pub fn venue(&self) -> Option<&Venue> {
        match self {
            Self::Data(cmd) => cmd.venue.as_ref(),
            Self::Instrument(cmd) => cmd.venue.as_ref(),
            Self::Instruments(cmd) => Some(&cmd.venue),
            Self::BookDeltas(cmd) => cmd.venue.as_ref(),
            Self::BookDepth(cmd) => cmd.venue.as_ref(),
            Self::BookSnapshots(cmd) => cmd.venue.as_ref(),
            Self::Quotes(cmd) => cmd.venue.as_ref(),
            Self::Trades(cmd) => cmd.venue.as_ref(),
            Self::Bars(cmd) => cmd.venue.as_ref(),
            Self::MarkPrices(cmd) => cmd.venue.as_ref(),
            Self::IndexPrices(cmd) => cmd.venue.as_ref(),
            Self::FundingRates(cmd) => cmd.venue.as_ref(),
            Self::InstrumentStatus(cmd) => cmd.venue.as_ref(),
            Self::InstrumentClose(cmd) => cmd.venue.as_ref(),
            Self::OptionGreeks(cmd) => cmd.venue.as_ref(),
            Self::OptionChain(cmd) => cmd.venue.as_ref(),
        }
    }

    pub fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Data(cmd) => cmd.ts_init,
            Self::Instrument(cmd) => cmd.ts_init,
            Self::Instruments(cmd) => cmd.ts_init,
            Self::BookDeltas(cmd) => cmd.ts_init,
            Self::BookDepth(cmd) => cmd.ts_init,
            Self::BookSnapshots(cmd) => cmd.ts_init,
            Self::Quotes(cmd) => cmd.ts_init,
            Self::Trades(cmd) => cmd.ts_init,
            Self::MarkPrices(cmd) => cmd.ts_init,
            Self::IndexPrices(cmd) => cmd.ts_init,
            Self::FundingRates(cmd) => cmd.ts_init,
            Self::Bars(cmd) => cmd.ts_init,
            Self::InstrumentStatus(cmd) => cmd.ts_init,
            Self::InstrumentClose(cmd) => cmd.ts_init,
            Self::OptionGreeks(cmd) => cmd.ts_init,
            Self::OptionChain(cmd) => cmd.ts_init,
        }
    }

    pub fn correlation_id(&self) -> Option<UUID4> {
        match self {
            Self::Data(cmd) => cmd.correlation_id,
            Self::Instrument(cmd) => cmd.correlation_id,
            Self::Instruments(cmd) => cmd.correlation_id,
            Self::BookDeltas(cmd) => cmd.correlation_id,
            Self::BookDepth(cmd) => cmd.correlation_id,
            Self::BookSnapshots(cmd) => cmd.correlation_id,
            Self::Quotes(cmd) => cmd.correlation_id,
            Self::Trades(cmd) => cmd.correlation_id,
            Self::MarkPrices(cmd) => cmd.correlation_id,
            Self::IndexPrices(cmd) => cmd.correlation_id,
            Self::FundingRates(cmd) => cmd.correlation_id,
            Self::Bars(cmd) => cmd.correlation_id,
            Self::InstrumentStatus(cmd) => cmd.correlation_id,
            Self::InstrumentClose(cmd) => cmd.correlation_id,
            Self::OptionGreeks(cmd) => cmd.correlation_id,
            Self::OptionChain(_) => None,
        }
    }
}

#[allow(
    clippy::ref_option,
    reason = "callers pass borrowed Option fields directly"
)]
fn check_client_id_or_venue(client_id: &Option<ClientId>, venue: &Option<Venue>) {
    assert!(
        client_id.is_some() || venue.is_some(),
        "Both `client_id` and `venue` were None"
    );
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RequestCommand {
    Data(RequestCustomData),
    Instrument(RequestInstrument),
    Instruments(RequestInstruments),
    BookSnapshot(RequestBookSnapshot),
    BookDeltas(RequestBookDeltas),
    BookDepth(RequestBookDepth),
    Quotes(RequestQuotes),
    Trades(RequestTrades),
    FundingRates(RequestFundingRates),
    OptionChainReferencePrice(RequestOptionChainReferencePrice),
    Bars(RequestBars),
    Join(RequestJoin),
}

impl PartialEq for RequestCommand {
    fn eq(&self, other: &Self) -> bool {
        self.request_id() == other.request_id()
    }
}

impl RequestCommand {
    /// Converts the command to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    pub fn request_id(&self) -> &UUID4 {
        match self {
            Self::Data(cmd) => &cmd.request_id,
            Self::Instrument(cmd) => &cmd.request_id,
            Self::Instruments(cmd) => &cmd.request_id,
            Self::BookSnapshot(cmd) => &cmd.request_id,
            Self::BookDeltas(cmd) => &cmd.request_id,
            Self::BookDepth(cmd) => &cmd.request_id,
            Self::Quotes(cmd) => &cmd.request_id,
            Self::Trades(cmd) => &cmd.request_id,
            Self::FundingRates(cmd) => &cmd.request_id,
            Self::OptionChainReferencePrice(cmd) => &cmd.request_id,
            Self::Bars(cmd) => &cmd.request_id,
            Self::Join(cmd) => &cmd.request_id,
        }
    }

    pub fn client_id(&self) -> Option<&ClientId> {
        match self {
            Self::Data(cmd) => Some(&cmd.client_id),
            Self::Instrument(cmd) => cmd.client_id.as_ref(),
            Self::Instruments(cmd) => cmd.client_id.as_ref(),
            Self::BookSnapshot(cmd) => cmd.client_id.as_ref(),
            Self::BookDeltas(cmd) => cmd.client_id.as_ref(),
            Self::BookDepth(cmd) => cmd.client_id.as_ref(),
            Self::Quotes(cmd) => cmd.client_id.as_ref(),
            Self::Trades(cmd) => cmd.client_id.as_ref(),
            Self::FundingRates(cmd) => cmd.client_id.as_ref(),
            Self::OptionChainReferencePrice(cmd) => cmd.client_id.as_ref(),
            Self::Bars(cmd) => cmd.client_id.as_ref(),
            Self::Join(_) => None,
        }
    }

    pub fn venue(&self) -> Option<&Venue> {
        match self {
            Self::Data(_) => None,
            Self::Instrument(cmd) => first_venue(&cmd.instrument_ids),
            Self::Instruments(cmd) => cmd.venue.as_ref(),
            Self::BookSnapshot(cmd) => first_venue(&cmd.instrument_ids),
            Self::BookDeltas(cmd) => first_venue(&cmd.instrument_ids),
            Self::BookDepth(cmd) => first_venue(&cmd.instrument_ids),
            Self::Quotes(cmd) => first_venue(&cmd.instrument_ids),
            Self::Trades(cmd) => first_venue(&cmd.instrument_ids),
            Self::FundingRates(cmd) => first_venue(&cmd.instrument_ids),
            Self::OptionChainReferencePrice(cmd) => Some(&cmd.series_id.venue),
            // TODO: Extract the below somewhere
            Self::Bars(cmd) => match cmd.bar_types.first()? {
                BarType::Standard { instrument_id, .. } => Some(&instrument_id.venue),
                BarType::Composite { instrument_id, .. } => Some(&instrument_id.venue),
            },
            Self::Join(_) => None,
        }
    }

    pub fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Data(cmd) => cmd.ts_init,
            Self::Instrument(cmd) => cmd.ts_init,
            Self::Instruments(cmd) => cmd.ts_init,
            Self::BookSnapshot(cmd) => cmd.ts_init,
            Self::BookDeltas(cmd) => cmd.ts_init,
            Self::BookDepth(cmd) => cmd.ts_init,
            Self::Quotes(cmd) => cmd.ts_init,
            Self::Trades(cmd) => cmd.ts_init,
            Self::FundingRates(cmd) => cmd.ts_init,
            Self::OptionChainReferencePrice(cmd) => cmd.ts_init,
            Self::Bars(cmd) => cmd.ts_init,
            Self::Join(cmd) => cmd.ts_init,
        }
    }
}

fn first_venue(ids: &[InstrumentId]) -> Option<&Venue> {
    ids.first().map(|id| &id.venue)
}

#[derive(Clone, Debug)]
pub enum DataResponse {
    Data(CustomDataResponse),
    Instrument(Box<InstrumentResponse>),
    Instruments(InstrumentsResponse),
    Book(BookResponse),
    BookDeltas(BookDeltasResponse),
    BookDepth(BookDepthResponse),
    Quotes(QuotesResponse),
    Trades(TradesResponse),
    FundingRates(FundingRatesResponse),
    OptionChainReferencePrice(OptionChainReferencePriceResponse),
    Bars(BarsResponse),
}

/// A historical payload and the completion state of its root request.
///
/// A terminal envelope can omit its payload, including when a request fails before returning data.
/// `sub_correlation_id` identifies a source child without changing the root correlation ID.
#[derive(Clone, Debug, Serialize)]
pub struct DataResponseEnvelope {
    correlation_id: UUID4,
    response: Option<DataResponse>,
    completed: bool,
    sub_correlation_id: Option<UUID4>,
    failure_reason: Option<String>,
}

impl DataResponseEnvelope {
    /// Creates an envelope containing a payload.
    #[must_use]
    pub fn new(response: DataResponse, completed: bool, sub_correlation_id: Option<UUID4>) -> Self {
        Self {
            correlation_id: *response.correlation_id(),
            response: Some(response),
            completed,
            sub_correlation_id,
            failure_reason: None,
        }
    }

    /// Completes a request without a payload. A reason marks the request as failed.
    #[must_use]
    pub fn terminal(correlation_id: UUID4, failure_reason: Option<String>) -> Self {
        Self {
            correlation_id,
            response: None,
            completed: true,
            sub_correlation_id: None,
            failure_reason,
        }
    }

    /// Sets the root request ID while preserving the payload correlation ID.
    #[must_use]
    pub fn with_correlation_id(mut self, correlation_id: UUID4) -> Self {
        self.correlation_id = correlation_id;
        self
    }

    /// Completes a failed request while preserving the payload returned by successful work.
    #[must_use]
    pub fn with_failure(mut self, reason: String) -> Self {
        self.completed = true;
        self.failure_reason = Some(reason);
        self
    }

    #[must_use]
    pub const fn correlation_id(&self) -> &UUID4 {
        &self.correlation_id
    }

    #[must_use]
    pub const fn response(&self) -> Option<&DataResponse> {
        self.response.as_ref()
    }

    /// Returns the payload for in-place metadata updates or bounds trimming.
    pub const fn response_mut(&mut self) -> Option<&mut DataResponse> {
        self.response.as_mut()
    }

    #[must_use]
    pub const fn completed(&self) -> bool {
        self.completed
    }

    #[must_use]
    pub const fn sub_correlation_id(&self) -> Option<UUID4> {
        self.sub_correlation_id
    }

    #[must_use]
    pub fn failure_reason(&self) -> Option<&str> {
        self.failure_reason.as_deref()
    }
}

impl<'de> Deserialize<'de> for DataResponseEnvelope {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Wire {
            correlation_id: UUID4,
            response: Option<DataResponse>,
            completed: bool,
            sub_correlation_id: Option<UUID4>,
            failure_reason: Option<String>,
        }

        let wire = Wire::deserialize(deserializer)?;
        let completed = wire.completed || wire.failure_reason.is_some();

        if wire.response.is_none() && !completed {
            return Err(serde::de::Error::custom(
                "a payload-free response must complete its request",
            ));
        }

        Ok(Self {
            correlation_id: wire.correlation_id,
            response: wire.response,
            completed,
            sub_correlation_id: wire.sub_correlation_id,
            failure_reason: wire.failure_reason,
        })
    }
}

/// An adapter request failure, optionally carrying data collected before the failure.
#[derive(Clone, Debug)]
pub struct DataRequestFailed {
    pub request_id: UUID4,
    pub reason: String,
    pub response: Option<DataResponse>,
}

impl DataResponse {
    /// Returns the responding client.
    #[must_use]
    pub fn client_id(&self) -> &ClientId {
        match self {
            Self::Data(r) => &r.client_id,
            Self::Instrument(r) => &r.client_id,
            Self::Instruments(r) => &r.client_id,
            Self::Book(r) => &r.client_id,
            Self::BookDeltas(r) => &r.client_id,
            Self::BookDepth(r) => &r.client_id,
            Self::Quotes(r) => &r.client_id,
            Self::Trades(r) => &r.client_id,
            Self::FundingRates(r) => &r.client_id,
            Self::OptionChainReferencePrice(r) => &r.client_id,
            Self::Bars(r) => &r.client_id,
        }
    }

    /// Returns response parameters.
    #[must_use]
    pub fn params(&self) -> Option<&Params> {
        match self {
            Self::Data(r) => r.params.as_ref(),
            Self::Instrument(r) => r.params.as_ref(),
            Self::Instruments(r) => r.params.as_ref(),
            Self::Book(r) => r.params.as_ref(),
            Self::BookDeltas(r) => r.params.as_ref(),
            Self::BookDepth(r) => r.params.as_ref(),
            Self::Quotes(r) => r.params.as_ref(),
            Self::Trades(r) => r.params.as_ref(),
            Self::FundingRates(r) => r.params.as_ref(),
            Self::OptionChainReferencePrice(r) => r.params.as_ref(),
            Self::Bars(r) => r.params.as_ref(),
        }
    }

    /// Returns the inclusive start bound.
    #[must_use]
    pub fn start(&self) -> Option<UnixNanos> {
        match self {
            Self::Data(r) => r.start,
            Self::Instrument(r) => r.start,
            Self::Instruments(r) => r.start,
            Self::Book(r) => r.start,
            Self::BookDeltas(r) => r.start,
            Self::BookDepth(r) => r.start,
            Self::Quotes(r) => r.start,
            Self::Trades(r) => r.start,
            Self::FundingRates(r) => r.start,
            Self::OptionChainReferencePrice(_) => None,
            Self::Bars(r) => r.start,
        }
    }

    /// Returns the inclusive end bound.
    #[must_use]
    pub fn end(&self) -> Option<UnixNanos> {
        match self {
            Self::Data(r) => r.end,
            Self::Instrument(r) => r.end,
            Self::Instruments(r) => r.end,
            Self::Book(r) => r.end,
            Self::BookDeltas(r) => r.end,
            Self::BookDepth(r) => r.end,
            Self::Quotes(r) => r.end,
            Self::Trades(r) => r.end,
            Self::FundingRates(r) => r.end,
            Self::OptionChainReferencePrice(_) => None,
            Self::Bars(r) => r.end,
        }
    }

    /// Returns the response creation timestamp.
    #[must_use]
    pub fn ts_init(&self) -> UnixNanos {
        match self {
            Self::Data(r) => r.ts_init,
            Self::Instrument(r) => r.ts_init,
            Self::Instruments(r) => r.ts_init,
            Self::Book(r) => r.ts_init,
            Self::BookDeltas(r) => r.ts_init,
            Self::BookDepth(r) => r.ts_init,
            Self::Quotes(r) => r.ts_init,
            Self::Trades(r) => r.ts_init,
            Self::FundingRates(r) => r.ts_init,
            Self::OptionChainReferencePrice(r) => r.ts_init,
            Self::Bars(r) => r.ts_init,
        }
    }

    /// Returns the payload's catalog data type.
    #[must_use]
    pub fn data_kind(&self) -> NautilusDataType {
        match self {
            Self::Data(r) => NautilusDataType::Custom {
                type_name: r.data_type.type_name().to_string(),
            },
            Self::Instrument(_) | Self::Instruments(_) => NautilusDataType::Instrument,
            Self::Book(_) | Self::BookDeltas(_) => NautilusDataType::OrderBookDelta,
            Self::BookDepth(_) => NautilusDataType::OrderBookDepth,
            Self::Quotes(_) => NautilusDataType::QuoteTick,
            Self::Trades(_) => NautilusDataType::TradeTick,
            Self::FundingRates(_) => NautilusDataType::FundingRateUpdate,
            Self::Bars(_) => NautilusDataType::Bar,
            Self::OptionChainReferencePrice(_) => NautilusDataType::Custom {
                type_name: "OptionChainReferencePrice".to_string(),
            },
        }
    }

    /// Converts the command to a dyn Any trait object for messaging.
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    pub fn correlation_id(&self) -> &UUID4 {
        match self {
            Self::Data(resp) => &resp.correlation_id,
            Self::Instrument(resp) => &resp.correlation_id,
            Self::Instruments(resp) => &resp.correlation_id,
            Self::Book(resp) => &resp.correlation_id,
            Self::BookDeltas(resp) => &resp.correlation_id,
            Self::BookDepth(resp) => &resp.correlation_id,
            Self::Quotes(resp) => &resp.correlation_id,
            Self::Trades(resp) => &resp.correlation_id,
            Self::FundingRates(resp) => &resp.correlation_id,
            Self::OptionChainReferencePrice(resp) => &resp.correlation_id,
            Self::Bars(resp) => &resp.correlation_id,
        }
    }

    /// Returns a short variant name for compact logging.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Data(_) => "Data",
            Self::Instrument(_) => "Instrument",
            Self::Instruments(_) => "Instruments",
            Self::Book(_) => "Book",
            Self::BookDeltas(_) => "BookDeltas",
            Self::BookDepth(_) => "BookDepth",
            Self::Quotes(_) => "Quotes",
            Self::Trades(_) => "Trades",
            Self::FundingRates(_) => "FundingRates",
            Self::OptionChainReferencePrice(_) => "OptionChainReferencePrice",
            Self::Bars(_) => "Bars",
        }
    }

    /// Returns the number of records carried by the response, where defined.
    ///
    /// Returns `None` for singular or opaque variants (`Data`, `Instrument`, `Book`)
    /// where a record count is not meaningful.
    #[must_use]
    pub fn record_count(&self) -> Option<usize> {
        match self {
            Self::Data(_) | Self::Instrument(_) | Self::Book(_) => None,
            Self::Instruments(resp) => Some(resp.data.len()),
            Self::BookDeltas(resp) => Some(resp.data.len()),
            Self::BookDepth(resp) => Some(resp.data.len()),
            Self::Quotes(resp) => Some(resp.data.len()),
            Self::Trades(resp) => Some(resp.data.len()),
            Self::FundingRates(resp) => Some(resp.data.len()),
            Self::OptionChainReferencePrice(_) => None,
            Self::Bars(resp) => Some(resp.data.len()),
        }
    }

    /// Trims payloads to the inclusive `[start, end]` window on `ts_init`.
    ///
    /// Applies to variants whose payload elements implement `HasTsInit`
    /// (`BookDeltas`, `BookDepth`, `Quotes`, `Trades`, `FundingRates`,
    /// `Bars`, `Instruments`). Other variants are untouched: singular payloads
    /// (`Instrument`, `Book`),
    /// `OptionChainReferencePrice` (singular), and the opaque custom
    /// `Data` variant.
    pub fn trim_to_bounds(&mut self) {
        match self {
            Self::Quotes(r) => response::trim_batch_to_bounds(&mut r.data, r.start, r.end),
            Self::Trades(r) => response::trim_batch_to_bounds(&mut r.data, r.start, r.end),
            Self::FundingRates(r) => {
                response::trim_batch_to_bounds(&mut r.data, r.start, r.end);
            }
            Self::Bars(r) => response::trim_batch_to_bounds(&mut r.data, r.start, r.end),
            Self::Instruments(r) => response::trim_data_to_bounds(&mut r.data, r.start, r.end),
            Self::BookDeltas(r) => {
                response::trim_batch_to_bounds(&mut r.data, r.start, r.end);
            }
            Self::BookDepth(r) => response::trim_batch_to_bounds(&mut r.data, r.start, r.end),
            Self::Data(_)
            | Self::Instrument(_)
            | Self::Book(_)
            | Self::OptionChainReferencePrice(_) => {}
        }
    }
}

pub type Payload = Arc<dyn Any + Send + Sync>;

/// Returns `true` when `params` carries the [`PARAMS_IS_PARENT`] flag set to `true`.
///
/// Absent or non-boolean values resolve to `false`, keeping the default subscription
/// path concrete (exact topic).
#[must_use]
pub fn is_parent_subscription(params: Option<&Params>) -> bool {
    params
        .and_then(|p| p.get_bool(PARAMS_IS_PARENT))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest::rstest]
    fn failure_envelopes_complete_the_request() {
        let response = QuotesResponse::new(
            UUID4::new(),
            ClientId::from("SOURCE"),
            InstrumentId::from("AUD/USD.SIM"),
            Vec::new(),
            Some(UnixNanos::from(3)),
            Some(UnixNanos::from(17)),
            UnixNanos::from(29),
            None,
        );
        let source = UUID4::new();
        let intermediate =
            DataResponseEnvelope::new(DataResponse::Quotes(response), false, Some(source));
        let mut expected = serde_json::to_value(&intermediate).unwrap();
        expected["completed"] = serde_json::json!(true);
        expected["failure_reason"] = serde_json::json!("Missing dataset");
        let failure = intermediate.with_failure("Missing dataset".to_owned());
        assert_eq!(serde_json::to_value(failure).unwrap(), expected);
    }

    #[rstest::rstest]
    fn deserialized_failure_envelopes_complete_the_request() {
        let expected =
            DataResponseEnvelope::terminal(UUID4::new(), Some("Missing dataset".to_owned()));
        let expected_value = serde_json::to_value(&expected).unwrap();
        let mut wire = expected_value.clone();
        wire["completed"] = serde_json::json!(false);
        let actual: DataResponseEnvelope = serde_json::from_value(wire).unwrap();
        assert_eq!(serde_json::to_value(actual).unwrap(), expected_value);
    }

    macro_rules! identifier_message_tests {
        ($projection_test:ident, $release_test:ident, $serde_test:ident, $subscribe:ident, $unsubscribe:ident, $variant:ident) => {
            #[rstest]
            #[case::subscribe(true)]
            #[case::unsubscribe(false)]
            fn $projection_test(#[case] subscribe: bool) {
                let first = InstrumentId::from("ESM4.GLBX");
                let second = InstrumentId::from("NQM4.GLBX");
                let command_id = UUID4::new();
                let correlation_id = UUID4::new();
                let params: Params =
                    serde_json::from_value(serde_json::json!({"price_precision": 3})).unwrap();

                let command = $subscribe::new(
                    [first, second],
                    Some(ClientId::from("QUOTE-CLIENT")),
                    Some(first.venue),
                    command_id,
                    UnixNanos::from(37),
                    Some(correlation_id),
                    Some(params),
                );
                let original = serde_json::to_value(&command).unwrap();
                let (before, after, dispatch, full_id, dispatch_id) = if subscribe {
                    let dispatch = command.with_instrument_ids(second);
                    (
                        original,
                        serde_json::to_value(&command).unwrap(),
                        serde_json::to_value(&dispatch).unwrap(),
                        command.single_instrument_id(),
                        dispatch.single_instrument_id(),
                    )
                } else {
                    let command = SubscribeCommand::$variant(command)
                        .into_unsubscribe(command_id, UnixNanos::from(37), Some(correlation_id));

                    let UnsubscribeCommand::$variant(command) = command else {
                        unreachable!()
                    };

                    let before = serde_json::to_value(&command).unwrap();
                    let dispatch = command.with_instrument_ids(second);
                    (
                        before,
                        serde_json::to_value(&command).unwrap(),
                        serde_json::to_value(&dispatch).unwrap(),
                        command.single_instrument_id(),
                        dispatch.single_instrument_id(),
                    )
                };

                let mut expected = before.clone();
                expected["instrument_ids"] = serde_json::to_value([second]).unwrap();
                assert_eq!(after, before);
                assert_eq!(dispatch, expected);
                assert_eq!(
                    full_id.unwrap_err().to_string(),
                    "subscription requires one instrument ID on this adapter path, received 2"
                );
                assert_eq!(dispatch_id.unwrap(), second);
            }

            #[rstest]
            fn $release_test(
                #[values(
                    "client_id",
                    "venue",
                    "params",
                    "command_id",
                    "ts_init",
                    "correlation_id"
                )]
                field: &str,
            ) {
                let first = InstrumentId::from("ESM4.GLBX");
                let second = InstrumentId::from("NQM4.GLBX");
                let third = InstrumentId::from("YMM4.GLBX");
                let params: Params =
                    serde_json::from_value(serde_json::json!({"route": 59})).unwrap();

                let first_command = $unsubscribe::new(
                    first,
                    Some(ClientId::from("FIRST")),
                    Some(first.venue),
                    UUID4::new(),
                    UnixNanos::from(41),
                    Some(UUID4::new()),
                    Some(params),
                );

                let second_command = first_command.with_instrument_ids(third);

                let other_route = match field {
                    "client_id" => $unsubscribe {
                        client_id: Some(ClientId::from("SECOND")),
                        ..second_command
                    },
                    "venue" => $unsubscribe {
                        venue: Some(Venue::from("XCBO")),
                        ..second_command
                    },
                    "params" => $unsubscribe {
                        params: Some(
                            serde_json::from_value(serde_json::json!({"route": 97})).unwrap(),
                        ),
                        ..second_command
                    },
                    "command_id" => $unsubscribe {
                        command_id: UUID4::new(),
                        ..second_command
                    },
                    "ts_init" => $unsubscribe {
                        ts_init: UnixNanos::from(43),
                        ..second_command
                    },
                    "correlation_id" => $unsubscribe {
                        correlation_id: Some(UUID4::new()),
                        ..second_command
                    },
                    _ => unreachable!(),
                };

                let commands = vec![
                    UnsubscribeCommand::$variant(first_command.clone()),
                    UnsubscribeCommand::$variant(other_route.clone()),
                    UnsubscribeCommand::$variant(
                        first_command.with_instrument_ids(second),
                    ),
                ];
                let before = serde_json::to_value(&commands).unwrap();
                let grouped = UnsubscribeCommand::group(commands.clone());
                let expected = vec![
                    UnsubscribeCommand::$variant(
                        first_command.with_instrument_ids([first, second]),
                    ),
                    UnsubscribeCommand::$variant(other_route),
                ];
                assert_eq!(serde_json::to_value(commands).unwrap(), before);
                assert_eq!(
                    serde_json::to_value(grouped).unwrap(),
                    serde_json::to_value(expected).unwrap()
                );
            }

            #[rstest]
            fn $serde_test() {
                let first = InstrumentId::from("ESM4.GLBX");
                let second = InstrumentId::from("NQM4.GLBX");

                let command = $subscribe::new(
                    [second, first, second],
                    None,
                    Some(first.venue),
                    UUID4::new(),
                    UnixNanos::from(53),
                    Some(UUID4::new()),
                    None,
                );
                let mut serialized = serde_json::to_value(&command).unwrap();
                serialized["instrument_ids"] =
                    serde_json::to_value([second, first, second]).unwrap();
                let subscribe: $subscribe =
                    serde_json::from_value(serialized.clone()).unwrap();
                let unsubscribe: $unsubscribe =
                    serde_json::from_value(serialized.clone()).unwrap();
                assert_eq!(
                    serde_json::to_value(subscribe).unwrap(),
                    serde_json::to_value(&command).unwrap()
                );
                assert_eq!(
                    serde_json::to_value(unsubscribe).unwrap(),
                    serde_json::to_value(&command).unwrap()
                );
                serialized["instrument_ids"] = serde_json::json!([]);
                assert!(
                    serde_json::from_value::<$subscribe>(serialized.clone())
                        .unwrap_err()
                        .to_string()
                        .contains("instrument IDs cannot be empty")
                );
                assert!(
                    serde_json::from_value::<$unsubscribe>(serialized)
                        .unwrap_err()
                        .to_string()
                        .contains("instrument IDs cannot be empty")
                );
            }
        };
    }

    identifier_message_tests!(
        quote_dispatch_preserves_original_intent,
        quote_release_grouping_preserves_inputs_and_retained_routes,
        quote_message_deserialization_normalizes_lists_and_rejects_empty,
        SubscribeQuotes,
        UnsubscribeQuotes,
        Quotes
    );
    identifier_message_tests!(
        trade_dispatch_preserves_original_intent,
        trade_release_grouping_preserves_inputs_and_retained_routes,
        trade_message_deserialization_normalizes_lists_and_rejects_empty,
        SubscribeTrades,
        UnsubscribeTrades,
        Trades
    );
    identifier_message_tests!(
        instrument_dispatch_preserves_original_intent,
        instrument_release_grouping_preserves_inputs_and_retained_routes,
        instrument_message_deserialization_normalizes_lists_and_rejects_empty,
        SubscribeInstrument,
        UnsubscribeInstrument,
        Instrument
    );
    identifier_message_tests!(
        mark_prices_dispatch_preserves_original_intent,
        mark_prices_release_grouping_preserves_inputs_and_retained_routes,
        mark_prices_message_deserialization_normalizes_lists_and_rejects_empty,
        SubscribeMarkPrices,
        UnsubscribeMarkPrices,
        MarkPrices
    );
    identifier_message_tests!(
        index_prices_dispatch_preserves_original_intent,
        index_prices_release_grouping_preserves_inputs_and_retained_routes,
        index_prices_message_deserialization_normalizes_lists_and_rejects_empty,
        SubscribeIndexPrices,
        UnsubscribeIndexPrices,
        IndexPrices
    );
    identifier_message_tests!(
        funding_rates_dispatch_preserves_original_intent,
        funding_rates_release_grouping_preserves_inputs_and_retained_routes,
        funding_rates_message_deserialization_normalizes_lists_and_rejects_empty,
        SubscribeFundingRates,
        UnsubscribeFundingRates,
        FundingRates
    );
    identifier_message_tests!(
        instrument_status_dispatch_preserves_original_intent,
        instrument_status_release_grouping_preserves_inputs_and_retained_routes,
        instrument_status_message_deserialization_normalizes_lists_and_rejects_empty,
        SubscribeInstrumentStatus,
        UnsubscribeInstrumentStatus,
        InstrumentStatus
    );
    identifier_message_tests!(
        instrument_close_dispatch_preserves_original_intent,
        instrument_close_release_grouping_preserves_inputs_and_retained_routes,
        instrument_close_message_deserialization_normalizes_lists_and_rejects_empty,
        SubscribeInstrumentClose,
        UnsubscribeInstrumentClose,
        InstrumentClose
    );
    identifier_message_tests!(
        option_greeks_dispatch_preserves_original_intent,
        option_greeks_release_grouping_preserves_inputs_and_retained_routes,
        option_greeks_message_deserialization_normalizes_lists_and_rejects_empty,
        SubscribeOptionGreeks,
        UnsubscribeOptionGreeks,
        OptionGreeks
    );

    #[rstest]
    #[case("BookDeltas")]
    #[case("BookDepth")]
    #[case("BookSnapshots")]
    fn test_book_identifier_lists_preserve_settings(#[case] family: &str) {
        let first = InstrumentId::from("ESM4.GLBX");
        let second = InstrumentId::from("NQM4.GLBX");
        let mut fields = serde_json::json!({
            "instrument_ids": [first, second, first], "book_type": "L2_MBP", "depth": 7,
            "client_id": "BOOKS", "venue": "GLBX", "command_id": UUID4::new(),
            "ts_init": 43, "correlation_id": UUID4::new(), "params": {"route": 59}
        });

        if family == "BookSnapshots" {
            fields["interval_ms"] = serde_json::json!(13);
        } else {
            fields["managed"] = serde_json::json!(true);
        }

        let command: SubscribeCommand =
            serde_json::from_value(serde_json::json!({family: fields})).unwrap();
        fields["instrument_ids"] = serde_json::json!([first, second]);
        let before = serde_json::to_value(&command).unwrap();
        let projected = command.with_instrument_ids(second).unwrap();
        let mut expected = fields.clone();
        expected["instrument_ids"] = serde_json::json!([second]);
        let inverse = command.clone().into_unsubscribe(
            command.command_id(),
            command.ts_init(),
            command.correlation_id(),
        );
        let mut expected_inverse = fields.clone();
        for field in ["book_type", "depth", "managed"] {
            expected_inverse.as_object_mut().unwrap().remove(field);
        }

        let mut empty = fields.clone();
        empty["instrument_ids"] = serde_json::json!([]);
        let error = serde_json::from_value::<SubscribeCommand>(serde_json::json!({family: empty}))
            .unwrap_err();
        let releases = [first, second].map(|id| inverse.with_instrument_ids(id).unwrap());
        let grouped = UnsubscribeCommand::group(releases.to_vec());
        assert_eq!(command.instrument_ids(), Some([first, second].as_slice()));
        assert_eq!(before, serde_json::json!({family: fields}));
        assert_eq!(serde_json::to_value(&command).unwrap(), before);
        assert_eq!(
            serde_json::to_value(projected).unwrap(),
            serde_json::json!({family: expected})
        );
        assert_eq!(
            serde_json::to_value(&inverse).unwrap(),
            serde_json::json!({family: expected_inverse})
        );
        assert_eq!(
            serde_json::to_value(grouped).unwrap(),
            serde_json::json!([{family: expected_inverse}])
        );
        assert!(error.to_string().contains("instrument IDs cannot be empty"));
    }

    #[rstest::rstest]
    #[case::subscribe(true)]
    #[case::unsubscribe(false)]
    fn bar_lists_serialize_normalize_and_project_metadata(#[case] subscribe: bool) {
        let first = BarType::from("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL");
        let second = BarType::from("AUD/USD.SIM-5-MINUTE-LAST-EXTERNAL");

        let command = SubscribeBars::new(
            [second, first, second],
            Some(ClientId::from("BARS")),
            Some(Venue::from("SIM")),
            UUID4::new(),
            UnixNanos::from(37),
            Some(UUID4::new()),
            Some(serde_json::from_value(serde_json::json!({"route": 59})).unwrap()),
        );

        let release = match SubscribeCommand::Bars(command.clone()).into_unsubscribe(
            UUID4::new(),
            UnixNanos::from(41),
            Some(UUID4::new()),
        ) {
            UnsubscribeCommand::Bars(command) => command,
            _ => unreachable!(),
        };

        let before = if subscribe {
            serde_json::to_value(&command).unwrap()
        } else {
            serde_json::to_value(&release).unwrap()
        };

        let projected = if subscribe {
            serde_json::to_value(command.with_bar_types(first)).unwrap()
        } else {
            serde_json::to_value(release.with_bar_types(first)).unwrap()
        };

        let mut expected = before.clone();
        expected["bar_types"] = serde_json::to_value([first]).unwrap();
        assert_eq!(projected, expected);
        assert_eq!(
            before["bar_types"],
            serde_json::to_value([second, first]).unwrap()
        );
        assert!(before.get("bar_type").is_none());
        assert_eq!(
            command.single_bar_type().unwrap_err().to_string(),
            "subscription requires one bar type on this adapter path, received 2"
        );
        assert_eq!(
            release.with_bar_types(first).single_bar_type().unwrap(),
            first
        );
        let mut repeated = before.clone();
        repeated["bar_types"] = serde_json::to_value([second, first, second]).unwrap();
        let normalized = if subscribe {
            serde_json::to_value(serde_json::from_value::<SubscribeBars>(repeated).unwrap())
                .unwrap()
        } else {
            serde_json::to_value(serde_json::from_value::<UnsubscribeBars>(repeated).unwrap())
                .unwrap()
        };

        assert_eq!(normalized, before);
        let mut empty = before;
        empty["bar_types"] = serde_json::json!([]);

        if subscribe {
            assert!(
                serde_json::from_value::<SubscribeBars>(empty)
                    .unwrap_err()
                    .to_string()
                    .contains("bar types cannot be empty")
            );
        } else {
            assert!(
                serde_json::from_value::<UnsubscribeBars>(empty)
                    .unwrap_err()
                    .to_string()
                    .contains("bar types cannot be empty")
            );
        }
    }

    #[rstest::rstest]
    fn bar_release_grouping_preserves_metadata_boundaries(
        #[values(
            "client_id",
            "venue",
            "params",
            "command_id",
            "ts_init",
            "correlation_id"
        )]
        field: &str,
    ) {
        let first = BarType::from("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL");
        let second = BarType::from("AUD/USD.SIM-5-MINUTE-LAST-EXTERNAL");

        let command = UnsubscribeBars::new(
            first,
            Some(ClientId::from("FIRST")),
            Some(Venue::from("SIM")),
            UUID4::new(),
            UnixNanos::from(41),
            Some(UUID4::new()),
            Some(serde_json::from_value(serde_json::json!({"route": 59})).unwrap()),
        );
        let same = command.with_bar_types([second, first]);
        let mut other = serde_json::to_value(command.with_bar_types(second)).unwrap();

        other[field] = match field {
            "client_id" => serde_json::to_value(ClientId::from("SECOND")).unwrap(),
            "venue" => serde_json::to_value(Venue::from("OTHER")).unwrap(),
            "params" => serde_json::json!({"route": 97}),
            "command_id" | "correlation_id" => serde_json::to_value(UUID4::new()).unwrap(),
            "ts_init" => serde_json::to_value(UnixNanos::from(43)).unwrap(),
            _ => unreachable!(),
        };

        let other: UnsubscribeBars = serde_json::from_value(other).unwrap();
        let expected = vec![
            UnsubscribeCommand::Bars(command.with_bar_types([first, second])),
            UnsubscribeCommand::Bars(other.clone()),
        ];
        let grouped = UnsubscribeCommand::group(vec![
            UnsubscribeCommand::Bars(command),
            UnsubscribeCommand::Bars(same),
            UnsubscribeCommand::Bars(other),
        ]);
        assert_eq!(
            serde_json::to_value(grouped).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
}
