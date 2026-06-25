# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import pytest

from nautilus_trader.common import CacheConfig
from nautilus_trader.common import DataActorConfig
from nautilus_trader.common import FileWriterConfig
from nautilus_trader.common import ImportableActorConfig
from nautilus_trader.common import LoggerConfig
from nautilus_trader.common import MessageBusConfig
from nautilus_trader.model import ActorId


def test_cache_config_defaults():
    config = CacheConfig(
        None,
        False,
        None,
        None,
        True,
        False,
        False,
        True,
        10000,
        10000,
        True,
        True,
    )

    assert str(config.encoding) == "SerializationEncoding.JSON"
    assert config.timestamps_as_iso8601 is False
    assert config.buffer_interval_ms is None
    assert config.bulk_read_batch_size is None
    assert config.use_trader_prefix is True
    assert config.use_instance_id is False
    assert config.flush_on_start is False
    assert config.drop_instruments_on_reset is True
    assert config.tick_capacity == 10000
    assert config.bar_capacity == 10000
    assert config.save_market_data is True
    assert config.persist_account_events is True


def test_cache_config_accepts_explicit_values():
    # Get SerializationEncoding.JSON via the enum type
    default = CacheConfig(
        None,
        False,
        None,
        None,
        True,
        False,
        False,
        True,
        10000,
        10000,
        True,
        True,
    )
    json_encoding = type(default.encoding).JSON

    config = CacheConfig(
        json_encoding,
        True,
        100,
        500,
        False,
        True,
        True,
        False,
        5000,
        2000,
        False,
        False,
    )

    assert config.encoding == json_encoding
    assert config.timestamps_as_iso8601 is True
    assert config.buffer_interval_ms == 100
    assert config.bulk_read_batch_size == 500
    assert config.use_trader_prefix is False
    assert config.use_instance_id is True
    assert config.flush_on_start is True
    assert config.drop_instruments_on_reset is False
    assert config.tick_capacity == 5000
    assert config.bar_capacity == 2000
    assert config.save_market_data is False
    assert config.persist_account_events is False


def test_cache_config_rejects_public_string_encoding_argument():
    with pytest.raises(TypeError, match="SerializationEncoding"):
        CacheConfig("msgpack", False, True, True, False, False, False, 1000, 1000, 100, 1000, True)


def test_cache_config_rejects_embedded_database_config():
    with pytest.raises(TypeError, match="database"):
        CacheConfig(database=None)


def test_data_actor_config_accepts_explicit_kwargs():
    config = DataActorConfig(
        actor_id=ActorId("ACTOR-001"),
        log_events=False,
        log_commands=True,
    )

    assert isinstance(config, DataActorConfig)


def test_file_writer_config_construction(tmp_path):
    config = FileWriterConfig(
        directory=str(tmp_path),
        file_name="common.log",
        file_format="json",
        file_rotate=(1, 2),
    )

    assert type(config).__name__ == "FileWriterConfig"


def test_importable_actor_config_fields():
    config = ImportableActorConfig(
        actor_path="tests.unit.common.actor:TestActor",
        config_path="tests.unit.common.actor:TestActorConfig",
        config={"log_events": False},
    )

    assert config.actor_path == "tests.unit.common.actor:TestActor"
    assert config.config_path == "tests.unit.common.actor:TestActorConfig"
    assert config.config == {"log_events": False}


def test_logger_config_from_spec():
    config = LoggerConfig.from_spec("stdout=INFO;file=DEBUG")

    assert type(config).__name__ == "LoggerConfig"


def test_message_bus_config_defaults():
    config = MessageBusConfig()

    assert config.timestamps_as_iso8601 is False
    assert config.buffer_interval_ms is None
    assert config.autotrim_mins is None
    assert config.use_trader_prefix is True
    assert config.use_trader_id is True
    assert config.use_instance_id is False
    assert config.stream_per_topic is True
    assert config.streams_prefix == "stream"
    assert config.external_streams is None
    assert config.types_filter is None
    assert config.heartbeat_interval_secs is None


def test_message_bus_config_accepts_explicit_kwargs():
    config = MessageBusConfig(
        timestamps_as_iso8601=True,
        buffer_interval_ms=7,
        autotrim_mins=8,
        use_trader_prefix=False,
        use_trader_id=False,
        use_instance_id=True,
        streams_prefix="streams",
        stream_per_topic=False,
        external_streams=["orders", "fills"],
        types_filter=["Signal", "CustomData"],
        heartbeat_interval_secs=9,
    )

    assert config.timestamps_as_iso8601 is True
    assert config.buffer_interval_ms == 7
    assert config.autotrim_mins == 8
    assert config.use_trader_prefix is False
    assert config.use_trader_id is False
    assert config.use_instance_id is True
    assert config.streams_prefix == "streams"
    assert config.stream_per_topic is False
    assert config.external_streams == ["orders", "fills"]
    assert config.types_filter == ["Signal", "CustomData"]
    assert config.heartbeat_interval_secs == 9


def test_message_bus_config_rejects_embedded_backing_config():
    with pytest.raises(TypeError, match="backing"):
        MessageBusConfig(backing=None)
