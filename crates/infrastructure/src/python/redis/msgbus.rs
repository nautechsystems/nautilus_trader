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

use bytes::Bytes;
use futures::{pin_mut, stream::StreamExt};
use nautilus_common::msgbus::backing::{MessageBusBacking, MessageBusConfig};
use nautilus_core::{
    UUID4,
    python::{IntoPyObjectNautilusExt, call_python, to_pyruntime_err, to_pyvalue_err},
};
use nautilus_model::identifiers::TraderId;
use pyo3::prelude::*;
use serde_json::Value;
use ustr::Ustr;

use crate::redis::msgbus::{RedisMessageBusBacking, RedisMessageBusConfig};

#[pymethods]
impl RedisMessageBusBacking {
    /// Creates a new `RedisMessageBusBacking` instance for the given `trader_id`, `instance_id`, and `config`.
    #[new]
    #[pyo3(signature = (trader_id, instance_id, config_json, backing_config_json=None))]
    fn py_new(
        trader_id: TraderId,
        instance_id: UUID4,
        config_json: &[u8],
        backing_config_json: Option<&[u8]>,
    ) -> PyResult<Self> {
        let (config, backing) = parse_inputs(config_json, backing_config_json)?;
        Self::new(trader_id, instance_id, config, backing).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    #[pyo3(name = "publish")]
    fn py_publish(&self, topic: &str, payload: Vec<u8>) {
        self.publish(Ustr::from(topic), Bytes::from(payload));
    }

    /// Streams messages arriving on the stream receiver channel.
    #[pyo3(name = "stream")]
    fn py_stream<'py>(
        &mut self,
        callback: Py<PyAny>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let stream_rx = self.get_stream_receiver().map_err(to_pyruntime_err)?;
        let stream = Self::stream(stream_rx);
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            pin_mut!(stream);
            while let Some(msg) = stream.next().await {
                Python::attach(|py| call_python(py, &callback, msg.into_py_any_unwrap(py)));
            }
            Ok(())
        })
    }

    #[pyo3(name = "close")]
    fn py_close(&mut self) {
        self.close();
    }
}

fn parse_inputs(
    config_json: &[u8],
    backing_config_json: Option<&[u8]>,
) -> PyResult<(MessageBusConfig, RedisMessageBusConfig)> {
    let mut config_value: Value = serde_json::from_slice(config_json).map_err(to_pyvalue_err)?;
    // TODO: Remove the legacy embedded backing path once Python v2 callers use backing_config_json.
    let legacy_backing = config_value.as_object_mut().and_then(|object| {
        object
            .remove("database")
            .or_else(|| object.remove("backing"))
    });

    let config = serde_json::from_value(config_value).map_err(to_pyvalue_err)?;
    let backing = match backing_config_json {
        Some(raw) => serde_json::from_slice(raw).map_err(to_pyvalue_err)?,
        None => match legacy_backing {
            Some(value) => config_from_legacy_backing(value)?,
            None => RedisMessageBusConfig::default(),
        },
    };

    Ok((config, backing))
}

fn config_from_legacy_backing(mut value: Value) -> PyResult<RedisMessageBusConfig> {
    if value.is_null() {
        return Ok(RedisMessageBusConfig::default());
    }

    remove_legacy_selector(&mut value, "message bus backing")?;
    serde_json::from_value(value).map_err(to_pyvalue_err)
}

fn remove_legacy_selector(value: &mut Value, label: &str) -> PyResult<()> {
    let Some(object) = value.as_object_mut() else {
        return Ok(());
    };

    let selector = object
        .remove("backing_type")
        .or_else(|| object.remove("database_type"))
        .or_else(|| object.remove("type"));
    let Some(selector) = selector else {
        return Ok(());
    };
    let Some(selector) = selector.as_str() else {
        return Err(to_pyvalue_err(format!(
            "invalid {label} type selector, expected string"
        )));
    };

    if selector != "redis" {
        return Err(to_pyvalue_err(format!(
            "invalid {label} type selector, expected 'redis', was '{selector}'"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[rstest]
    #[case("database")]
    #[case("backing")]
    fn test_parse_inputs_accepts_legacy_backing_field(#[case] field_name: &str) {
        let config_json = serde_json::to_vec(&json!({
            field_name: {
                "type": "redis",
                "host": "redis.example.com",
                "port": 6380,
                "password": "secret",
                "ssl": true,
            },
            "encoding": "json",
            "streams_prefix": "events",
        }))
        .unwrap();

        let (config, backing) = parse_inputs(&config_json, None).unwrap();

        assert_eq!(config.streams_prefix, "events");
        assert_eq!(backing.host, Some("redis.example.com".to_string()));
        assert_eq!(backing.port, Some(6380));
        assert_eq!(backing.password, Some("secret".to_string()));
        assert!(backing.ssl);
    }

    #[rstest]
    fn test_parse_inputs_defaults_null_legacy_backing() {
        let config_json = serde_json::to_vec(&json!({
            "database": null,
            "streams_prefix": "events",
        }))
        .unwrap();

        let (config, backing) = parse_inputs(&config_json, None).unwrap();

        assert_eq!(config.streams_prefix, "events");
        assert_eq!(backing, RedisMessageBusConfig::default());
    }

    #[rstest]
    fn test_parse_inputs_prefers_explicit_backing_config() {
        let config_json = serde_json::to_vec(&json!({
            "database": {
                "type": "redis",
                "host": "legacy.example.com",
            },
        }))
        .unwrap();
        let backing_config_json = serde_json::to_vec(&json!({
            "host": "explicit.example.com",
            "port": 6381,
        }))
        .unwrap();

        let (_, backing) = parse_inputs(&config_json, Some(&backing_config_json)).unwrap();

        assert_eq!(backing.host, Some("explicit.example.com".to_string()));
        assert_eq!(backing.port, Some(6381));
    }

    #[rstest]
    fn test_parse_inputs_rejects_non_redis_legacy_backing() {
        Python::initialize();
        let config_json = serde_json::to_vec(&json!({
            "database": {
                "type": "postgres",
            },
        }))
        .unwrap();

        let error = parse_inputs(&config_json, None).unwrap_err();

        assert_eq!(
            error.to_string(),
            "ValueError: invalid message bus backing type selector, expected 'redis', was 'postgres'"
        );
    }
}
