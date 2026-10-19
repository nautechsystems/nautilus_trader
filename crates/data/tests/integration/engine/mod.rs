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

mod bars;
mod book;
mod catalog;
mod clients;
mod common;
mod continuous_futures;
mod engine_misc;
mod historical;
mod instruments;
mod market_data;
mod option_chain;
mod pipelines;
mod requests;
mod spread;
mod subscriptions;

#[cfg(feature = "streaming")]
use std::path::{Path, PathBuf};
use std::{any::Any, cell::RefCell, num::NonZeroUsize, rc::Rc, time::Duration};
#[cfg(feature = "defi")]
use std::{str::FromStr, sync::Arc};

#[cfg(feature = "defi")]
use alloy_primitives::{Address, I256, U160, U256};
use common::*;
#[cfg(feature = "defi")]
use nautilus_common::defi;
#[cfg(feature = "defi")]
use nautilus_common::messages::defi::{
    DefiRequestCommand, DefiSubscribeCommand, DefiUnsubscribeCommand, RequestPoolSnapshot,
    SubscribeBlocks, SubscribePool, SubscribePoolFeeCollects, SubscribePoolFlashEvents,
    SubscribePoolLiquidityUpdates, SubscribePoolSwaps, UnsubscribeBlocks,
    UnsubscribePoolFeeCollects, UnsubscribePoolFlashEvents, UnsubscribePoolLiquidityUpdates,
    UnsubscribePoolSwaps,
};
use nautilus_common::{
    cache::Cache,
    clients::DataClient,
    clock::{Clock, VirtualClock},
    messages::data::{
        BarsResponse, BookDeltasResponse, BookDepthResponse, BookResponse, CustomDataResponse,
        DataCommand, DataResponse, FundingRatesResponse, InstrumentResponse, InstrumentsResponse,
        OptionChainReferencePriceResponse, PARAMS_IS_PARENT, QuotesResponse, RequestBars,
        RequestBookDeltas, RequestBookDepth, RequestBookSnapshot, RequestCommand,
        RequestCustomData, RequestFundingRates, RequestInstrument, RequestInstruments, RequestJoin,
        RequestOptionChainReferencePrice, RequestQuotes, RequestTrades, SubscribeBars,
        SubscribeBookDeltas, SubscribeBookDepth, SubscribeBookSnapshots, SubscribeCommand,
        SubscribeCustomData, SubscribeFundingRates, SubscribeIndexPrices, SubscribeInstrument,
        SubscribeInstrumentClose, SubscribeInstrumentStatus, SubscribeInstruments,
        SubscribeMarkPrices, SubscribeOptionChain, SubscribeOptionGreeks, SubscribeQuotes,
        SubscribeTrades, TradesResponse, UnsubscribeBars, UnsubscribeBookDeltas,
        UnsubscribeBookDepth, UnsubscribeBookSnapshots, UnsubscribeCommand, UnsubscribeCustomData,
        UnsubscribeFundingRates, UnsubscribeIndexPrices, UnsubscribeInstrument,
        UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus, UnsubscribeMarkPrices,
        UnsubscribeOptionChain, UnsubscribeOptionGreeks, UnsubscribeQuotes, UnsubscribeTrades,
    },
    msgbus::{
        self, BusPayloadType, BusTap, Endpoint, MStr, MessageBus, Topic, TypedHandler,
        TypedIntoHandler,
        stubs::{get_any_saving_handler, get_typed_message_saving_handler},
        switchboard::{self, MessagingSwitchboard},
    },
    testing::wait_until,
};
use nautilus_core::{DurationNanos, Params, UUID4, UnixNanos, datetime::NANOSECONDS_IN_SECOND};
use nautilus_data::{
    client::DataClientAdapter,
    engine::{DataEngine, config::DataEngineConfig},
};
#[cfg(feature = "defi")]
use nautilus_model::defi::tick_map::tick_math::get_tick_at_sqrt_ratio;
#[cfg(feature = "defi")]
use nautilus_model::defi::{AmmType, Dex, DexType, chain::chains};
#[cfg(feature = "defi")]
use nautilus_model::defi::{
    Block, Blockchain, DefiData, Pool, PoolIdentifier, PoolLiquidityUpdate,
    PoolLiquidityUpdateType, PoolProfiler, PoolSwap, Token,
    data::PoolFeeCollect,
    data::PoolFlash,
    data::block::BlockPosition,
    pool_analysis::snapshot::{PoolAnalytics, PoolSnapshot, PoolState},
};
#[cfg(feature = "defi")]
use nautilus_model::enums::CurrencyType;
#[cfg(feature = "streaming")]
use nautilus_model::enums::{BookAction, OrderSide};
use nautilus_model::{
    data::{
        Bar, BarType, BookOrder, CustomData, DEPTH10_LEN, Data, DataRef, DataType,
        FundingRateUpdate, IndexPriceUpdate, InstrumentClose, InstrumentStatus, MarkPriceUpdate,
        OrderBookDelta, OrderBookDeltas, OrderBookDepth, QuoteTick, TradeTick,
        greeks::OptionGreekValues,
        option_chain::{OptionChainSlice, OptionGreeks, StrikeRange},
        stubs::{
            OrderBookDeltaTestBuilder, stub_custom_data, stub_delta, stub_deltas, stub_depth10,
        },
    },
    enums::{
        AggressorSide, AssetClass, BookType, GreeksConvention, InstrumentClass,
        InstrumentCloseType, MarketStatusAction, OptionKind, PriceType, RecordFlag,
    },
    identifiers::{ClientId, InstrumentId, OptionSeriesId, Symbol, TradeId, TraderId, Venue},
    instruments::{
        CurrencyPair, FuturesContract, FuturesSpread, Instrument, InstrumentAny, OptionContract,
        SyntheticInstrument,
        stubs::{audusd_sim, futures_spread_es, gbpusd_sim},
    },
    orderbook::OrderBook,
    stubs::TestDefault,
    types::{Currency, Price, Quantity},
};
#[cfg(feature = "streaming")]
use nautilus_persistence::backend::parquet::{
    catalog::ParquetDataCatalog, paths::timestamps_to_filename,
};
#[cfg(feature = "streaming")]
use nautilus_persistence::test_data::RustTestCustomData;
#[cfg(feature = "streaming")]
use nautilus_serialization::ensure_custom_data_registered;
use rstest::*;
use serde_json::{Value, json};
use ustr::Ustr;

#[cfg(feature = "defi")]
use crate::common::defi::make_initialized_pool_and_swap;
use crate::common::mocks::{FailingMockDataClient, MockDataClient, MockSubscribeFailure};
