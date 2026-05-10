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

//! Integration tests for Tardis data client and factory.

use std::{
    any::Any,
    cell::{OnceCell, RefCell},
    rc::Rc,
};

use nautilus_common::{
    cache::Cache, clock::TestClock, live::runner::set_data_event_sender, messages::DataEvent,
};
use nautilus_model::identifiers::ClientId;
use nautilus_system::factories::{ClientConfig, DataClientFactory};
use nautilus_tardis::{config::TardisDataClientConfig, factories::TardisDataClientFactory};
use rstest::rstest;

#[derive(Debug)]
struct WrongConfig;

impl ClientConfig for WrongConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn setup_test_env() {
    thread_local! {
        static INIT: OnceCell<()> = const { OnceCell::new() };
    }

    INIT.with(|cell| {
        cell.get_or_init(|| {
            let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
            set_data_event_sender(sender);
        });
    });
}

#[rstest]
fn test_tardis_data_client_factory_creation() {
    let factory = TardisDataClientFactory::new();
    assert_eq!(factory.name(), "TARDIS");
    assert_eq!(factory.config_type(), "TardisDataClientConfig");
}

#[rstest]
fn test_tardis_data_client_config_implements_client_config() {
    let config = TardisDataClientConfig::default();

    let boxed_config: Box<dyn ClientConfig> = Box::new(config);
    let downcasted = boxed_config
        .as_any()
        .downcast_ref::<TardisDataClientConfig>();

    assert!(downcasted.is_some());
}

#[rstest]
fn test_tardis_data_client_factory_creates_client() {
    setup_test_env();

    let factory = TardisDataClientFactory::new();
    let config = TardisDataClientConfig::default();

    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));

    let result = factory.create("TARDIS", &config, cache, clock);
    assert!(result.is_ok());

    let client = result.unwrap();
    assert_eq!(client.client_id(), ClientId::from("TARDIS"));
}

#[rstest]
fn test_client_initial_state() {
    setup_test_env();

    let factory = TardisDataClientFactory::new();
    let config = TardisDataClientConfig::default();

    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));

    let client = factory.create("TARDIS", &config, cache, clock).unwrap();
    assert!(!client.is_connected());
    assert!(client.is_disconnected());
    assert!(client.venue().is_none());
}

#[rstest]
fn test_factory_create_wrong_config_type_errors() {
    setup_test_env();

    let factory = TardisDataClientFactory::new();
    let config = WrongConfig;

    let cache = Rc::new(RefCell::new(Cache::default()));
    let clock = Rc::new(RefCell::new(TestClock::new()));

    let result = factory.create("TARDIS", &config, cache, clock);
    assert!(result.is_err());

    let err = result.err().unwrap();
    assert!(err.to_string().contains("Invalid config type"));
}
