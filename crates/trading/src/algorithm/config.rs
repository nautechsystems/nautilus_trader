//! Configuration for execution algorithms.

use nautilus_core::serialization::default_true;
use nautilus_model::identifiers::ExecAlgorithmId;
use serde::{Deserialize, Serialize};

/// Configuration for an execution algorithm.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.trading", from_py_object)
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
