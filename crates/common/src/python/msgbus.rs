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

use pyo3::pymethods;

use crate::{
    enums::SerializationEncoding,
    msgbus::{
        BusMessage,
        database::{DatabaseConfig, MessageBusConfig},
    },
};

#[pymethods]
impl BusMessage {
    #[getter]
    #[pyo3(name = "topic")]
    fn py_topic(&mut self) -> String {
        self.topic.to_string()
    }

    #[getter]
    #[pyo3(name = "payload")]
    fn py_payload(&mut self) -> &[u8] {
        self.payload.as_ref()
    }

    fn __repr__(&self) -> String {
        format!("{}('{}')", stringify!(BusMessage), self)
    }

    fn __str__(&self) -> String {
        self.to_string()
    }
}

#[pymethods]
impl DatabaseConfig {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (database_type=None, host=None, port=None, username=None, password=None, ssl=None, connection_timeout=None, response_timeout=None, number_of_retries=None, exponent_base=None, max_delay=None, factor=None))]
    fn py_new(
        database_type: Option<String>,
        host: Option<String>,
        port: Option<u16>,
        username: Option<String>,
        password: Option<String>,
        ssl: Option<bool>,
        connection_timeout: Option<u16>,
        response_timeout: Option<u16>,
        number_of_retries: Option<usize>,
        exponent_base: Option<u64>,
        max_delay: Option<u64>,
        factor: Option<u64>,
    ) -> Self {
        let default = Self::default();
        Self {
            database_type: database_type.unwrap_or(default.database_type),
            host,
            port,
            username,
            password,
            ssl: ssl.unwrap_or(default.ssl),
            connection_timeout: connection_timeout.unwrap_or(default.connection_timeout),
            response_timeout: response_timeout.unwrap_or(default.response_timeout),
            number_of_retries: number_of_retries.unwrap_or(default.number_of_retries),
            exponent_base: exponent_base.unwrap_or(default.exponent_base),
            max_delay: max_delay.unwrap_or(default.max_delay),
            factor: factor.unwrap_or(default.factor),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn database_type(&self) -> &str {
        &self.database_type
    }

    #[getter]
    fn host(&self) -> Option<&str> {
        self.host.as_deref()
    }

    #[getter]
    fn port(&self) -> Option<u16> {
        self.port
    }

    #[getter]
    fn username(&self) -> Option<&str> {
        self.username.as_deref()
    }

    #[getter]
    fn password(&self) -> Option<&str> {
        self.password.as_deref()
    }

    #[getter]
    fn ssl(&self) -> bool {
        self.ssl
    }

    #[getter]
    fn connection_timeout(&self) -> u16 {
        self.connection_timeout
    }

    #[getter]
    fn response_timeout(&self) -> u16 {
        self.response_timeout
    }

    #[getter]
    fn number_of_retries(&self) -> usize {
        self.number_of_retries
    }

    #[getter]
    fn exponent_base(&self) -> u64 {
        self.exponent_base
    }

    #[getter]
    fn max_delay(&self) -> u64 {
        self.max_delay
    }

    #[getter]
    fn factor(&self) -> u64 {
        self.factor
    }
}

#[pymethods]
impl MessageBusConfig {
    #[new]
    #[allow(clippy::too_many_arguments)]
    #[pyo3(signature = (database=None, encoding=None, timestamps_as_iso8601=None, buffer_interval_ms=None, autotrim_mins=None, use_trader_prefix=None, use_trader_id=None, use_instance_id=None, streams_prefix=None, stream_per_topic=None, external_streams=None, types_filter=None, heartbeat_interval_secs=None))]
    fn py_new(
        database: Option<DatabaseConfig>,
        encoding: Option<SerializationEncoding>,
        timestamps_as_iso8601: Option<bool>,
        buffer_interval_ms: Option<u32>,
        autotrim_mins: Option<u32>,
        use_trader_prefix: Option<bool>,
        use_trader_id: Option<bool>,
        use_instance_id: Option<bool>,
        streams_prefix: Option<String>,
        stream_per_topic: Option<bool>,
        external_streams: Option<Vec<String>>,
        types_filter: Option<Vec<String>>,
        heartbeat_interval_secs: Option<u16>,
    ) -> Self {
        let default = Self::default();
        Self {
            database,
            encoding: encoding.unwrap_or(default.encoding),
            timestamps_as_iso8601: timestamps_as_iso8601.unwrap_or(default.timestamps_as_iso8601),
            buffer_interval_ms,
            autotrim_mins,
            use_trader_prefix: use_trader_prefix.unwrap_or(default.use_trader_prefix),
            use_trader_id: use_trader_id.unwrap_or(default.use_trader_id),
            use_instance_id: use_instance_id.unwrap_or(default.use_instance_id),
            streams_prefix: streams_prefix.unwrap_or(default.streams_prefix),
            stream_per_topic: stream_per_topic.unwrap_or(default.stream_per_topic),
            external_streams,
            types_filter,
            heartbeat_interval_secs,
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn database(&self) -> Option<DatabaseConfig> {
        self.database.clone()
    }

    #[getter]
    fn encoding(&self) -> SerializationEncoding {
        self.encoding
    }

    #[getter]
    fn timestamps_as_iso8601(&self) -> bool {
        self.timestamps_as_iso8601
    }

    #[getter]
    fn buffer_interval_ms(&self) -> Option<u32> {
        self.buffer_interval_ms
    }

    #[getter]
    fn autotrim_mins(&self) -> Option<u32> {
        self.autotrim_mins
    }

    #[getter]
    fn use_trader_prefix(&self) -> bool {
        self.use_trader_prefix
    }

    #[getter]
    fn use_trader_id(&self) -> bool {
        self.use_trader_id
    }

    #[getter]
    fn use_instance_id(&self) -> bool {
        self.use_instance_id
    }

    #[getter]
    fn streams_prefix(&self) -> &str {
        &self.streams_prefix
    }

    #[getter]
    fn stream_per_topic(&self) -> bool {
        self.stream_per_topic
    }

    #[getter]
    fn external_streams(&self) -> Option<Vec<String>> {
        self.external_streams.clone()
    }

    #[getter]
    fn types_filter(&self) -> Option<Vec<String>> {
        self.types_filter.clone()
    }

    #[getter]
    fn heartbeat_interval_secs(&self) -> Option<u16> {
        self.heartbeat_interval_secs
    }
}
