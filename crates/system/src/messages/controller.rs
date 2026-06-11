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

use std::any::Any;

use nautilus_common::actor::data_actor::ImportableActorConfig;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::identifiers::{ActorId, StrategyId};
use nautilus_trading::ImportableStrategyConfig;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub struct CreateActor {
    pub actor_config: ImportableActorConfig,
    pub start: bool,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
}

impl CreateActor {
    /// Creates a new [`CreateActor`] instance.
    #[must_use]
    pub const fn new(
        actor_config: ImportableActorConfig,
        start: bool,
        command_id: UUID4,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            actor_config,
            start,
            command_id,
            ts_init,
        }
    }

    #[must_use]
    pub fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub struct CreateStrategy {
    pub strategy_config: ImportableStrategyConfig,
    pub start: bool,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
}

impl CreateStrategy {
    /// Creates a new [`CreateStrategy`] instance.
    #[must_use]
    pub const fn new(
        strategy_config: ImportableStrategyConfig,
        start: bool,
        command_id: UUID4,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            strategy_config,
            start,
            command_id,
            ts_init,
        }
    }

    #[must_use]
    pub fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
pub struct StartActor {
    pub actor_id: ActorId,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
}

impl StartActor {
    /// Creates a new [`StartActor`] instance.
    #[must_use]
    pub const fn new(actor_id: ActorId, command_id: UUID4, ts_init: UnixNanos) -> Self {
        Self {
            actor_id,
            command_id,
            ts_init,
        }
    }

    #[must_use]
    pub fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
pub struct StartStrategy {
    pub strategy_id: StrategyId,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
}

impl StartStrategy {
    /// Creates a new [`StartStrategy`] instance.
    #[must_use]
    pub const fn new(strategy_id: StrategyId, command_id: UUID4, ts_init: UnixNanos) -> Self {
        Self {
            strategy_id,
            command_id,
            ts_init,
        }
    }

    #[must_use]
    pub fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
pub struct StopActor {
    pub actor_id: ActorId,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
}

impl StopActor {
    /// Creates a new [`StopActor`] instance.
    #[must_use]
    pub const fn new(actor_id: ActorId, command_id: UUID4, ts_init: UnixNanos) -> Self {
        Self {
            actor_id,
            command_id,
            ts_init,
        }
    }

    #[must_use]
    pub fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
pub struct StopStrategy {
    pub strategy_id: StrategyId,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
}

impl StopStrategy {
    /// Creates a new [`StopStrategy`] instance.
    #[must_use]
    pub const fn new(strategy_id: StrategyId, command_id: UUID4, ts_init: UnixNanos) -> Self {
        Self {
            strategy_id,
            command_id,
            ts_init,
        }
    }

    #[must_use]
    pub fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
pub struct RemoveActor {
    pub actor_id: ActorId,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
}

impl RemoveActor {
    /// Creates a new [`RemoveActor`] instance.
    #[must_use]
    pub const fn new(actor_id: ActorId, command_id: UUID4, ts_init: UnixNanos) -> Self {
        Self {
            actor_id,
            command_id,
            ts_init,
        }
    }

    #[must_use]
    pub fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
pub struct RemoveStrategy {
    pub strategy_id: StrategyId,
    pub command_id: UUID4,
    pub ts_init: UnixNanos,
}

impl RemoveStrategy {
    /// Creates a new [`RemoveStrategy`] instance.
    #[must_use]
    pub const fn new(strategy_id: StrategyId, command_id: UUID4, ts_init: UnixNanos) -> Self {
        Self {
            strategy_id,
            command_id,
            ts_init,
        }
    }

    #[must_use]
    pub fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Commands handled by the [`Controller`](crate::controller::Controller).
#[derive(Debug, Clone)]
pub enum ControllerCommand {
    CreateActor(CreateActor),
    StartActor(StartActor),
    StopActor(StopActor),
    RemoveActor(RemoveActor),
    CreateStrategy(CreateStrategy),
    StartStrategy(StartStrategy),
    StopStrategy(StopStrategy),
    ExitMarket(StrategyId),
    RemoveStrategy(RemoveStrategy),
}

impl From<CreateActor> for ControllerCommand {
    fn from(command: CreateActor) -> Self {
        Self::CreateActor(command)
    }
}

impl From<CreateStrategy> for ControllerCommand {
    fn from(command: CreateStrategy) -> Self {
        Self::CreateStrategy(command)
    }
}

impl From<StartActor> for ControllerCommand {
    fn from(command: StartActor) -> Self {
        Self::StartActor(command)
    }
}

impl From<StartStrategy> for ControllerCommand {
    fn from(command: StartStrategy) -> Self {
        Self::StartStrategy(command)
    }
}

impl From<StopActor> for ControllerCommand {
    fn from(command: StopActor) -> Self {
        Self::StopActor(command)
    }
}

impl From<StopStrategy> for ControllerCommand {
    fn from(command: StopStrategy) -> Self {
        Self::StopStrategy(command)
    }
}

impl From<RemoveActor> for ControllerCommand {
    fn from(command: RemoveActor) -> Self {
        Self::RemoveActor(command)
    }
}

impl From<RemoveStrategy> for ControllerCommand {
    fn from(command: RemoveStrategy) -> Self {
        Self::RemoveStrategy(command)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_create_actor_command_fields() {
        let actor_config = ImportableActorConfig {
            actor_path: "tests.actors:Actor".to_string(),
            config_path: "tests.actors:ActorConfig".to_string(),
            config: HashMap::new(),
        };
        let command_id = UUID4::new();
        let command = CreateActor::new(actor_config.clone(), false, command_id, UnixNanos::new(1));

        assert_eq!(command.actor_config.actor_path, actor_config.actor_path);
        assert_eq!(command.actor_config.config_path, actor_config.config_path);
        assert!(!command.start);
        assert_eq!(command.command_id, command_id);
        assert_eq!(command.ts_init, UnixNanos::new(1));
        assert!(matches!(
            ControllerCommand::from(command),
            ControllerCommand::CreateActor(_)
        ));
    }

    #[rstest]
    fn test_create_actor_command_serde_round_trip() {
        let actor_config = ImportableActorConfig {
            actor_path: "tests.actors:Actor".to_string(),
            config_path: "tests.actors:ActorConfig".to_string(),
            config: HashMap::from([(
                "threshold".to_string(),
                serde_json::Value::String("10".to_string()),
            )]),
        };
        let command_id = UUID4::new();
        let command = CreateActor::new(actor_config, true, command_id, UnixNanos::new(9));

        let value = serde_json::to_value(&command).unwrap();
        assert_eq!(value["type"], "CreateActor");
        let round_trip: CreateActor = serde_json::from_value(value).unwrap();

        assert_eq!(round_trip.actor_config.actor_path, "tests.actors:Actor");
        assert_eq!(
            round_trip.actor_config.config_path,
            "tests.actors:ActorConfig"
        );
        assert_eq!(
            round_trip.actor_config.config["threshold"],
            serde_json::Value::String("10".to_string())
        );
        assert!(round_trip.start);
        assert_eq!(round_trip.command_id, command_id);
        assert_eq!(round_trip.ts_init, UnixNanos::new(9));
    }

    #[rstest]
    fn test_create_strategy_command_fields() {
        let strategy_config = ImportableStrategyConfig {
            strategy_path: "tests.strategies:Strategy".to_string(),
            config_path: "tests.strategies:StrategyConfig".to_string(),
            config: HashMap::new(),
        };
        let command_id = UUID4::new();
        let command =
            CreateStrategy::new(strategy_config.clone(), true, command_id, UnixNanos::new(2));

        assert_eq!(
            command.strategy_config.strategy_path,
            strategy_config.strategy_path
        );
        assert_eq!(
            command.strategy_config.config_path,
            strategy_config.config_path
        );
        assert!(command.start);
        assert_eq!(command.command_id, command_id);
        assert_eq!(command.ts_init, UnixNanos::new(2));
        assert!(matches!(
            ControllerCommand::from(command),
            ControllerCommand::CreateStrategy(_)
        ));
    }

    #[rstest]
    fn test_create_strategy_command_serde_round_trip() {
        let strategy_config = ImportableStrategyConfig {
            strategy_path: "tests.strategies:Strategy".to_string(),
            config_path: "tests.strategies:StrategyConfig".to_string(),
            config: HashMap::from([(
                "threshold".to_string(),
                serde_json::Value::String("20".to_string()),
            )]),
        };
        let command_id = UUID4::new();
        let command = CreateStrategy::new(strategy_config, false, command_id, UnixNanos::new(11));

        let value = serde_json::to_value(&command).unwrap();
        assert_eq!(value["type"], "CreateStrategy");
        let round_trip: CreateStrategy = serde_json::from_value(value).unwrap();

        assert_eq!(
            round_trip.strategy_config.strategy_path,
            "tests.strategies:Strategy"
        );
        assert_eq!(
            round_trip.strategy_config.config_path,
            "tests.strategies:StrategyConfig"
        );
        assert_eq!(
            round_trip.strategy_config.config["threshold"],
            serde_json::Value::String("20".to_string())
        );
        assert!(!round_trip.start);
        assert_eq!(round_trip.command_id, command_id);
        assert_eq!(round_trip.ts_init, UnixNanos::new(11));
    }

    #[rstest]
    fn test_start_actor_command_fields() {
        let command_id = UUID4::new();
        let command = StartActor::new(ActorId::from("Actor-001"), command_id, UnixNanos::new(3));

        assert_eq!(command.actor_id, ActorId::from("Actor-001"));
        assert_eq!(command.command_id, command_id);
        assert_eq!(command.ts_init, UnixNanos::new(3));
        assert!(matches!(
            ControllerCommand::from(command),
            ControllerCommand::StartActor(_)
        ));
    }

    #[rstest]
    fn test_start_actor_command_serde_round_trip() {
        let actor_id = ActorId::from("Actor-001");
        let command_id = UUID4::new();
        let command = StartActor::new(actor_id, command_id, UnixNanos::new(10));

        let value = serde_json::to_value(command).unwrap();
        assert_eq!(value["type"], "StartActor");
        let round_trip: StartActor = serde_json::from_value(value).unwrap();

        assert_eq!(round_trip.actor_id, actor_id);
        assert_eq!(round_trip.command_id, command_id);
        assert_eq!(round_trip.ts_init, UnixNanos::new(10));
    }

    #[rstest]
    fn test_stop_actor_command_fields() {
        let command_id = UUID4::new();
        let command = StopActor::new(ActorId::from("Actor-001"), command_id, UnixNanos::new(4));

        assert_eq!(command.actor_id, ActorId::from("Actor-001"));
        assert_eq!(command.command_id, command_id);
        assert_eq!(command.ts_init, UnixNanos::new(4));
        assert!(matches!(
            ControllerCommand::from(command),
            ControllerCommand::StopActor(_)
        ));
    }

    #[rstest]
    fn test_remove_actor_command_fields() {
        let command_id = UUID4::new();
        let command = RemoveActor::new(ActorId::from("Actor-001"), command_id, UnixNanos::new(5));

        assert_eq!(command.actor_id, ActorId::from("Actor-001"));
        assert_eq!(command.command_id, command_id);
        assert_eq!(command.ts_init, UnixNanos::new(5));
        assert!(matches!(
            ControllerCommand::from(command),
            ControllerCommand::RemoveActor(_)
        ));
    }

    #[rstest]
    fn test_start_strategy_command_fields() {
        let command_id = UUID4::new();
        let command = StartStrategy::new(
            StrategyId::from("Strategy-001"),
            command_id,
            UnixNanos::new(6),
        );

        assert_eq!(command.strategy_id, StrategyId::from("Strategy-001"));
        assert_eq!(command.command_id, command_id);
        assert_eq!(command.ts_init, UnixNanos::new(6));
        assert!(matches!(
            ControllerCommand::from(command),
            ControllerCommand::StartStrategy(_)
        ));
    }

    #[rstest]
    fn test_start_strategy_command_serde_round_trip() {
        let strategy_id = StrategyId::from("Strategy-001");
        let command_id = UUID4::new();
        let command = StartStrategy::new(strategy_id, command_id, UnixNanos::new(12));

        let value = serde_json::to_value(command).unwrap();
        assert_eq!(value["type"], "StartStrategy");
        let round_trip: StartStrategy = serde_json::from_value(value).unwrap();

        assert_eq!(round_trip.strategy_id, strategy_id);
        assert_eq!(round_trip.command_id, command_id);
        assert_eq!(round_trip.ts_init, UnixNanos::new(12));
    }

    #[rstest]
    fn test_stop_strategy_command_fields() {
        let command_id = UUID4::new();
        let command = StopStrategy::new(
            StrategyId::from("Strategy-001"),
            command_id,
            UnixNanos::new(7),
        );

        assert_eq!(command.strategy_id, StrategyId::from("Strategy-001"));
        assert_eq!(command.command_id, command_id);
        assert_eq!(command.ts_init, UnixNanos::new(7));
        assert!(matches!(
            ControllerCommand::from(command),
            ControllerCommand::StopStrategy(_)
        ));
    }

    #[rstest]
    fn test_remove_strategy_command_fields() {
        let command_id = UUID4::new();
        let command = RemoveStrategy::new(
            StrategyId::from("Strategy-001"),
            command_id,
            UnixNanos::new(8),
        );

        assert_eq!(command.strategy_id, StrategyId::from("Strategy-001"));
        assert_eq!(command.command_id, command_id);
        assert_eq!(command.ts_init, UnixNanos::new(8));
        assert!(matches!(
            ControllerCommand::from(command),
            ControllerCommand::RemoveStrategy(_)
        ));
    }
}
