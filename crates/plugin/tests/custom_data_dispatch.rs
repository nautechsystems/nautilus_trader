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

//! Per-slot dispatch tests for [`CustomDataVTable`].
//!
//! Each entry on `CustomDataVTable` is paired with a thunk in
//! `surfaces::custom_data`. A wiring mistake at the vtable-init site (e.g.
//! assigning `to_json_thunk` to the `from_json` slot) compiles but routes
//! through the wrong trait method. These tests guard against that class of
//! mistake by:
//!
//! 1. Implementing a hook-counting `PluginCustomData` whose every trait
//!    method (and `Drop` impl) increments a per-hook atomic counter.
//! 2. Invoking each vtable slot with a valid input.
//! 3. Asserting only the matching counter incremented.
//!
//! `type_name` and `drop_handle(null)` invoke no trait method, so their
//! tests assert every counter stays at zero.
//!
//! Mirrors the parametrised pattern in
//! [`tests/hook_dispatch.rs`](./hook_dispatch.rs) for the actor and
//! strategy surfaces.

#![allow(unsafe_code)]

use std::sync::{
    Mutex, MutexGuard, OnceLock,
    atomic::{AtomicU64, Ordering},
};

use nautilus_plugin::{
    boundary::{BorrowedStr, Slice},
    surfaces::custom_data::{
        CustomDataHandle, CustomDataVTable, MetadataEntry, PluginCustomData, PluginCustomDataRef,
        custom_data_vtable,
    },
};
use rstest::rstest;

macro_rules! generated_slot {
    ($vtable:expr, $slot:ident) => {{
        ($vtable)
            .$slot
            .expect(concat!("generated vtable includes ", stringify!($slot)))
    }};
}

// See note in hook_dispatch.rs on the `On`/method-name lint suppression.
#[allow(clippy::enum_variant_names)]
#[repr(usize)]
#[derive(Clone, Copy, Debug)]
enum CustomDataHook {
    SchemaIpc,
    FromJson,
    EncodeBatch,
    DecodeBatch,
    TsEvent,
    TsInit,
    ToJson,
    CloneValue,
    DropHandle,
    Equals,
}

const HOOK_COUNT: usize = CustomDataHook::Equals as usize + 1;
static HOOK_CALLS: [AtomicU64; HOOK_COUNT] = [const { AtomicU64::new(0) }; HOOK_COUNT];

// Serialises hook-dispatch test cases so the shared counter array is not
// contaminated by parallel runs. cargo test runs cases in parallel by
// default; each case acquires this lock for its body.
fn dispatch_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|p| p.into_inner())
}

fn reset_counters() {
    for c in &HOOK_CALLS {
        c.store(0, Ordering::SeqCst);
    }
}

fn bump(hook: CustomDataHook) {
    HOOK_CALLS[hook as usize].fetch_add(1, Ordering::SeqCst);
}

fn assert_only_hook(expected: CustomDataHook) {
    for (i, c) in HOOK_CALLS.iter().enumerate() {
        let v = c.load(Ordering::SeqCst);
        if i == expected as usize {
            assert_eq!(v, 1, "hook {expected:?} should have fired exactly once");
        } else {
            assert_eq!(
                v, 0,
                "hook at index {i} fired but {expected:?} was expected",
            );
        }
    }
}

fn assert_no_hooks_fired() {
    for (i, c) in HOOK_CALLS.iter().enumerate() {
        assert_eq!(
            c.load(Ordering::SeqCst),
            0,
            "no trait hook should have fired, but index {i} did",
        );
    }
}

// Plug-in custom-data type whose every trait method bumps a per-hook
// counter. Uses an ASCII decimal encoding so payloads are valid UTF-8 for
// the JSON-shaped `from_json`/`to_json` boundary.
#[derive(Clone, PartialEq)]
struct HookCountingTick {
    value: u64,
}

impl Drop for HookCountingTick {
    fn drop(&mut self) {
        bump(CustomDataHook::DropHandle);
    }
}

impl PluginCustomData for HookCountingTick {
    const TYPE_NAME: &'static str = "HookCountingTick";

    fn ts_event(&self) -> u64 {
        bump(CustomDataHook::TsEvent);
        self.value
    }

    fn ts_init(&self) -> u64 {
        bump(CustomDataHook::TsInit);
        self.value
    }

    fn to_json(&self) -> anyhow::Result<Vec<u8>> {
        bump(CustomDataHook::ToJson);
        Ok(self.value.to_string().into_bytes())
    }

    fn from_json(payload: &[u8]) -> anyhow::Result<Self> {
        bump(CustomDataHook::FromJson);
        let text = std::str::from_utf8(payload)?;
        Ok(Self {
            value: text.parse()?,
        })
    }

    fn schema_ipc() -> anyhow::Result<Vec<u8>> {
        bump(CustomDataHook::SchemaIpc);
        Ok(b"hook-schema".to_vec())
    }

    fn encode_batch(items: &[&Self]) -> anyhow::Result<Vec<u8>> {
        bump(CustomDataHook::EncodeBatch);
        let parts: Vec<String> = items.iter().map(|i| i.value.to_string()).collect();
        Ok(parts.join(",").into_bytes())
    }

    fn decode_batch(ipc_bytes: &[u8], _metadata: &[(String, String)]) -> anyhow::Result<Vec<Self>> {
        bump(CustomDataHook::DecodeBatch);
        let text = std::str::from_utf8(ipc_bytes)?;
        if text.is_empty() {
            return Ok(Vec::new());
        }
        text.split(',')
            .map(|p| Ok(Self { value: p.parse()? }))
            .collect()
    }

    fn equals(&self, other: &Self) -> bool {
        bump(CustomDataHook::Equals);
        self.value == other.value
    }

    fn clone_value(&self) -> Self {
        bump(CustomDataHook::CloneValue);
        Self { value: self.value }
    }
}

#[derive(Clone, PartialEq)]
struct OtherTick {
    value: u64,
}

impl PluginCustomData for OtherTick {
    const TYPE_NAME: &'static str = "OtherTick";

    fn ts_event(&self) -> u64 {
        self.value
    }

    fn ts_init(&self) -> u64 {
        self.value
    }

    fn to_json(&self) -> anyhow::Result<Vec<u8>> {
        Ok(self.value.to_string().into_bytes())
    }

    fn from_json(payload: &[u8]) -> anyhow::Result<Self> {
        let text = std::str::from_utf8(payload)?;
        Ok(Self {
            value: text.parse()?,
        })
    }

    fn schema_ipc() -> anyhow::Result<Vec<u8>> {
        Ok(Vec::new())
    }

    fn encode_batch(_items: &[&Self]) -> anyhow::Result<Vec<u8>> {
        Ok(Vec::new())
    }

    fn decode_batch(
        _ipc_bytes: &[u8],
        _metadata: &[(String, String)],
    ) -> anyhow::Result<Vec<Self>> {
        Ok(Vec::new())
    }
}

fn vtable() -> &'static CustomDataVTable {
    // SAFETY: vtable lives for the process lifetime.
    unsafe { &*custom_data_vtable::<HookCountingTick>() }
}

// Builds a live handle via `from_json` (which bumps `FromJson` and the
// returned Box's Drop later). Callers reset counters after setup so the
// per-slot assertion only observes the slot under test.
fn make_handle(value: u64) -> *mut CustomDataHandle {
    let s = value.to_string();
    let payload = BorrowedStr::from_str(&s);
    // SAFETY: payload outlives the call.
    unsafe { generated_slot!(vtable(), from_json)(payload) }
        .into_result()
        .expect("from_json")
}

#[rstest]
fn custom_data_ref_downcast_rejects_null_handle() {
    let _g = dispatch_lock();
    reset_counters();
    // SAFETY: Null handles are part of the boundary guard contract;
    // downcast_ref must return None before dereferencing.
    let data = unsafe {
        PluginCustomDataRef::from_raw_parts(
            BorrowedStr::from_str(HookCountingTick::TYPE_NAME),
            custom_data_vtable::<HookCountingTick>(),
            std::ptr::null(),
        )
    };

    assert_eq!(data.type_name(), HookCountingTick::TYPE_NAME);
    assert!(data.is::<HookCountingTick>());
    assert!(data.downcast_ref::<HookCountingTick>().is_none());
    assert_no_hooks_fired();
}

#[rstest]
fn custom_data_ref_downcast_rejects_mismatched_vtable() {
    let _g = dispatch_lock();
    let h = make_handle(42);
    reset_counters();
    // SAFETY: handle is live and was allocated by HookCountingTick's vtable.
    let data = unsafe {
        PluginCustomDataRef::from_raw_parts(
            BorrowedStr::from_str(HookCountingTick::TYPE_NAME),
            custom_data_vtable::<HookCountingTick>(),
            h.cast_const(),
        )
    };

    assert!(data.downcast_ref::<OtherTick>().is_none());
    assert_no_hooks_fired();

    // SAFETY: handle is live and is consumed by drop_handle.
    unsafe {
        generated_slot!(vtable(), drop_handle)(h);
    };
}

#[rstest]
fn type_name_slot_returns_type_name_constant_without_invoking_trait_methods() {
    let _g = dispatch_lock();
    reset_counters();
    let vt = vtable();
    // SAFETY: type_name returns a static string.
    let name = unsafe { generated_slot!(vt, type_name)() };
    // SAFETY: storage is process-lifetime static memory in this binary.
    assert_eq!(unsafe { name.as_str() }, "HookCountingTick");
    assert_no_hooks_fired();
}

#[rstest]
fn schema_ipc_slot_calls_trait_schema_ipc() {
    let _g = dispatch_lock();
    reset_counters();
    let vt = vtable();
    // SAFETY: schema_ipc takes no inputs and returns owned bytes.
    let r = unsafe { generated_slot!(vt, schema_ipc)() };
    let bytes = r.into_result().expect("schema_ipc");
    // SAFETY: buffer live until `bytes` drops.
    assert_eq!(unsafe { bytes.as_bytes() }, b"hook-schema");
    assert_only_hook(CustomDataHook::SchemaIpc);
}

#[rstest]
fn from_json_slot_calls_trait_from_json() {
    let _g = dispatch_lock();
    reset_counters();
    let vt = vtable();
    let s = "42".to_string();
    let payload = BorrowedStr::from_str(&s);
    // SAFETY: payload outlives the call.
    let handle = unsafe { generated_slot!(vt, from_json)(payload) }
        .into_result()
        .expect("from_json");
    assert!(!handle.is_null());
    assert_only_hook(CustomDataHook::FromJson);
    // SAFETY: handle is live; drop it to release the boxed value.
    unsafe {
        generated_slot!(vt, drop_handle)(handle);
    };
}

#[rstest]
fn encode_batch_slot_calls_trait_encode_batch() {
    let _g = dispatch_lock();
    let vt = vtable();
    let h1 = make_handle(1);
    let h2 = make_handle(2);
    reset_counters();

    let handles: [*const CustomDataHandle; 2] = [h1.cast_const(), h2.cast_const()];
    let slice = Slice::from_slice(&handles);
    // SAFETY: handles slice and both handles outlive the call.
    let r = unsafe { generated_slot!(vt, encode_batch)(slice) }
        .into_result()
        .expect("encode_batch");
    // SAFETY: buffer live until `r` drops.
    assert_eq!(unsafe { r.as_bytes() }, b"1,2");
    assert_only_hook(CustomDataHook::EncodeBatch);

    // SAFETY: h1 is still live.
    unsafe {
        generated_slot!(vt, drop_handle)(h1);
    };
    // SAFETY: h2 is still live.
    unsafe {
        generated_slot!(vt, drop_handle)(h2);
    };
}

#[rstest]
fn decode_batch_slot_calls_trait_decode_batch() {
    let _g = dispatch_lock();
    reset_counters();
    let vt = vtable();
    let bytes = b"5,6,7";
    let bytes_slice = Slice::from_slice(bytes);
    let md: [MetadataEntry<'_>; 0] = [];
    let md_slice = Slice::from_slice(&md);
    // SAFETY: slices outlive the call.
    let owned = unsafe { generated_slot!(vt, decode_batch)(bytes_slice, md_slice) }
        .into_result()
        .expect("decode_batch");
    assert_only_hook(CustomDataHook::DecodeBatch);

    let elem_size = std::mem::size_of::<*mut CustomDataHandle>();
    // SAFETY: buffer live and contains `count` aligned handle pointers.
    let buf = unsafe { owned.as_bytes() };
    let count = buf.len() / elem_size;
    assert_eq!(count, 3, "decoded handle count");
    let handle_ptr = buf.as_ptr().cast::<*mut CustomDataHandle>();

    for i in 0..count {
        // SAFETY: i < count and the buffer is `count * elem_size` bytes.
        let slot = unsafe { handle_ptr.add(i) };
        // SAFETY: slot points at a freshly-decoded handle pointer.
        let h = unsafe { slot.read() };
        // SAFETY: handle is live (decode just produced it).
        unsafe {
            generated_slot!(vt, drop_handle)(h);
        };
    }
    drop(owned);
}

#[rstest]
fn ts_event_slot_calls_trait_ts_event() {
    let _g = dispatch_lock();
    let vt = vtable();
    let h = make_handle(123);
    reset_counters();
    // SAFETY: handle is live.
    let v = unsafe { generated_slot!(vt, ts_event)(h) };
    assert_eq!(v, 123);
    assert_only_hook(CustomDataHook::TsEvent);
    // SAFETY: handle is live.
    unsafe {
        generated_slot!(vt, drop_handle)(h);
    };
}

#[rstest]
fn ts_init_slot_calls_trait_ts_init() {
    let _g = dispatch_lock();
    let vt = vtable();
    let h = make_handle(456);
    reset_counters();
    // SAFETY: handle is live.
    let v = unsafe { generated_slot!(vt, ts_init)(h) };
    assert_eq!(v, 456);
    assert_only_hook(CustomDataHook::TsInit);
    // SAFETY: handle is live.
    unsafe {
        generated_slot!(vt, drop_handle)(h);
    };
}

#[rstest]
fn to_json_slot_calls_trait_to_json() {
    let _g = dispatch_lock();
    let vt = vtable();
    let h = make_handle(99);
    reset_counters();
    // SAFETY: handle is live.
    let r = unsafe { generated_slot!(vt, to_json)(h) }
        .into_result()
        .expect("to_json");
    // SAFETY: bytes live until `r` drops.
    assert_eq!(unsafe { r.as_bytes() }, b"99");
    assert_only_hook(CustomDataHook::ToJson);
    // SAFETY: handle is live.
    unsafe {
        generated_slot!(vt, drop_handle)(h);
    };
}

#[rstest]
fn clone_handle_slot_calls_trait_clone_value() {
    let _g = dispatch_lock();
    let vt = vtable();
    let h = make_handle(7);
    reset_counters();
    // SAFETY: handle is live.
    let cloned = unsafe { generated_slot!(vt, clone_handle)(h) };
    assert!(!cloned.is_null());
    assert!(
        !std::ptr::eq(h.cast_const(), cloned.cast_const()),
        "clone must return a distinct allocation",
    );
    assert_only_hook(CustomDataHook::CloneValue);
    // SAFETY: original handle is live.
    unsafe {
        generated_slot!(vt, drop_handle)(h);
    };
    // SAFETY: cloned handle is live.
    unsafe {
        generated_slot!(vt, drop_handle)(cloned);
    };
}

#[rstest]
fn drop_handle_slot_calls_t_drop() {
    let _g = dispatch_lock();
    let vt = vtable();
    let h = make_handle(11);
    reset_counters();
    // SAFETY: handle is live and is consumed by drop_handle.
    unsafe {
        generated_slot!(vt, drop_handle)(h);
    };
    assert_only_hook(CustomDataHook::DropHandle);
}

#[rstest]
fn drop_handle_slot_is_null_safe() {
    let _g = dispatch_lock();
    reset_counters();
    let vt = vtable();
    // SAFETY: the documented contract: drop_handle ignores null pointers.
    unsafe {
        generated_slot!(vt, drop_handle)(std::ptr::null_mut());
    };
    assert_no_hooks_fired();
}

#[rstest]
#[case::equal(7, 7, true)]
#[case::unequal(7, 8, false)]
fn eq_handles_slot_calls_trait_equals(#[case] lhs: u64, #[case] rhs: u64, #[case] expected: bool) {
    let _g = dispatch_lock();
    let vt = vtable();
    let h1 = make_handle(lhs);
    let h2 = make_handle(rhs);
    reset_counters();
    // SAFETY: both handles are live.
    let eq = unsafe { generated_slot!(vt, eq_handles)(h1, h2) };
    assert_eq!(eq, expected);
    assert_only_hook(CustomDataHook::Equals);
    // SAFETY: h1 is live.
    unsafe {
        generated_slot!(vt, drop_handle)(h1);
    };
    // SAFETY: h2 is live.
    unsafe {
        generated_slot!(vt, drop_handle)(h2);
    };
}
