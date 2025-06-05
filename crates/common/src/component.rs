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

use std::{cell::RefCell, rc::Rc};

use nautilus_model::identifiers::{ComponentId, TraderId};

use crate::{
    actor::Actor,
    cache::Cache,
    clock::Clock,
    enums::{ComponentState, ComponentTrigger},
    timer::TimeEvent,
};

/// Components are actors with lifecycle management capabilities.
pub trait Component: Actor {
    /// Returns the unique identifier for this component.
    fn component_id(&self) -> ComponentId;

    /// Returns the current state of the component.
    fn state(&self) -> ComponentState;

    /// Returns whether the component is ready.
    fn is_ready(&self) -> bool {
        self.state() == ComponentState::Ready
    }

    /// Returns whether the component is *not* running.
    fn not_running(&self) -> bool {
        !self.is_running()
    }

    /// Returns whether the component is running.
    fn is_running(&self) -> bool {
        self.state() == ComponentState::Running
    }

    /// Returns whether the component is stopped.
    fn is_stopped(&self) -> bool {
        self.state() == ComponentState::Stopped
    }

    /// Returns whether the component has been degraded.
    fn is_degraded(&self) -> bool {
        self.state() == ComponentState::Degraded
    }

    /// Returns whether the component has been faulted.
    fn is_faulted(&self) -> bool {
        self.state() == ComponentState::Faulted
    }

    /// Returns whether the component has been disposed.
    fn is_disposed(&self) -> bool {
        self.state() == ComponentState::Disposed
    }

    /// Registers the component with a system.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to register.
    fn register(
        &mut self,
        trader_id: TraderId,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> anyhow::Result<()>;

    /// Starts the component.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to start.
    fn start(&mut self) -> anyhow::Result<()>;

    /// Stops the component.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to stop.
    fn stop(&mut self) -> anyhow::Result<()>;

    /// Resets the component to its initial state.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to reset.
    fn reset(&mut self) -> anyhow::Result<()>;

    /// Disposes of the component, releasing any resources.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to dispose.
    fn dispose(&mut self) -> anyhow::Result<()>;

    /// Handles a time event (TBD).
    fn handle_event(&mut self, event: TimeEvent);
}

#[rustfmt::skip]
impl ComponentState {
    /// Transition the state machine with the component `trigger`.
    ///
    /// # Errors
    ///
    /// Returns an error if `trigger` is invalid for the current state.
    pub fn transition(&mut self, trigger: &ComponentTrigger) -> anyhow::Result<Self> {
        let new_state = match (&self, trigger) {
            (Self::PreInitialized, ComponentTrigger::Initialize) => Self::Ready,
            (Self::Ready, ComponentTrigger::Reset) => Self::Resetting,
            (Self::Ready, ComponentTrigger::Start) => Self::Starting,
            (Self::Ready, ComponentTrigger::Dispose) => Self::Disposing,
            (Self::Resetting, ComponentTrigger::ResetCompleted) => Self::Ready,
            (Self::Starting, ComponentTrigger::StartCompleted) => Self::Running,
            (Self::Starting, ComponentTrigger::Stop) => Self::Stopping,
            (Self::Starting, ComponentTrigger::Fault) => Self::Faulting,
            (Self::Running, ComponentTrigger::Stop) => Self::Stopping,
            (Self::Running, ComponentTrigger::Degrade) => Self::Degrading,
            (Self::Running, ComponentTrigger::Fault) => Self::Faulting,
            (Self::Resuming, ComponentTrigger::Stop) => Self::Stopping,
            (Self::Resuming, ComponentTrigger::ResumeCompleted) => Self::Running,
            (Self::Resuming, ComponentTrigger::Fault) => Self::Faulting,
            (Self::Stopping, ComponentTrigger::StopCompleted) => Self::Stopped,
            (Self::Stopping, ComponentTrigger::Fault) => Self::Faulting,
            (Self::Stopped, ComponentTrigger::Reset) => Self::Resetting,
            (Self::Stopped, ComponentTrigger::Resume) => Self::Resuming,
            (Self::Stopped, ComponentTrigger::Dispose) => Self::Disposing,
            (Self::Stopped, ComponentTrigger::Fault) => Self::Faulting,
            (Self::Degrading, ComponentTrigger::DegradeCompleted) => Self::Degraded,
            (Self::Degraded, ComponentTrigger::Resume) => Self::Resuming,
            (Self::Degraded, ComponentTrigger::Stop) => Self::Stopping,
            (Self::Degraded, ComponentTrigger::Fault) => Self::Faulting,
            (Self::Disposing, ComponentTrigger::DisposeCompleted) => Self::Disposed,
            (Self::Faulting, ComponentTrigger::FaultCompleted) => Self::Faulted,
            _ => anyhow::bail!("Invalid state trigger {self} -> {trigger}"),
        };
        Ok(new_state)
    }
}
