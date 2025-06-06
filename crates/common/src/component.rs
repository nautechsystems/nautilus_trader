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
};

/// Components are actors with lifecycle management capabilities.
pub trait Component: Actor {
    /// Returns the unique identifier for this component.
    fn component_id(&self) -> ComponentId;

    /// Returns the current state of the component.
    fn state(&self) -> ComponentState;

    /// Transition the component with the state trigger.
    ///
    /// # Errors
    ///
    /// Returns an error if the `trigger` is an invalid transition from the current state.
    fn transition_state(&mut self, trigger: ComponentTrigger) -> anyhow::Result<()>;

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

    /// Initializes the component.
    ///
    /// # Errors
    ///
    /// Returns an error if the initialization state transition fails.
    fn initialize(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Initialize)
    }

    /// Starts the component.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to start.
    fn start(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Start)?; // -> Starting

        if let Err(e) = self.on_start() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::StartCompleted)?;

        Ok(())
    }

    /// Stops the component.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to stop.
    fn stop(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Stop)?; // -> Stopping

        if let Err(e) = self.on_stop() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::StopCompleted)?;

        Ok(())
    }

    /// Resumes the component.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to resume.
    fn resume(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Resume)?; // -> Resuming

        if let Err(e) = self.on_resume() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::ResumeCompleted)?;

        Ok(())
    }

    /// Degrades the component.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to degrade.
    fn degrade(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Degrade)?; // -> Degrading

        if let Err(e) = self.on_degrade() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::DegradeCompleted)?;

        Ok(())
    }

    /// Faults the component.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to fault.
    fn fault(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Fault)?; // -> Faulting

        if let Err(e) = self.on_fault() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::FaultCompleted)?;

        Ok(())
    }

    /// Resets the component to its initial state.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to reset.
    fn reset(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Reset)?; // -> Resetting

        if let Err(e) = self.on_reset() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::ResetCompleted)?;

        Ok(())
    }

    /// Disposes of the component, releasing any resources.
    ///
    /// # Errors
    ///
    /// Returns an error if the component fails to dispose.
    fn dispose(&mut self) -> anyhow::Result<()> {
        self.transition_state(ComponentTrigger::Dispose)?; // -> Disposing

        if let Err(e) = self.on_dispose() {
            log_error(&e);
            return Err(e); // Halt state transition
        }

        self.transition_state(ComponentTrigger::DisposeCompleted)?;

        Ok(())
    }

    /// Actions to be performed on start.
    ///
    /// # Errors
    ///
    /// Returns an error if starting the actor fails.
    fn on_start(&mut self) -> anyhow::Result<()> {
        log::warn!(
            "The `on_start` handler was called when not overridden, \
            it's expected that any actions required when stopping the component \
            occur here, such as unsubscribing from data",
        );
        Ok(())
    }

    /// Actions to be performed on stop.
    ///
    /// # Errors
    ///
    /// Returns an error if stopping the actor fails.
    fn on_stop(&mut self) -> anyhow::Result<()> {
        log::warn!(
            "The `on_stop` handler was called when not overridden, \
            it's expected that any actions required when stopping the component \
            occur here, such as unsubscribing from data",
        );
        Ok(())
    }

    /// Actions to be performed on resume.
    ///
    /// # Errors
    ///
    /// Returns an error if resuming the actor fails.
    fn on_resume(&mut self) -> anyhow::Result<()> {
        log::warn!(
            "The `on_resume` handler was called when not overridden, \
            it's expected that any actions required when resuming the component \
            following a stop occur here"
        );
        Ok(())
    }

    /// Actions to be performed on reset.
    ///
    /// # Errors
    ///
    /// Returns an error if resetting the actor fails.
    fn on_reset(&mut self) -> anyhow::Result<()> {
        log::warn!(
            "The `on_reset` handler was called when not overridden, \
            it's expected that any actions required when resetting the component \
            occur here, such as resetting indicators and other state"
        );
        Ok(())
    }

    /// Actions to be performed on dispose.
    ///
    /// # Errors
    ///
    /// Returns an error if disposing the actor fails.
    fn on_dispose(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed on degrade.
    ///
    /// # Errors
    ///
    /// Returns an error if degrading the actor fails.
    fn on_degrade(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    /// Actions to be performed on fault.
    ///
    /// # Errors
    ///
    /// Returns an error if faulting the actor fails.
    fn on_fault(&mut self) -> anyhow::Result<()> {
        Ok(())
    }
}

fn log_error(e: &anyhow::Error) {
    log::error!("{e}");
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
