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

//! Reproduces a cache borrow panic during synchronous publication to another actor.
//!
//! `held` retains the producer's cache borrow across publication; `released` drops it first.
//! `checked` uses fallible cache access; the callback handler logs the error and continues.
//!
//! Run with `cargo run -p nautilus-common --example cache_reentrancy -- held`, `released`, or `checked`.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use bytes::Bytes;
use log::LevelFilter;
use nautilus_common::{
    actor::{
        DataActor, DataActorCore, DataActorNative,
        data_actor::DataActorConfig,
        registry::{get_actor_unchecked, register_actor},
    },
    cache::Cache,
    clock::VirtualClock,
    component::Component,
    logging::{config::LoggerConfig, init_logging},
    nautilus_actor,
    runner::{SyncDataCommandSender, set_data_cmd_sender},
    signal::Signal,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::identifiers::{ActorId, TraderId};

const CACHE_KEY: &str = "reentry-value";

fn main() -> anyhow::Result<()> {
    let scenario = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "held".to_string());

    if !matches!(scenario.as_str(), "held" | "released" | "checked") {
        anyhow::bail!("Usage: cache_reentrancy [held|released|checked]");
    }

    let trader_id = TraderId::from("TRADER-001");

    let _log_guard = init_logging(
        trader_id,
        UUID4::new(),
        LoggerConfig {
            stdout_level: LevelFilter::Warn,
            ..Default::default()
        },
        Default::default(),
    )?;

    set_data_cmd_sender(Arc::new(SyncDataCommandSender));
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    let cache = Rc::new(RefCell::new(Cache::new(None, None)));
    println!("Scenario: {scenario}");

    for name in ["A", "B"] {
        let mut actor = CacheActor {
            core: DataActorCore::new(DataActorConfig {
                actor_id: Some(ActorId::new(name)),
                ..Default::default()
            }),
            name,
            release_before_publish: scenario == "released",
            checked: scenario == "checked",
        };

        actor.register(trader_id, clock.clone(), cache.clone())?;
        let id = actor.actor_id().inner();
        register_actor(actor);
        get_actor_unchecked::<CacheActor>(&id).start()?;
    }

    let mut publisher = DataActorCore::new(DataActorConfig::default());
    publisher.register(trader_id, clock, cache)?;
    println!("PUBLISHER: publish request");
    publisher.publish_signal("request", "request".to_string(), UnixNanos::from(1));
    println!("PUBLISHER: publication returned");
    Ok(())
}

#[derive(Debug)]
struct CacheActor {
    core: DataActorCore,
    name: &'static str,
    release_before_publish: bool,
    checked: bool,
}

nautilus_actor!(CacheActor);

impl DataActor for CacheActor {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_signal(
            if self.name == "A" {
                "request"
            } else {
                "notice"
            },
            None,
        );

        Ok(())
    }

    fn on_signal(&mut self, _signal: &Signal) -> anyhow::Result<()> {
        if self.name == "A" {
            let cache = self.cache_rc();
            let mut cache = cache.borrow_mut();
            cache.add(CACHE_KEY, Bytes::from_static(b"updated"))?;
            println!("A: cache updated; mutable borrow held");

            if self.release_before_publish {
                drop(cache);
                println!("A: cache borrow released before publication");
            }

            self.publish_signal("notice", "updated".to_string(), UnixNanos::from(2));
            println!("A: nested publication returned");
        } else {
            println!("B: nested callback entered; reading cache");

            let cache = if self.checked {
                self.try_cache_ref()?
            } else {
                self.cache_ref()
            };

            println!("B: cached value = {:?}", cache.get(CACHE_KEY)?);
        }

        Ok(())
    }
}
