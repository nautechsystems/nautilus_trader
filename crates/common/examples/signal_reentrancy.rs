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

//! Prints nested signal delivery: `self` republishes to the same actor;
//! `fanout` adds a second subscriber.
//!
//! Run with `cargo run -p nautilus-common --example signal_reentrancy -- self` or `fanout`.

use std::{cell::RefCell, rc::Rc, sync::Arc};

use log::LevelFilter;
use nautilus_common::{
    actor::{
        DataActor, DataActorCore,
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

fn main() -> anyhow::Result<()> {
    let scenario = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "fanout".to_string());

    if !matches!(scenario.as_str(), "self" | "fanout") {
        anyhow::bail!("Usage: signal_reentrancy [self|fanout]");
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
        if name == "B" && scenario == "self" {
            continue;
        }

        let mut actor = SignalActor {
            core: DataActorCore::new(DataActorConfig {
                actor_id: Some(ActorId::new(name)),
                ..Default::default()
            }),
            name,
        };

        actor.register(trader_id, clock.clone(), cache.clone())?;
        let id = actor.actor_id().inner();
        register_actor(actor);
        let mut actor = get_actor_unchecked::<SignalActor>(&id);
        actor.start()?;
    }

    let mut publisher = SignalActor {
        core: DataActorCore::new(DataActorConfig {
            actor_id: Some(ActorId::new("PUBLISHER")),
            ..Default::default()
        }),
        name: "PUBLISHER",
    };

    publisher.register(trader_id, clock, cache)?;
    println!("PUBLISHER: publish outer");
    publisher.publish_signal("reentry", "outer".to_string(), UnixNanos::from(1));
    println!("PUBLISHER: publication returned");
    println!("Completed without a propagated runtime error");
    Ok(())
}

#[derive(Debug)]
struct SignalActor {
    core: DataActorCore,
    name: &'static str,
}

nautilus_actor!(SignalActor);

impl DataActor for SignalActor {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let priority = if self.name == "A" { 100 } else { 10 };
        self.subscribe_signal("reentry", Some(priority));
        Ok(())
    }

    fn on_signal(&mut self, signal: &Signal) -> anyhow::Result<()> {
        println!("{}:{}:enter", self.name, signal.value);

        if self.name == "A" && signal.value == "outer" {
            self.publish_signal("reentry", "inner".to_string(), UnixNanos::from(2));
        }

        println!("{}:{}:exit", self.name, signal.value);
        Ok(())
    }
}
