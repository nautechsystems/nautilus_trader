#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from enum import IntEnum

from nautilus_trader.core.fsm import FiniteStateMachine
from nautilus_trader.core.fsm import InvalidStateTrigger


if __name__ == "__main__":
    # Step 1: Define states
    class AppState(IntEnum):  # Only IntEnum is supported for Nautilus State Machine
        """
        Represents the possible states of our application.
        """

        INITIALIZED = 0
        READY = 1
        ACTIVE = 2
        PAUSED = 3
        STOPPED = 4

    # Step 2: Define triggers (actions)
    class AppTrigger(IntEnum):  # Only IntEnum is supported for Nautilus State Machine
        """
        Represents the possible triggers that can cause transitions between states.
        """

        START = 0
        PAUSE = 1
        RESUME = 2
        STOP = 3

    # Step 3: Define transitions
    # Each line contains: (OLD_STATE, TRIGGER) -> NEW STATE
    STATE_TRANSITIONS: dict[tuple[AppState, AppTrigger], AppState] = {
        (AppState.READY, AppTrigger.START): AppState.ACTIVE,
        (AppState.ACTIVE, AppTrigger.PAUSE): AppState.PAUSED,
        (AppState.ACTIVE, AppTrigger.STOP): AppState.STOPPED,
        (AppState.PAUSED, AppTrigger.RESUME): AppState.ACTIVE,
        (AppState.PAUSED, AppTrigger.STOP): AppState.STOPPED,
    }

    # ---------------------------------------
    # Example how to create State Machine
    # ---------------------------------------

    fsm = FiniteStateMachine(
        state_transition_table=STATE_TRANSITIONS,
        initial_state=AppState.READY,
        # Next 2 parameters refer to function(s), that convert enum number -> string
        state_parser=lambda code: {enum.value: enum.name for enum in AppState}[code],
        trigger_parser=lambda code: {enum.value: enum.name for enum in AppTrigger}[code],
    )

    # ---------------------------------------
    # Using State Machine simply means:
    #   - checking current state
    #   - invoking triggers (actions)
    # ---------------------------------------

    print(f"Current (initial) state: {fsm.state_string}")

    print(f"Invoking trigger: {AppTrigger.START}")
    fsm.trigger(AppTrigger.START)
    print(f"Current state: {fsm.state_string}")

    print(f"Invoking trigger: {AppTrigger.PAUSE}")
    fsm.trigger(AppTrigger.PAUSE)
    print(f"Current state: {fsm.state_string}")

    print(f"Invoking trigger: {AppTrigger.RESUME}")
    fsm.trigger(AppTrigger.RESUME)
    print(f"Current state: {fsm.state_string}")

    print(f"Invoking trigger: {AppTrigger.STOP}")
    fsm.trigger(AppTrigger.STOP)
    print(f"Current state: {fsm.state_string}")

    # Let's try invalid action: We cannot RESUME, after state-machine was STOPPED.
    try:
        print(f"Invoking trigger: {AppTrigger.RESUME}")
        fsm.trigger(AppTrigger.RESUME)
    except InvalidStateTrigger:
        # We expect this exception be thrown, as we intentionally invoke invalid trigger/action.
        print("We got expected exception: InvalidStateTrigger")
