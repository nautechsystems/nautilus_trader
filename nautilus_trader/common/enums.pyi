# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

class ComponentState:
    PreInitialized: int = 0
    Ready: int = 1
    Starting: int = 2
    Running: int = 3
    Stopping: int = 4
    Stopped: int = 5
    Resuming: int = 6
    Resetting: int = 7
    Disposing: int = 8
    Disposed: int = 9
    Degrading: int = 10
    Degraded: int = 11
    Faulting: int = 12
    Faulted: int = 13

class ComponentTrigger:
    Initialize: int = 1
    Start: int = 2
    StartCompleted: int = 3
    Stop: int = 4
    StopCompleted: int = 5
    Resume: int = 6
    ResumeCompleted: int = 7
    Reset: int = 8
    ResetCompleted: int = 9
    Dispose: int = 10
    DisposeCompleted: int = 11
    Degrade: int = 12
    DegradeCompleted: int = 13
    Fault: int = 14
    FaultCompleted: int = 15

class LogLevel:
    Off: int = 0
    Trace: int = 1
    Debug: int = 2
    Info: int = 3
    Warning: int = 4
    Error: int = 5

class LogColor:
    Normal: int = 0
    Green: int = 1
    Blue: int = 2
    Magenta: int = 3
    Cyan: int = 4
    Yellow: int = 5
    Red: int = 6

class LogFormat:
    Header: int = 1
    Endc: int = 2
    Bold: int = 3
    Underline: int = 4

class SerializationEncoding:
    MsgPack: int = 0
    Json: int = 1
