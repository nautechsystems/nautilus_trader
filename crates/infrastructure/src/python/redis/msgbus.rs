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
use nautilus_common::{
    enums::SerializationEncoding,
    msgbus::{BusMessage, BusPayloadType, MessageBusBacking, MessageBusConfig},
    python::config_error_to_pyvalue_err,
};
use nautilus_core::{
    UUID4,
    python::{IntoPyObjectNautilusExt, call_python, to_pyruntime_err, to_pyvalue_err},
};
use nautilus_model::identifiers::TraderId;
use pyo3::{prelude::*, pybacked::PyBackedBytes};
use serde_json::Value;
use ustr::Ustr;

use crate::redis::msgbus::{RedisMessageBusBacking, RedisMessageBusConfig};

#[derive(Debug)]
#[pyclass(
    name = "RedisMessageBusBacking",
    module = "nautilus_trader.core.nautilus_pyo3.infrastructure"
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.infrastructure")]
pub struct PyRedisMessageBusBacking {
    inner: RedisMessageBusBacking,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyRedisMessageBusBacking {
    #[new]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "PyBackedBytes is required for generated Python bytes stubs"
    )]
    fn py_new(
        trader_id: TraderId,
        instance_id: UUID4,
        config_json: PyBackedBytes,
    ) -> PyResult<Self> {
        let (config, backing) = parse_config(config_json.as_ref())?;
        let inner = RedisMessageBusBacking::new(trader_id, instance_id, config, backing)
            .map_err(to_pyvalue_err)?;
        Ok(Self { inner })
    }

    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        MessageBusBacking::is_closed(&self.inner)
    }

    #[pyo3(name = "publish")]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "PyBackedBytes is required for generated Python bytes stubs"
    )]
    fn py_publish(&self, topic: &str, payload: PyBackedBytes) {
        let message = BusMessage::new(
            Ustr::from(topic),
            BusPayloadType::Custom(Ustr::default()),
            Bytes::copy_from_slice(payload.as_ref()),
            SerializationEncoding::default(),
        );
        MessageBusBacking::publish(&self.inner, message);
    }

    #[pyo3(name = "stream")]
    fn py_stream<'py>(
        &mut self,
        callback: Py<PyAny>,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let stream_rx = self.inner.get_stream_receiver().map_err(to_pyruntime_err)?;
        let stream = RedisMessageBusBacking::stream(stream_rx);
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
        MessageBusBacking::close(&mut self.inner);
    }
}

fn parse_config(config_json: &[u8]) -> PyResult<(MessageBusConfig, RedisMessageBusConfig)> {
    let mut value: Value = serde_json::from_slice(config_json).map_err(to_pyvalue_err)?;
    let backing = parse_backing_config(&mut value)?;
    let config = serde_json::from_value::<MessageBusConfig>(value).map_err(to_pyvalue_err)?;
    config.validate().map_err(config_error_to_pyvalue_err)?;

    Ok((config, backing))
}

fn parse_backing_config(value: &mut Value) -> PyResult<RedisMessageBusConfig> {
    let Value::Object(config) = value else {
        return Err(to_pyvalue_err("MessageBusConfig must be a JSON object"));
    };

    let Some(database) = config.remove("database") else {
        return Ok(RedisMessageBusConfig::default());
    };

    let mut database = match database {
        Value::Null => return Ok(RedisMessageBusConfig::default()),
        Value::Object(database) => database,
        _ => {
            return Err(to_pyvalue_err(
                "MessageBusConfig.database must be a JSON object",
            ));
        }
    };

    if let Some(database_type) = database.remove("type") {
        match database_type {
            Value::String(database_type) if database_type == "redis" => {}
            Value::String(database_type) => {
                return Err(to_pyvalue_err(format!(
                    "MessageBusConfig.database.type must be 'redis', was '{database_type}'"
                )));
            }
            other => {
                return Err(to_pyvalue_err(format!(
                    "MessageBusConfig.database.type must be a string, was {other}"
                )));
            }
        }
    }

    serde_json::from_value(Value::Object(database)).map_err(to_pyvalue_err)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_parse_config_splits_legacy_database_config() {
        let config_json = json!({
            "database": {
                "type": "redis",
                "host": "localhost",
                "port": 6380,
                "ssl": true,
            },
            "buffer_interval_ms": 100,
            "streams_prefix": "signals",
            "stream_per_topic": false,
            "external_streams": ["signals"],
        });

        let (config, backing) = parse_config(config_json.to_string().as_bytes()).unwrap();

        assert_eq!(config.buffer_interval_ms, Some(100));
        assert_eq!(config.streams_prefix, "signals");
        assert!(!config.stream_per_topic);
        assert_eq!(config.external_streams, Some(vec!["signals".to_string()]));
        assert_eq!(backing.host, Some("localhost".to_string()));
        assert_eq!(backing.port, Some(6380));
        assert!(backing.ssl);
    }

    #[rstest]
    fn test_parse_config_accepts_python_message_bus_config_json() {
        let config_json = json!({
            "database": {
                "type": "redis",
                "host": "redis.example.com",
                "port": 6380,
                "username": "user",
                "password": "secret",
                "ssl": true,
                "connection_timeout": 30,
                "response_timeout": 10,
                "number_of_retries": 3,
                "exponent_base": 3,
                "max_delay": 15,
                "factor": 4,
            },
            "encoding": "msgpack",
            "timestamps_as_iso8601": true,
            "buffer_interval_ms": null,
            "autotrim_mins": null,
            "use_trader_prefix": true,
            "use_trader_id": false,
            "use_instance_id": true,
            "streams_prefix": "stream",
            "stream_per_topic": false,
            "external_streams": ["signals"],
            "types_filter": ["nautilus_trader.model.data:QuoteTick"],
            "heartbeat_interval_secs": null,
        });

        let (config, backing) = parse_config(config_json.to_string().as_bytes()).unwrap();

        assert_eq!(config.encoding, SerializationEncoding::MsgPack);
        assert!(config.timestamps_as_iso8601);
        assert_eq!(config.buffer_interval_ms, None);
        assert_eq!(config.autotrim_mins, None);
        assert!(config.use_trader_prefix);
        assert!(!config.use_trader_id);
        assert!(config.use_instance_id);
        assert_eq!(config.streams_prefix, "stream");
        assert!(!config.stream_per_topic);
        assert_eq!(config.external_streams, Some(vec!["signals".to_string()]));
        assert_eq!(
            config.types_filter,
            Some(vec!["nautilus_trader.model.data:QuoteTick".to_string()])
        );
        assert_eq!(config.heartbeat_interval_secs, None);
        assert_eq!(backing.host, Some("redis.example.com".to_string()));
        assert_eq!(backing.port, Some(6380));
        assert_eq!(backing.username, Some("user".to_string()));
        assert_eq!(backing.password, Some("secret".to_string()));
        assert!(backing.ssl);
        assert_eq!(backing.connection_timeout, 30);
        assert_eq!(backing.response_timeout, 10);
        assert_eq!(backing.number_of_retries, 3);
        assert_eq!(backing.exponent_base, 3);
        assert_eq!(backing.max_delay, 15);
        assert_eq!(backing.factor, 4);
    }

    #[rstest]
    fn test_parse_config_rejects_non_redis_database_type() {
        Python::initialize();
        let config_json = json!({
            "database": {
                "type": "postgres",
            },
        });

        let result = parse_config(config_json.to_string().as_bytes());

        assert_eq!(
            result.unwrap_err().to_string(),
            "ValueError: MessageBusConfig.database.type must be 'redis', was 'postgres'"
        );
    }
}
