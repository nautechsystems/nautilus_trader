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

#![cfg(feature = "python")]

use nautilus_blockchain::{
    config::BlockchainExecutionClientConfig, python::blockchain as blockchain_pymodule,
};
use nautilus_model::{
    defi::chain::chains,
    identifiers::{AccountId, TraderId, Venue},
    stubs::TestDefault,
};
use nautilus_system::get_global_pyo3_registry;
use pyo3::{
    prelude::*,
    types::{PyDict, PyModule},
};

#[test]
fn test_pyo3_exec_factory_and_config_registry_roundtrip() {
    Python::initialize();
    Python::attach(|py| {
        let module = PyModule::new(py, "blockchain").expect("Module creation should succeed");
        blockchain_pymodule(py, &module).expect("Module init should succeed");
        module
            .getattr("BlockchainDataClientConfig")
            .expect("Data config should be exposed in blockchain pymodule with python feature");

        let registry = get_global_pyo3_registry();

        let factory_class = module
            .getattr("BlockchainExecutionClientFactory")
            .expect("Execution factory should be exposed in blockchain pymodule");
        let factory_any = factory_class
            .call0()
            .expect("Execution factory constructor should succeed")
            .unbind();

        let boxed_factory = registry
            .extract_exec_factory(py, factory_any)
            .expect("Execution factory extractor roundtrip should succeed");
        assert_eq!(boxed_factory.name(), "BLOCKCHAIN");
        assert_eq!(
            boxed_factory.config_type(),
            "BlockchainExecutionClientConfig"
        );

        let config = BlockchainExecutionClientConfig::new(
            TraderId::test_default(),
            AccountId::test_default(),
            Venue::new("Arbitrum:UniswapV3"),
            chains::ARBITRUM.clone(),
            String::from("0x49E96E255bA418d08E66c35b588E2f2F3766E1d0"),
            None,
            String::from("https://arb.example.com"),
            None,
        );
        let config_any = Py::new(py, config)
            .expect("Execution config should convert to PyAny")
            .into_any();

        let boxed_config = registry
            .extract_config(py, config_any)
            .expect("Execution config extractor roundtrip should succeed");
        assert!(
            boxed_config
                .as_any()
                .downcast_ref::<BlockchainExecutionClientConfig>()
                .is_some(),
            "Extracted config should downcast to BlockchainExecutionClientConfig"
        );

        let config_class = module
            .getattr("BlockchainExecutionClientConfig")
            .expect("Execution config class should be exposed in blockchain pymodule");
        let kwargs = PyDict::new(py);
        kwargs
            .set_item(
                "wallet_extra_tokens",
                vec![String::from("0x0000000000000000000000000000000000000001")],
            )
            .expect("Should set wallet_extra_tokens kwarg");
        kwargs
            .set_item(
                "wallet_allowance_spenders",
                vec![String::from("0x0000000000000000000000000000000000000002")],
            )
            .expect("Should set wallet_allowance_spenders kwarg");
        kwargs
            .set_item(
                "wallet_wnative_address",
                String::from("0x0000000000000000000000000000000000000003"),
            )
            .expect("Should set wallet_wnative_address kwarg");

        let py_config = config_class
            .call(
                (
                    TraderId::test_default(),
                    AccountId::test_default(),
                    Venue::new("Bsc:PancakeSwapV2"),
                    chains::BSC.clone(),
                    String::from("0x49E96E255bA418d08E66c35b588E2f2F3766E1d0"),
                    String::from("https://bsc.example.com"),
                    Option::<Vec<String>>::None,
                    Option::<u32>::None,
                ),
                Some(&kwargs),
            )
            .expect("Execution config should accept legacy positional args plus new keyword-only wallet args");

        let wallet_extra_tokens: Vec<String> = py_config
            .getattr("wallet_extra_tokens")
            .expect("Config should expose wallet_extra_tokens")
            .extract()
            .expect("wallet_extra_tokens should extract");
        assert_eq!(
            wallet_extra_tokens,
            vec![String::from("0x0000000000000000000000000000000000000001")]
        );

        let wallet_allowance_spenders: Vec<String> = py_config
            .getattr("wallet_allowance_spenders")
            .expect("Config should expose wallet_allowance_spenders")
            .extract()
            .expect("wallet_allowance_spenders should extract");
        assert_eq!(
            wallet_allowance_spenders,
            vec![String::from("0x0000000000000000000000000000000000000002")]
        );

        let wallet_wnative_address: Option<String> = py_config
            .getattr("wallet_wnative_address")
            .expect("Config should expose wallet_wnative_address")
            .extract()
            .expect("wallet_wnative_address should extract");
        assert_eq!(
            wallet_wnative_address,
            Some(String::from("0x0000000000000000000000000000000000000003"))
        );
    });
}
