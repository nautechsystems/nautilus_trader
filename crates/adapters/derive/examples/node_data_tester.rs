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

//! Example demonstrating live data testing with the Derive adapter.
//!
//! Run with: `cargo run --example derive-data-tester --package nautilus-derive --features examples`
//!
//! Uses Derive testnet by default. Set `DERIVE_ENVIRONMENT=mainnet` to use production data.
//! Set `DERIVE_DATA_TESTER_KIND=perp|option|spot` to switch the product.
//! Set `DERIVE_DATA_TESTER_SYMBOL` to test a specific venue symbol.

use nautilus_common::enums::Environment;
use nautilus_derive::{
    common::{
        consts::DERIVE_CLIENT_ID,
        enums::{DeriveEnvironment, DeriveInstrumentType},
    },
    config::DeriveDataClientConfig,
    factories::DeriveDataClientFactory,
};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    data::BarType,
    identifiers::{InstrumentId, TraderId},
    stubs::TestDefault,
};
use nautilus_testkit::testers::{DataTester, DataTesterConfig};

/// Base token used to build the instrument symbol.
const TOKEN: &str = "ETH";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let derive_environment = derive_environment_from_env();
    let trader_id = TraderId::test_default();
    let node_name = "DERIVE-DATA-TESTER-001".to_string();

    let setup = InstrumentSetup::from_env(TOKEN)?;
    let instrument_id = InstrumentId::from(format!("{}.DERIVE", setup.symbol).as_str());
    let bar_type =
        BarType::from(format!("{}.DERIVE-15-MINUTE-LAST-EXTERNAL", setup.symbol).as_str());

    let derive_config = DeriveDataClientConfig {
        environment: derive_environment,
        currencies: vec![TOKEN.to_string()],
        ..Default::default()
    };

    let client_factory = DeriveDataClientFactory::new();
    let client_id = *DERIVE_CLIENT_ID;

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name(node_name)
        .with_delay_post_stop_secs(2)
        .add_data_client(None, Box::new(client_factory), Box::new(derive_config))?
        .build()?;

    let tester_config = DataTesterConfig::builder()
        .client_id(client_id)
        .instrument_ids(vec![instrument_id])
        .bar_types(vec![bar_type])
        // .subscribe_book_deltas(true)
        .subscribe_quotes(true)
        .subscribe_trades(true)
        .subscribe_mark_prices(true)
        .subscribe_index_prices(true)
        .subscribe_funding_rates(setup.has_funding)
        .subscribe_option_greeks(setup.has_greeks)
        .request_instruments(true)
        .request_trades(true)
        .request_bars(true)
        .request_funding_rates(setup.has_funding)
        .manage_book(true)
        .build();
    let tester = DataTester::new(tester_config);

    node.add_actor(tester)?;
    node.run().await?;

    Ok(())
}

/// Per-product symbol and feed setup, resolved from the selected [`DeriveInstrumentType`].
struct InstrumentSetup {
    symbol: String,
    has_funding: bool,
    has_greeks: bool,
}

impl InstrumentSetup {
    fn from_env(token: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let kind = instrument_type_from_env()?;
        let mut setup = Self::resolve(kind, token);

        if let Ok(symbol) = std::env::var("DERIVE_DATA_TESTER_SYMBOL") {
            if !symbol.trim().is_empty() {
                setup.symbol = symbol.trim().to_string();
            }
        }

        Ok(setup)
    }

    fn resolve(kind: DeriveInstrumentType, token: &str) -> Self {
        match kind {
            DeriveInstrumentType::Perp => Self {
                symbol: format!("{token}-PERP"),
                has_funding: true,
                has_greeks: false,
            },
            // Expiry and strike are illustrative; set a live option contract before running.
            DeriveInstrumentType::Option => Self {
                symbol: format!("{token}-20260627-3500-C"),
                has_funding: false,
                has_greeks: true,
            },
            // Spot is the venue's ERC-20 product, quoted in USDC.
            DeriveInstrumentType::Erc20 => Self {
                symbol: format!("{token}-USDC"),
                has_funding: false,
                has_greeks: false,
            },
        }
    }
}

fn instrument_type_from_env() -> Result<DeriveInstrumentType, Box<dyn std::error::Error>> {
    match std::env::var("DERIVE_DATA_TESTER_KIND") {
        Ok(value) if value.eq_ignore_ascii_case("spot") => Ok(DeriveInstrumentType::Erc20),
        Ok(value) if value.eq_ignore_ascii_case("erc20") => Ok(DeriveInstrumentType::Erc20),
        Ok(value) if value.eq_ignore_ascii_case("option") => Ok(DeriveInstrumentType::Option),
        Ok(value) if value.eq_ignore_ascii_case("perp") => Ok(DeriveInstrumentType::Perp),
        Ok(value) if value.eq_ignore_ascii_case("perpetual") => Ok(DeriveInstrumentType::Perp),
        Ok(value) => Err(format!("unsupported DERIVE_DATA_TESTER_KIND: {value}").into()),
        Err(_) => Ok(DeriveInstrumentType::Perp),
    }
}

fn derive_environment_from_env() -> DeriveEnvironment {
    match std::env::var("DERIVE_ENVIRONMENT") {
        Ok(value)
            if value.eq_ignore_ascii_case("mainnet") || value.eq_ignore_ascii_case("live") =>
        {
            DeriveEnvironment::Mainnet
        }
        _ => DeriveEnvironment::Testnet,
    }
}
