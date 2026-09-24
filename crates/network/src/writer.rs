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

//! Admission limits shared by writer queues and reconnect buffers.

use std::sync::Arc;

use crate::error::{NetworkConfigError, NetworkConfigResult, SendError};

pub(crate) const DEFAULT_WRITER_CAPACITY: usize = 1_024;

/// A writer command sender with a shared limit on outstanding messages.
///
/// Clones share capacity. A message holds its slot until written or discarded, including
/// while it waits for reconnect replay.
#[derive(Debug)]
pub struct WriterSender<T> {
    tx: tokio::sync::mpsc::UnboundedSender<(T, tokio::sync::OwnedSemaphorePermit)>,
    slots: Arc<tokio::sync::Semaphore>,
    slots_control: Arc<tokio::sync::Semaphore>,
    slots_update: Arc<tokio::sync::Semaphore>,
}

impl<T> Clone for WriterSender<T> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            slots: Arc::clone(&self.slots),
            slots_control: Arc::clone(&self.slots_control),
            slots_update: Arc::clone(&self.slots_update),
        }
    }
}

impl<T> WriterSender<T> {
    /// Enqueues a command without waiting for capacity.
    ///
    /// # Errors
    ///
    /// Returns [`SendError::BufferFull`] if all slots are occupied, or
    /// [`SendError::BrokenPipe`] if the writer has stopped.
    pub fn send(&self, command: T) -> Result<(), SendError> {
        self.send_with_slots(command, &self.slots)
    }

    // Auth recovery and keepalives must not compete with messages held for replay
    pub(crate) fn send_control(&self, command: T) -> Result<(), SendError> {
        self.send_with_slots(command, &self.slots_control)
    }

    // Reconnect timeouts can abandon an update before the writer processes it,
    // reserve one independent slot so retries stay bounded even when replay is full.
    pub(crate) fn send_update(&self, command: T) -> Result<(), SendError> {
        self.send_with_slots(command, &self.slots_update)
    }

    fn send_with_slots(
        &self,
        command: T,
        slots: &Arc<tokio::sync::Semaphore>,
    ) -> Result<(), SendError> {
        if self.tx.is_closed() {
            return Err(SendError::BrokenPipe("writer channel closed".to_string()));
        }

        let permit = Arc::clone(slots)
            .try_acquire_owned()
            .map_err(|_| SendError::BufferFull)?;
        self.tx
            .send((command, permit))
            .map_err(|_| SendError::BrokenPipe("writer channel closed".to_string()))
    }
}

pub(crate) type WriterReceiver<T> =
    tokio::sync::mpsc::UnboundedReceiver<(T, tokio::sync::OwnedSemaphorePermit)>;

pub(crate) fn channel<T>(capacity: usize) -> (WriterSender<T>, WriterReceiver<T>) {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    (
        WriterSender {
            tx,
            slots: Arc::new(tokio::sync::Semaphore::new(capacity)),
            slots_control: Arc::new(tokio::sync::Semaphore::new(capacity)),
            slots_update: Arc::new(tokio::sync::Semaphore::new(1)),
        },
        rx,
    )
}

pub(crate) fn validate_capacity(capacity: Option<usize>) -> NetworkConfigResult<()> {
    if let Some(capacity) = capacity
        && (capacity == 0 || capacity > tokio::sync::Semaphore::MAX_PERMITS)
    {
        return Err(NetworkConfigError::invalid(
            "writer_capacity",
            "must be positive and no greater than Semaphore::MAX_PERMITS",
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[tokio::test]
    async fn test_capacity_follows_message_until_discarded() {
        let (sender, mut receiver) = channel(1);
        sender.send("accepted").unwrap();
        let (message, permit) = receiver.recv().await.unwrap();
        let clone = sender.clone();

        assert_eq!(message, "accepted");
        assert!(matches!(clone.send("overflow"), Err(SendError::BufferFull)));
        sender.send_control("authentication").unwrap();
        assert!(matches!(
            clone.send_control("overflow"),
            Err(SendError::BufferFull)
        ));
        let (control, control_permit) = receiver.recv().await.unwrap();
        assert_eq!(control, "authentication");
        assert!(matches!(
            clone.send_control("overflow"),
            Err(SendError::BufferFull)
        ));
        drop(control_permit);
        sender.send_update("replacement").unwrap();
        let (update, update_permit) = receiver.recv().await.unwrap();
        assert_eq!(update, "replacement");
        assert!(matches!(
            sender.send_update("overflow"),
            Err(SendError::BufferFull)
        ));
        drop(update_permit);
        sender.send_update("retry").unwrap();
        let (retry, _) = receiver.recv().await.unwrap();
        assert_eq!(retry, "retry");
        assert!(matches!(
            sender.send("overflow"),
            Err(SendError::BufferFull)
        ));

        drop(permit);
        clone.send("after discard").unwrap();
        let (message, _) = receiver.recv().await.unwrap();
        assert_eq!(message, "after discard");

        drop(receiver);
        assert!(matches!(
            sender.send("closed"),
            Err(SendError::BrokenPipe(_))
        ));
    }
}
