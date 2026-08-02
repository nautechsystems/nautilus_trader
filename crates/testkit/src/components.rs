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

//! Stateful actor and strategy test components.

use indexmap::IndexMap;
use nautilus_common::{
    actor::{DataActor, DataActorCore, data_actor::DataActorConfig},
    nautilus_actor,
};
use nautilus_model::identifiers::{ActorId, StrategyId};
use nautilus_trading::{
    nautilus_strategy,
    strategy::{config::StrategyConfig, core::StrategyCore},
};

use crate::cache::TestCacheDatabaseControl;

/// Actor with observable state lifecycle callbacks.
#[derive(Debug)]
pub struct StateActor {
    core: DataActorCore,
    control: TestCacheDatabaseControl,
    state_load: Option<IndexMap<String, Vec<u8>>>,
    state_save: IndexMap<String, Vec<u8>>,
    fail_load: bool,
    fail_save: bool,
    fail_start: bool,
}

impl StateActor {
    /// Creates a stateful actor.
    #[must_use]
    pub fn new(
        actor_id: ActorId,
        control: TestCacheDatabaseControl,
        state_save: IndexMap<String, Vec<u8>>,
    ) -> Self {
        Self {
            core: DataActorCore::new(DataActorConfig {
                actor_id: Some(actor_id),
                ..Default::default()
            }),
            control,
            state_load: None,
            state_save,
            fail_load: false,
            fail_save: false,
            fail_start: false,
        }
    }

    /// Configures the actor load callback to fail.
    #[must_use]
    pub const fn with_fail_load(mut self) -> Self {
        self.fail_load = true;
        self
    }

    /// Configures the actor save callback to fail.
    #[must_use]
    pub const fn with_fail_save(mut self) -> Self {
        self.fail_save = true;
        self
    }

    /// Configures the actor start callback to fail.
    #[must_use]
    pub const fn with_fail_start(mut self) -> Self {
        self.fail_start = true;
        self
    }

    /// Returns the state received by the load callback.
    #[must_use]
    pub const fn state_load(&self) -> Option<&IndexMap<String, Vec<u8>>> {
        self.state_load.as_ref()
    }
}

impl DataActor for StateActor {
    fn on_load(&mut self, state: IndexMap<String, Vec<u8>>) -> anyhow::Result<()> {
        self.control.record("actor.on_load");
        if self.fail_load {
            anyhow::bail!("test actor on_load failure");
        }
        self.state_load = Some(state);
        Ok(())
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        self.control.record("actor.on_start");
        if self.fail_start {
            anyhow::bail!("test actor on_start failure");
        }
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.control.record("actor.on_stop");
        Ok(())
    }

    fn on_save(&self) -> anyhow::Result<IndexMap<String, Vec<u8>>> {
        self.control.record("actor.on_save");
        if self.fail_save {
            anyhow::bail!("test actor on_save failure");
        }
        Ok(self.state_save.clone())
    }
}

nautilus_actor!(StateActor);

/// Strategy with observable state lifecycle callbacks.
#[derive(Debug)]
pub struct StateStrategy {
    core: StrategyCore,
    control: TestCacheDatabaseControl,
    state_load: Option<IndexMap<String, Vec<u8>>>,
    state_save: IndexMap<String, Vec<u8>>,
    fail_load: bool,
    fail_save: bool,
    fail_start: bool,
}

impl StateStrategy {
    /// Creates a stateful strategy.
    #[must_use]
    pub fn new(
        strategy_id: StrategyId,
        control: TestCacheDatabaseControl,
        state_save: IndexMap<String, Vec<u8>>,
    ) -> Self {
        Self {
            core: StrategyCore::new(StrategyConfig {
                strategy_id: Some(strategy_id),
                ..Default::default()
            }),
            control,
            state_load: None,
            state_save,
            fail_load: false,
            fail_save: false,
            fail_start: false,
        }
    }

    /// Configures the strategy load callback to fail.
    #[must_use]
    pub const fn with_fail_load(mut self) -> Self {
        self.fail_load = true;
        self
    }

    /// Configures the strategy save callback to fail.
    #[must_use]
    pub const fn with_fail_save(mut self) -> Self {
        self.fail_save = true;
        self
    }

    /// Configures the strategy start callback to fail.
    #[must_use]
    pub const fn with_fail_start(mut self) -> Self {
        self.fail_start = true;
        self
    }

    /// Returns the state received by the load callback.
    #[must_use]
    pub const fn state_load(&self) -> Option<&IndexMap<String, Vec<u8>>> {
        self.state_load.as_ref()
    }
}

impl DataActor for StateStrategy {
    fn on_load(&mut self, state: IndexMap<String, Vec<u8>>) -> anyhow::Result<()> {
        self.control.record("strategy.on_load");
        if self.fail_load {
            anyhow::bail!("test strategy on_load failure");
        }
        self.state_load = Some(state);
        Ok(())
    }

    fn on_start(&mut self) -> anyhow::Result<()> {
        self.control.record("strategy.on_start");
        if self.fail_start {
            anyhow::bail!("test strategy on_start failure");
        }
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.control.record("strategy.on_stop");
        Ok(())
    }

    fn on_save(&self) -> anyhow::Result<IndexMap<String, Vec<u8>>> {
        self.control.record("strategy.on_save");
        if self.fail_save {
            anyhow::bail!("test strategy on_save failure");
        }
        Ok(self.state_save.clone())
    }
}

nautilus_strategy!(StateStrategy);
