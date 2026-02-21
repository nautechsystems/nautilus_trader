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

from pathlib import Path

import pytest

from nautilus_trader.adapters.betfair.data_types import BetfairRaceProgress
from nautilus_trader.adapters.betfair.data_types import BetfairRaceRunnerData
from nautilus_trader.adapters.betfair.parsing.rcm import BetfairRCMParser
from nautilus_trader.adapters.betfair.parsing.rcm import is_rcm_message
from nautilus_trader.adapters.betfair.parsing.rcm import rcm_decode
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import DataType


RESOURCES_PATH = Path(__file__).parent / "resources"


@pytest.fixture
def rcm_raw() -> bytes:
    return (RESOURCES_PATH / "streaming" / "streaming_rcm.json").read_bytes()


@pytest.fixture
def rcm_multi_runner_raw() -> bytes:
    return (RESOURCES_PATH / "streaming" / "streaming_rcm_multi_runner.json").read_bytes()


@pytest.fixture
def rcm_race_start_raw() -> bytes:
    return (RESOURCES_PATH / "streaming" / "streaming_rcm_race_start.json").read_bytes()


@pytest.fixture
def rcm_sequence_path() -> Path:
    return RESOURCES_PATH / "streaming" / "streaming_rcm_sequence.jsonl"


@pytest.fixture
def rcm_parser() -> BetfairRCMParser:
    return BetfairRCMParser()


def test_is_rcm_message_true(rcm_raw: bytes):
    assert is_rcm_message(rcm_raw) is True


def test_is_rcm_message_false_for_mcm():
    mcm_raw = b'{"op":"mcm","id":1,"clk":"123","pt":1234567890}'
    assert is_rcm_message(mcm_raw) is False


def test_rcm_decode_basic(rcm_raw: bytes):
    rcm = rcm_decode(rcm_raw)

    assert rcm.op == "rcm"
    assert rcm.id == 2
    assert rcm.clk == 12
    assert rcm.pt == 1518626764


def test_rcm_decode_race_changes(rcm_raw: bytes):
    rcm = rcm_decode(rcm_raw)

    assert rcm.rc is not None
    assert len(rcm.rc) == 1
    rc = rcm.rc[0]
    assert rc.id == "28587288.1650"
    assert rc.mid == "1.1234567"


def test_rcm_decode_race_runner_changes(rcm_raw: bytes):
    rcm = rcm_decode(rcm_raw)
    assert rcm.rc is not None
    rc = rcm.rc[0]
    assert rc.rrc is not None

    assert len(rc.rrc) == 1
    rrc = rc.rrc[0]
    assert rrc.ft == 1518626674
    assert rrc.id == 7390417
    assert rrc.lat == 51.4189543
    assert rrc.long_ == -0.4058491
    assert rrc.spd == 17.8
    assert rrc.prg == 2051
    assert rrc.sfq == 2.07


def test_rcm_decode_race_progress_changes(rcm_raw: bytes):
    rcm = rcm_decode(rcm_raw)
    assert rcm.rc is not None
    rc = rcm.rc[0]
    assert rc.rpc is not None

    rpc = rc.rpc
    assert rpc.ft == 1518626674
    assert rpc.g == "1f"
    assert rpc.st == 10.6
    assert rpc.rt == 46.7
    assert rpc.spd == 17.8
    assert rpc.prg == 87.5
    assert rpc.ord == [7390417, 5600338, 11527189, 6395118, 8706072]


def test_rcm_decode_jumps(rcm_raw: bytes):
    rcm = rcm_decode(rcm_raw)
    assert rcm.rc is not None
    rpc = rcm.rc[0].rpc
    assert rpc is not None
    assert rpc.J is not None

    assert len(rpc.J) == 2
    assert rpc.J[0].J == 2
    assert rpc.J[0].L == 370.1
    assert rpc.J[1].J == 1
    assert rpc.J[1].L == 203.8


def test_rcm_decode_multi_runner(rcm_multi_runner_raw: bytes):
    rcm = rcm_decode(rcm_multi_runner_raw)

    assert rcm.op == "rcm"
    assert rcm.rc is not None
    assert len(rcm.rc) == 1
    rc = rcm.rc[0]
    assert rc.id == "32908802.0000"
    assert rc.mid == "1.223101854"
    assert rc.rrc is not None
    assert len(rc.rrc) == 5


def test_rcm_decode_runner_gps_coordinates(rcm_multi_runner_raw: bytes):
    rcm = rcm_decode(rcm_multi_runner_raw)
    assert rcm.rc is not None
    rc = rcm.rc[0]
    assert rc.rrc is not None

    runner1 = rc.rrc[0]
    assert runner1.id == 35467839
    assert runner1.lat == 37.8837153
    assert runner1.long_ == -122.3093228
    assert runner1.spd == 16.33
    assert runner1.prg == 1076.5
    assert runner1.sfq == 2.5
    for rrc in rc.rrc:
        assert rrc.lat is not None
        assert rrc.long_ is not None
        assert rrc.spd is not None
        assert rrc.prg is not None


def test_rcm_decode_race_start_with_jumps(rcm_race_start_raw: bytes):
    rcm = rcm_decode(rcm_race_start_raw)
    assert rcm.rc is not None

    assert rcm.op == "rcm"
    rc = rcm.rc[0]
    assert rc.rpc is not None
    rpc = rc.rpc
    assert rpc.g == "S1M7f195y"
    assert rpc.J is not None
    assert len(rpc.J) == 9


def test_rcm_decode_jump_distances(rcm_race_start_raw: bytes):
    rcm = rcm_decode(rcm_race_start_raw)
    assert rcm.rc is not None
    rpc = rcm.rc[0].rpc
    assert rpc is not None
    assert rpc.J is not None

    assert rpc.J[0].J == 9
    assert rpc.J[0].L == 3123.5
    assert rpc.J[8].J == 1
    assert rpc.J[8].L == 196.2


def test_rcm_decode_race_order(rcm_race_start_raw: bytes):
    rcm = rcm_decode(rcm_race_start_raw)
    assert rcm.rc is not None
    rpc = rcm.rc[0].rpc
    assert rpc is not None

    assert rpc.ord == [53127827, 49011080]
    assert rpc.rt == 1.74
    assert rpc.spd == 11.1
    assert rpc.prg == 3180.3


def test_rcm_sequence_parsing(rcm_sequence_path: Path):
    with open(rcm_sequence_path, "rb") as f:
        messages = [rcm_decode(line) for line in f if line.strip()]

    assert len(messages) == 4

    # First message: 5 runners
    rc0 = messages[0].rc
    assert rc0 is not None
    assert rc0[0].rrc is not None
    assert len(rc0[0].rrc) == 5
    assert rc0[0].rpc is None

    # Second message: 2 more runners
    rc1 = messages[1].rc
    assert rc1 is not None
    assert rc1[0].rrc is not None
    assert len(rc1[0].rrc) == 2

    # Third message: 1 more runner
    rc2 = messages[2].rc
    assert rc2 is not None
    assert rc2[0].rrc is not None
    assert len(rc2[0].rrc) == 1

    # Fourth message: race progress only
    rc3 = messages[3].rc
    assert rc3 is not None
    assert rc3[0].rrc is None
    assert rc3[0].rpc is not None


def test_rcm_sequence_total_runners(rcm_sequence_path: Path):
    all_runner_ids: set[int] = set()
    with open(rcm_sequence_path, "rb") as f:
        for line in f:
            if not line.strip():
                continue
            rcm = rcm_decode(line)
            assert rcm.rc is not None
            if rcm.rc[0].rrc:
                for rrc in rcm.rc[0].rrc:
                    if rrc.id is not None:
                        all_runner_ids.add(rrc.id)

    assert len(all_runner_ids) == 8


def test_rcm_sequence_final_order(rcm_sequence_path: Path):
    with open(rcm_sequence_path, "rb") as f:
        messages = [rcm_decode(line) for line in f if line.strip()]

    assert messages[3].rc is not None
    rpc = messages[3].rc[0].rpc
    assert rpc is not None
    assert rpc.ord == [24947967, 40695865, 41694785, 299569, 40562776, 31422647, 35467839, 41436946]


def test_parse_returns_custom_data(rcm_raw: bytes, rcm_parser: BetfairRCMParser):
    rcm = rcm_decode(rcm_raw)
    updates = rcm_parser.parse(rcm)

    assert len(updates) == 2
    assert all(isinstance(u, CustomData) for u in updates)


def test_parse_race_runner_data(rcm_raw: bytes, rcm_parser: BetfairRCMParser):
    rcm = rcm_decode(rcm_raw)
    updates = rcm_parser.parse(rcm)

    runner_update = updates[0]
    assert isinstance(runner_update, CustomData)
    assert runner_update.data_type.type == BetfairRaceRunnerData
    runner_data = runner_update.data
    assert runner_data.race_id == "28587288.1650"
    assert runner_data.market_id == "1.1234567"
    assert runner_data.selection_id == 7390417
    assert runner_data.latitude == 51.4189543
    assert runner_data.longitude == -0.4058491
    assert runner_data.speed == 17.8
    assert runner_data.progress == 2051
    assert runner_data.stride_frequency == 2.07


def test_parse_race_progress(rcm_raw: bytes, rcm_parser: BetfairRCMParser):
    rcm = rcm_decode(rcm_raw)
    updates = rcm_parser.parse(rcm)

    progress_update = updates[1]
    assert isinstance(progress_update, CustomData)
    assert progress_update.data_type.type == BetfairRaceProgress
    progress = progress_update.data
    assert progress.race_id == "28587288.1650"
    assert progress.market_id == "1.1234567"
    assert progress.gate_name == "1f"
    assert progress.sectional_time == 10.6
    assert progress.running_time == 46.7
    assert progress.speed == 17.8
    assert progress.progress == 87.5
    assert progress.order == [7390417, 5600338, 11527189, 6395118, 8706072]
    assert progress.jumps == [{"J": 2, "L": 370.1}, {"J": 1, "L": 203.8}]


def test_parse_timestamps(rcm_raw: bytes, rcm_parser: BetfairRCMParser):
    rcm = rcm_decode(rcm_raw)
    updates = rcm_parser.parse(rcm)

    expected_ts_event = 1518626674 * 1_000_000
    expected_ts_init = 1518626764 * 1_000_000

    runner_data = updates[0].data
    assert runner_data.ts_event == expected_ts_event
    assert runner_data.ts_init == expected_ts_init

    progress = updates[1].data
    assert progress.ts_event == expected_ts_event
    assert progress.ts_init == expected_ts_init


def test_runner_data_to_dict_from_dict_round_trip():
    runner_data = BetfairRaceRunnerData(
        race_id="28587288.1650",
        market_id="1.1234567",
        selection_id=7390417,
        latitude=51.4189543,
        longitude=-0.4058491,
        speed=17.8,
        progress=2051.0,
        stride_frequency=2.07,
        ts_event=1518626764000000,
        ts_init=1518626764000000,
    )

    d = BetfairRaceRunnerData.to_dict(runner_data)
    restored = BetfairRaceRunnerData.from_dict(d)

    assert restored.race_id == runner_data.race_id
    assert restored.market_id == runner_data.market_id
    assert restored.selection_id == runner_data.selection_id
    assert restored.latitude == runner_data.latitude
    assert restored.longitude == runner_data.longitude
    assert restored.speed == runner_data.speed
    assert restored.progress == runner_data.progress
    assert restored.stride_frequency == runner_data.stride_frequency
    assert restored.ts_event == runner_data.ts_event
    assert restored.ts_init == runner_data.ts_init


def test_runner_data_repr():
    runner_data = BetfairRaceRunnerData(
        race_id="28587288.1650",
        market_id="1.1234567",
        selection_id=7390417,
        latitude=51.4189543,
        longitude=-0.4058491,
        speed=17.8,
        progress=2051.0,
        stride_frequency=2.07,
        ts_event=1518626764000000,
        ts_init=1518626764000000,
    )

    r = repr(runner_data)
    assert "BetfairRaceRunnerData" in r
    assert "28587288.1650" in r
    assert "7390417" in r


def test_race_progress_to_dict_from_dict_round_trip():
    progress = BetfairRaceProgress(
        race_id="28587288.1650",
        market_id="1.1234567",
        gate_name="1f",
        sectional_time=10.6,
        running_time=46.7,
        speed=17.8,
        progress=87.5,
        order=[7390417, 5600338],
        jumps=[{"J": 2, "L": 370.1}, {"J": 1, "L": 203.8}],
        ts_event=1518626764000000,
        ts_init=1518626764000000,
    )

    d = BetfairRaceProgress.to_dict(progress)
    restored = BetfairRaceProgress.from_dict(d)

    assert restored.race_id == progress.race_id
    assert restored.market_id == progress.market_id
    assert restored.gate_name == progress.gate_name
    assert restored.sectional_time == progress.sectional_time
    assert restored.running_time == progress.running_time
    assert restored.speed == progress.speed
    assert restored.progress == progress.progress
    assert restored.order == progress.order
    assert restored.jumps == progress.jumps
    assert restored.ts_event == progress.ts_event
    assert restored.ts_init == progress.ts_init


def test_race_progress_to_dict_from_dict_with_json_jumps():
    progress = BetfairRaceProgress(
        race_id="28587288.1650",
        market_id="1.1234567",
        gate_name="1f",
        sectional_time=10.6,
        running_time=46.7,
        speed=17.8,
        progress=87.5,
        order=[7390417],
        jumps=[{"J": 2, "L": 370.1}],
        ts_event=1518626764000000,
        ts_init=1518626764000000,
    )

    d = BetfairRaceProgress.to_dict(progress)
    assert isinstance(d["jumps"], str)

    restored = BetfairRaceProgress.from_dict(d)
    assert restored.jumps == [{"J": 2, "L": 370.1}]


def test_race_progress_to_dict_from_dict_with_empty_jumps():
    """
    Test that empty jumps list is preserved (not converted to None).
    """
    progress = BetfairRaceProgress(
        race_id="28587288.1650",
        market_id="1.1234567",
        gate_name="1f",
        sectional_time=10.6,
        running_time=46.7,
        speed=17.8,
        progress=87.5,
        order=[7390417],
        jumps=[],
        ts_event=1518626764000000,
        ts_init=1518626764000000,
    )

    d = BetfairRaceProgress.to_dict(progress)
    assert d["jumps"] == "[]"

    restored = BetfairRaceProgress.from_dict(d)
    assert restored.jumps == []


def test_race_progress_repr():
    progress = BetfairRaceProgress(
        race_id="28587288.1650",
        market_id="1.1234567",
        gate_name="1f",
        sectional_time=10.6,
        running_time=46.7,
        speed=17.8,
        progress=87.5,
        order=[7390417],
        jumps=None,
        ts_event=1518626764000000,
        ts_init=1518626764000000,
    )

    r = repr(progress)
    assert "BetfairRaceProgress" in r
    assert "28587288.1650" in r
    assert "1f" in r


def test_runner_data_type_metadata_excludes_race_id(
    rcm_raw: bytes,
    rcm_parser: BetfairRCMParser,
):
    """
    Published runner DataType metadata should only contain selection_id (not race_id) so
    subscribers can match without knowing the race_id.
    """
    # Arrange, Act
    rcm = rcm_decode(rcm_raw)
    updates = rcm_parser.parse(rcm)

    # Assert
    runner_update = updates[0]
    assert runner_update.data_type.metadata == {"selection_id": 7390417}
    assert "race_id" not in runner_update.data_type.metadata


def test_progress_data_type_has_race_id_metadata(
    rcm_raw: bytes,
    rcm_parser: BetfairRCMParser,
):
    """
    Published progress DataType should include race_id metadata so subscribers can
    filter to a specific race.
    """
    # Arrange, Act
    rcm = rcm_decode(rcm_raw)
    updates = rcm_parser.parse(rcm)

    # Assert
    progress_update = updates[1]
    assert progress_update.data_type.metadata == {"race_id": "28587288.1650"}


def test_runner_subscription_topic_matches_published_topic(
    rcm_raw: bytes,
    rcm_parser: BetfairRCMParser,
):
    """
    An actor subscribing with DataType(BetfairRaceRunnerData, {"selection_id": N}) must
    produce the same topic string as the published data.
    """
    # Arrange
    rcm = rcm_decode(rcm_raw)
    updates = rcm_parser.parse(rcm)
    published_data_type = updates[0].data_type

    # Act - simulate actor subscription
    subscription_data_type = DataType(
        BetfairRaceRunnerData,
        metadata={"selection_id": 7390417},
    )

    # Assert
    assert subscription_data_type.topic == published_data_type.topic


def test_progress_subscription_topic_matches_published_topic(
    rcm_raw: bytes,
    rcm_parser: BetfairRCMParser,
):
    """
    An actor subscribing with DataType(BetfairRaceProgress, {"race_id": ...}) must
    produce a topic that matches the published data's topic.
    """
    # Arrange
    rcm = rcm_decode(rcm_raw)
    updates = rcm_parser.parse(rcm)
    published_data_type = updates[1].data_type

    # Act
    subscription_data_type = DataType(BetfairRaceProgress, {"race_id": "28587288.1650"})

    # Assert
    assert subscription_data_type.topic == published_data_type.topic
