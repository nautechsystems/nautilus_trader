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
"""
Betfair Race Change Message (RCM) parsing for Total Performance Data (TPD).

TPD provides live GPS tracking data for horse racing including:
- Individual horse position, speed, and stride frequency
- Race progress with sectional times and running order
- Jump obstacle locations for National Hunt races

"""

from __future__ import annotations

import msgspec

from nautilus_trader.adapters.betfair.data_types import BetfairRaceProgress
from nautilus_trader.adapters.betfair.data_types import BetfairRaceRunnerData
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import DataType


class Jump(msgspec.Struct, frozen=True):
    """
    Jump obstacle data for National Hunt races.
    """

    J: int  # Jump number
    L: float  # Distance to jump from finish line in meters


class RaceRunnerChange(msgspec.Struct, frozen=True):
    """
    Individual runner GPS data (rrc).

    Fields:
    - ft: feed_time (timestamp ms)
    - id: selection_id (Betfair selection ID)
    - lat: latitude (GPS)
    - long: longitude (GPS) - note: Python reserved word, accessed via getattr
    - spd: speed in m/s (Doppler-derived)
    - prg: progress (distance to finish in meters)
    - sfq: stride_frequency in Hz

    """

    ft: int | None = None
    id: int | None = None
    lat: float | None = None
    spd: float | None = None
    prg: float | None = None
    sfq: float | None = None

    # 'long' is a Python reserved word, handled via rename
    long_: float | None = msgspec.field(default=None, name="long")


class RaceProgressChange(msgspec.Struct, frozen=True):
    """
    Overall race progress data (rpc).

    Fields:
    - ft: feed_time (timestamp ms)
    - g: gate_name (e.g., "1f", "2f", "Finish")
    - st: sectional_time (time for the section in seconds)
    - rt: running_time (time since race start in seconds)
    - spd: speed of lead horse in m/s
    - prg: progress to finish for leading horse in meters
    - ord: order (list of selection IDs in current race position)
    - J: jumps (list of obstacle locations for jump races)

    """

    ft: int | None = None
    g: str | None = None
    st: float | None = None
    rt: float | None = None
    spd: float | None = None
    prg: float | None = None
    ord: list[int] | None = None
    J: list[Jump] | None = None


class RaceChange(msgspec.Struct, frozen=True):
    """
    Race change data container (rc).

    Fields:
    - id: Race ID (e.g., "28587288.1650")
    - mid: Market ID (Betfair market ID)
    - rrc: Race runner changes (individual horse data)
    - rpc: Race progress change (overall race summary)

    """

    id: str | None = None
    mid: str | None = None
    rrc: list[RaceRunnerChange] | None = None
    rpc: RaceProgressChange | None = None


class RCM(msgspec.Struct, frozen=True):
    """
    Race Change Message (RCM) - top level message.

    Fields:
    - op: Operation type ("rcm")
    - id: Request ID (optional, only present if sent with subscription)
    - clk: Clock token
    - pt: Publish time (ms since epoch)
    - rc: Race changes
    """

    op: str
    clk: int | str
    pt: int
    rc: list[RaceChange] | None = None
    id: int | None = None


_RCM_DECODER = msgspec.json.Decoder(RCM)


def is_rcm_message(raw: bytes) -> bool:
    """
    Check if raw bytes represent an RCM message.

    Quick check without full parsing.

    """
    return b'"op":"rcm"' in raw


def rcm_decode(raw: bytes) -> RCM:
    """
    Decode RCM message bytes to RCM struct.
    """
    return _RCM_DECODER.decode(raw)


RCM_PARSE_TYPES = BetfairRaceRunnerData | BetfairRaceProgress


def race_change_to_updates(
    rc: RaceChange,
    ts_init: int,
    ts_event_fallback: int,
) -> list[CustomData]:
    """
    Convert a RaceChange to Nautilus CustomData updates.
    """
    updates: list[CustomData] = []

    # Skip if race_id is missing (required for data types)
    if rc.id is None:
        return updates

    # Use each runner's feed time (ft) for ts_event, fall back to message publish time.
    if rc.rrc:
        for rrc in rc.rrc:
            # Skip runners without selection_id (required field)
            if rrc.id is None:
                continue

            ts_event = ts_event_fallback if rrc.ft is None else millis_to_nanos(rrc.ft)
            runner_data = BetfairRaceRunnerData(
                race_id=rc.id,
                market_id=rc.mid,
                selection_id=rrc.id,
                latitude=rrc.lat,
                longitude=rrc.long_,
                speed=rrc.spd,
                progress=rrc.prg,
                stride_frequency=rrc.sfq,
                ts_event=ts_event,
                ts_init=ts_init,
            )
            updates.append(
                CustomData(
                    DataType(BetfairRaceRunnerData, {"selection_id": rrc.id}),
                    runner_data,
                ),
            )

    if rc.rpc:
        ts_event = ts_event_fallback if rc.rpc.ft is None else millis_to_nanos(rc.rpc.ft)
        jumps = [{"J": j.J, "L": j.L} for j in rc.rpc.J] if rc.rpc.J is not None else None

        progress = BetfairRaceProgress(
            race_id=rc.id,
            market_id=rc.mid,
            gate_name=rc.rpc.g,
            sectional_time=rc.rpc.st,
            running_time=rc.rpc.rt,
            speed=rc.rpc.spd,
            progress=rc.rpc.prg,
            order=rc.rpc.ord,
            jumps=jumps,
            ts_event=ts_event,
            ts_init=ts_init,
        )
        updates.append(
            CustomData(
                DataType(BetfairRaceProgress, {"race_id": rc.id}),
                progress,
            ),
        )

    return updates


class BetfairRCMParser:
    """
    Stateful parser for RCM messages.
    """

    def __init__(self) -> None:
        pass

    def parse(
        self,
        rcm: RCM,
        ts_init: int | None = None,
    ) -> list[CustomData]:
        """
        Parse an RCM message into Nautilus CustomData types.

        Parameters
        ----------
        rcm : RCM
            The RCM message to parse.
        ts_init : int, optional
            The initialization timestamp in nanoseconds. Defaults to ts_event.

        Returns
        -------
        list[CustomData]

        """
        updates: list[CustomData] = []

        # Use publish time for ts_init (when message was received)
        ts_event_fallback = millis_to_nanos(rcm.pt)
        ts_init = ts_init or ts_event_fallback

        if rcm.rc:
            for rc in rcm.rc:
                updates.extend(race_change_to_updates(rc, ts_init, ts_event_fallback))

        return updates
