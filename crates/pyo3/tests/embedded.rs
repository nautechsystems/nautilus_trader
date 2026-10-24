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

use std::path::Path;

use nautilus_model::identifiers::InstrumentId;
use nautilus_pyo3::register_embedded_module;
use pyo3::{prelude::*, types::PyList};
use rstest::rstest;

#[rstest]
fn test_embedded_module_shares_host_types_and_selects_adapters() {
    register_embedded_module();
    Python::initialize();

    Python::attach(|py| {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../python");
        let path = py.import("sys").unwrap().getattr("path").unwrap();
        path.cast::<PyList>()
            .unwrap()
            .insert(0, source.as_os_str())
            .unwrap();

        let module = py.import("_libnautilus").unwrap();
        let named = py.import("nautilus_trader._libnautilus").unwrap();
        let model = py.import("nautilus_trader.model").unwrap();
        let class = model.getattr("InstrumentId").unwrap();
        let value = class
            .call_method1("from_str", ("ETHUSDT.BINANCE",))
            .unwrap();
        let instrument_id = value.extract::<InstrumentId>().unwrap();

        assert!(module.is(&named));
        assert!(class.is(py.get_type::<InstrumentId>()));
        assert_eq!(instrument_id, InstrumentId::from("ETHUSDT.BINANCE"));

        for name in [
            "architect_ax",
            "binance",
            "bybit",
            "coinbase",
            "databento",
            "deribit",
            "derive",
            "dydx",
            "hyperliquid",
            "interactive_brokers",
            "kraken",
            "lighter",
            "okx",
            "polymarket",
            "sandbox",
            "tardis",
        ] {
            assert_eq!(
                module.hasattr(name).unwrap(),
                cfg!(feature = "adapters"),
                "{name}"
            );
        }

        assert_eq!(
            module.hasattr("betfair").unwrap(),
            cfg!(feature = "betfair")
        );
        assert_eq!(
            module.hasattr("coinapi").unwrap(),
            cfg!(feature = "coinapi")
        );
    });
}
