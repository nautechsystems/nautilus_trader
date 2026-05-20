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

//! Per-instance opaque context attached to every plug-in strategy or actor.
//!
//! The host hands the plug-in a `*const HostContext` at create time; the
//! plug-in passes the same pointer back through `HostVTable::submit_order`
//! and friends. The host side interprets the pointer as
//! [`HostContextInner`] to recover the calling adapter's `ActorId`, then
//! looks the adapter up via the thread-local actor registry.
//!
//! The host owns the allocation: adapters allocate one [`HostContextInner`]
//! per instance via [`leak_host_context`] when they create the plug-in handle,
//! and release it via [`drop_host_context`] when they drop the handle.

#![allow(unsafe_code)]

#[cfg(test)]
use std::sync::{
    Mutex, MutexGuard,
    atomic::{AtomicUsize, Ordering},
};

use nautilus_model::identifiers::ActorId;
use nautilus_plugin::host::HostContext;

#[cfg(test)]
static HOST_CONTEXT_LIVE: AtomicUsize = AtomicUsize::new(0);

#[cfg(test)]
static HOST_CONTEXT_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Serializes leak-counter assertions across parallel tests. Acquire this
/// guard at the top of any test that reads [`host_context_live_count`].
/// Test-only.
#[cfg(test)]
pub fn host_context_test_lock() -> MutexGuard<'static, ()> {
    // Poisoning can occur if a panic interrupts a holder; clear it so
    // subsequent tests still serialize cleanly.
    HOST_CONTEXT_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Returns the number of [`HostContextInner`] allocations currently alive.
/// Test-only: used to verify the adapter's leak/free pairing.
#[cfg(test)]
#[must_use]
pub fn host_context_live_count() -> usize {
    HOST_CONTEXT_LIVE.load(Ordering::SeqCst)
}

/// Inner payload behind the opaque `*const HostContext` the host hands every
/// plug-in instance.
#[repr(C)]
#[derive(Debug)]
pub struct HostContextInner {
    /// Canonical identifier of the host-side adapter that owns the plug-in
    /// instance. Looked up in the thread-local actor registry by the host
    /// vtable's order-command thunks.
    pub actor_id: ActorId,

    /// Whether the registered adapter is a strategy. The host's order-command
    /// thunks reject calls coming from actor contexts because actors must not
    /// submit orders.
    pub is_strategy: bool,
}

/// Boxes `inner` on the heap, leaks it, and returns the resulting pointer as
/// a `*const HostContext` to hand to a plug-in.
///
/// Pair every leak with a matching [`drop_host_context`] when the plug-in
/// instance is dropped to avoid leaking the allocation.
#[must_use]
pub fn leak_host_context(inner: HostContextInner) -> *const HostContext {
    #[cfg(test)]
    HOST_CONTEXT_LIVE.fetch_add(1, Ordering::SeqCst);
    Box::into_raw(Box::new(inner)).cast::<HostContext>()
}

/// Reclaims a previously [`leak_host_context`]-leaked allocation.
///
/// # Safety
///
/// `ctx` must originate from [`leak_host_context`] and must not be aliased.
pub unsafe fn drop_host_context(ctx: *const HostContext) {
    if ctx.is_null() {
        return;
    }
    #[cfg(test)]
    HOST_CONTEXT_LIVE.fetch_sub(1, Ordering::SeqCst);
    // SAFETY: caller upholds the origin and aliasing contract.
    unsafe {
        drop(Box::from_raw(ctx.cast_mut().cast::<HostContextInner>()));
    }
}

/// Interprets `ctx` as a `*const HostContextInner` and returns a reference.
///
/// Returns `None` if `ctx` is null.
///
/// # Safety
///
/// `ctx` must originate from [`leak_host_context`] and must still be live.
#[must_use]
pub unsafe fn host_context_inner<'a>(ctx: *const HostContext) -> Option<&'a HostContextInner> {
    if ctx.is_null() {
        return None;
    }
    // SAFETY: caller upholds the origin and liveness contract.
    Some(unsafe { &*ctx.cast::<HostContextInner>() })
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn leak_round_trip() {
        let _guard = host_context_test_lock();
        let before = host_context_live_count();
        let id = ActorId::from("TEST-001");
        let ctx = leak_host_context(HostContextInner {
            actor_id: id,
            is_strategy: true,
        });
        assert_eq!(host_context_live_count(), before + 1);

        // SAFETY: ctx came from leak_host_context, still live.
        let inner = unsafe { host_context_inner(ctx) }.unwrap();
        assert_eq!(inner.actor_id, id);
        assert!(inner.is_strategy);

        // SAFETY: ctx came from leak_host_context.
        unsafe { drop_host_context(ctx) };
        assert_eq!(host_context_live_count(), before);
    }

    #[rstest]
    fn host_context_inner_null_returns_none() {
        // SAFETY: documented behaviour for null input.
        assert!(unsafe { host_context_inner(std::ptr::null()) }.is_none());
    }

    #[rstest]
    fn drop_host_context_null_is_noop() {
        // SAFETY: documented behaviour for null input.
        unsafe { drop_host_context(std::ptr::null()) };
    }
}
