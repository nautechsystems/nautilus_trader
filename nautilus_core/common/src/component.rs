// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use crate::enums::ComponentState;

pub struct PreInitialized;
pub struct Ready;
pub struct Starting;
pub struct Running;
pub struct Stopping;
pub struct Stopped;
pub struct Resuming;
pub struct Degrading;
pub struct Degraded;
pub struct Faulting;
pub struct Faulted;
pub struct Disposed;

pub trait State {
    fn state() -> ComponentState;
}

impl State for PreInitialized {
    fn state() -> ComponentState {
        ComponentState::PreInitialized
    }
}

impl State for Ready {
    fn state() -> ComponentState {
        ComponentState::Ready
    }
}

impl State for Starting {
    fn state() -> ComponentState {
        ComponentState::Starting
    }
}

impl State for Running {
    fn state() -> ComponentState {
        ComponentState::Running
    }
}

impl State for Stopping {
    fn state() -> ComponentState {
        ComponentState::Stopping
    }
}

impl State for Stopped {
    fn state() -> ComponentState {
        ComponentState::Stopped
    }
}

impl State for Resuming {
    fn state() -> ComponentState {
        ComponentState::Resuming
    }
}

impl State for Degrading {
    fn state() -> ComponentState {
        ComponentState::Degrading
    }
}

impl State for Degraded {
    fn state() -> ComponentState {
        ComponentState::Degraded
    }
}

impl State for Faulting {
    fn state() -> ComponentState {
        ComponentState::Faulting
    }
}

impl State for Faulted {
    fn state() -> ComponentState {
        ComponentState::Faulted
    }
}

impl State for Disposed {
    fn state() -> ComponentState {
        ComponentState::Disposed
    }
}
