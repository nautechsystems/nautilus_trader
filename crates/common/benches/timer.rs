// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.txt
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use std::{
    hint::black_box,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use nautilus_common::{
    clock::CallbackRegistry,
    timer::{TimeEvent, TimeEventCallback, TimeEventHandler},
};
use nautilus_core::UUID4;
use ustr::Ustr;

fn bench_dispatch(c: &mut Criterion) {
    let mut group = c.benchmark_group("timer/dispatch");
    group.throughput(Throughput::Elements(1));

    for (name, local, guarded) in [
        ("rust_raw", false, false),
        ("rust_guarded", false, true),
        ("local_raw", true, false),
        ("local_guarded", true, true),
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let callback_calls = calls.clone();
        let callback = move |_: TimeEvent| {
            callback_calls.fetch_add(1, Ordering::Relaxed);
        };

        let callback = if local {
            TimeEventCallback::RustLocal(Rc::new(callback))
        } else {
            TimeEventCallback::from(callback)
        };

        let event = TimeEvent::new(Ustr::from("dispatch"), UUID4::new(), 20.into(), 21.into());
        let mut registry = CallbackRegistry::new();
        registry.register_default_handler(callback.clone());
        let handler = if guarded {
            registry.get_handler(event.clone())
        } else {
            TimeEventHandler::new(event.clone(), callback.clone())
        };

        handler.clone().run();
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        registry.cancel_default_handler();
        handler.run();
        assert_eq!(calls.load(Ordering::Relaxed), if guarded { 1 } else { 2 });

        registry.register_default_handler(callback.clone());
        let handler = if guarded {
            registry.get_handler(event)
        } else {
            TimeEventHandler::new(event, callback)
        };

        handler.clone().run();
        assert_eq!(calls.load(Ordering::Relaxed), if guarded { 2 } else { 3 });

        // Include queued-handler ownership and dispatch, with setup and revocation outside timing.
        group.bench_function(name, |b| b.iter(|| black_box(handler.clone()).run()));
    }

    group.finish();
}

criterion_group!(benches, bench_dispatch);
criterion_main!(benches);
