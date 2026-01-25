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

//! Configuration for execution algorithms.

use nautilus_core::serialization::default_true;
use nautilus_model::identifiers::ExecAlgorithmId;
use serde::{Deserialize, Serialize};

/// Configuration for an execution algorithm.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.trading")
)]
pub struct ExecutionAlgorithmConfig {
    /// The unique ID for the execution algorithm.
    pub exec_algorithm_id: Option<ExecAlgorithmId>,
    /// If events should be logged by the algorithm.
    #[serde(default = "default_true")]
    pub log_events: bool,
    /// If commands should be logged by the algorithm.
    #[serde(default = "default_true")]
    pub log_commands: bool,
}

impl Default for ExecutionAlgorithmConfig {
    fn default() -> Self {
        Self {
            exec_algorithm_id: None,
            log_events: true,
            log_commands: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_config_default() {
        let config = ExecutionAlgorithmConfig::default();

        assert!(config.exec_algorithm_id.is_none());
        assert!(config.log_events);
        assert!(config.log_commands);
    }

    #[rstest]
    fn test_config_with_id() {
        let exec_algorithm_id = ExecAlgorithmId::new("TWAP");
        let config = ExecutionAlgorithmConfig {
            exec_algorithm_id: Some(exec_algorithm_id),
            ..Default::default()
        };

        assert_eq!(config.exec_algorithm_id, Some(exec_algorithm_id));
    }

    #[rstest]
    fn test_config_serialization() {
        let config = ExecutionAlgorithmConfig {
            exec_algorithm_id: Some(ExecAlgorithmId::new("TWAP")),
            log_events: false,
            log_commands: true,
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ExecutionAlgorithmConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.exec_algorithm_id, deserialized.exec_algorithm_id);
        assert_eq!(config.log_events, deserialized.log_events);
        assert_eq!(config.log_commands, deserialized.log_commands);
    }
}
