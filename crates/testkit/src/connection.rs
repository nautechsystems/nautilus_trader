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

//! Deterministic command injection at an asynchronous connection publication boundary.

use std::{
    future::{Future, poll_fn},
    pin::Pin,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
    time::Duration,
};

use parking_lot::Mutex;

/// Connects while injecting a command immediately after the shared sender becomes available.
///
/// `command` constructs the command after acquiring the newly published sender.
/// The sender must use Tokio's fair read/write lock. This function holds a reader until
/// `connect` queues its publication writer, then queues the command's reader behind it.
/// The reader's waker sends the command synchronously during publication, before `connect`
/// can continue past releasing the writer. No scheduler timing or sleep is required.
///
/// # Panics
///
/// Panics if connection or publication takes more than five seconds, `connect` finishes before
/// publication, `active` is true before publication, or the command cannot enter the new channel.
pub async fn race_command_with_publication<C, F, R>(
    sender: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<C>>>,
    connect: F,
    command: impl FnOnce() -> C + Send + 'static,
    active: impl Fn() -> bool,
) -> R
where
    C: Send + 'static,
    F: Future<Output = R>,
{
    let publication = sender.read().await;
    let mut connect = Box::pin(connect);
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let polled = poll_fn(|cx| Poll::Ready(connect.as_mut().poll(cx))).await;
            assert!(polled.is_pending(), "connect finished before publication");

            if sender.try_read().is_err() {
                break;
            }

            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("connect must wait to publish its sender");

    let active_before_publication = active();
    let command_sender = Arc::clone(&sender);
    let command = PublicationCommand {
        future: Mutex::new(Some(Box::pin(async move {
            let sender = command_sender.read().await;
            assert!(sender.send(command()).is_ok());
        }))),
    };

    let command = Arc::new(command);
    let waker = Waker::from(Arc::clone(&command));
    assert!(command.poll(&waker).is_pending());
    drop(publication);

    let result = tokio::time::timeout(Duration::from_secs(5), connect)
        .await
        .expect("connect must complete after publication");
    assert!(command.future.lock().is_none(), "command was not sent");
    assert!(
        !active_before_publication,
        "active before sender publication"
    );
    result
}

struct PublicationCommand<F> {
    future: Mutex<Option<Pin<Box<F>>>>,
}

impl<F: Future<Output = ()>> PublicationCommand<F> {
    fn poll(&self, waker: &Waker) -> Poll<()> {
        let mut future = self.future.lock();
        let polled = future
            .as_mut()
            .expect("publication command polled after completion")
            .as_mut()
            .poll(&mut Context::from_waker(waker));

        if polled.is_ready() {
            *future = None;
        }

        polled
    }
}

impl<F: Future<Output = ()> + Send + 'static> Wake for PublicationCommand<F> {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        assert!(self.poll(Waker::noop()).is_ready());
    }
}
