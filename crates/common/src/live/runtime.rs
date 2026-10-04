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

//! The centralized Tokio runtime for a running Nautilus system.
//!
//! # Design Rationale
//!
//! NautilusTrader uses a single global Tokio runtime because:
//! - A single long-lived runtime avoids repeated startup/shutdown overhead.
//! - The runtime is lazily initialized on first call to `get_runtime()` via `OnceLock`.
//! - Worker thread count is configurable via the `NAUTILUS_WORKER_THREADS` environment variable.
//! - Rust-native hosts can install a pre-built runtime via [`set_runtime`] before first use.
//!
//! # Custom Runtime Injection
//!
//! Callers who use [`set_runtime`] must supply a multi-threaded runtime built with
//! `tokio::runtime::Builder::new_multi_thread()` and `enable_all()`. Adapters assume I/O,
//! timers, spawning, and `tokio::task::block_in_place()` are available.
//!
//! # Python Support
//!
//! When the `python` feature is enabled, the runtime initializes the Python interpreter
//! before starting worker threads. The PyO3 module registers an `atexit` handler via
//! `shutdown_runtime()` to cleanly shut down when Python exits.
//!
//! A runtime passed to [`set_runtime`] is already built, so this module cannot run the default
//! Python initialization hook before its worker threads start. Hosts using custom runtimes with
//! Python support must prepare Python before building the runtime.
//!
//! # Testing Considerations
//!
//! The global runtime pattern makes it harder to inject test doubles. For testing:
//! - Unit tests can use `#[tokio::test]` which creates its own runtime.
//! - Integration tests should be aware they share the global runtime state.

use std::{cell::Cell, future::Future, sync::OnceLock, time::Duration};

use tokio::{runtime::Builder, task, time::timeout};

struct NautilusRuntime {
    runtime: tokio::runtime::Runtime,
    injected: bool,
}

static RUNTIME: OnceLock<NautilusRuntime> = OnceLock::new();

thread_local! {
    static NAUTILUS_RUNTIME_THREAD: Cell<bool> = const { Cell::new(false) };
}

/// Environment variable name to configure the number of OS threads for the common runtime.
/// If not set or if the value cannot be parsed as a positive integer, Tokio's default is used.
const NAUTILUS_WORKER_THREADS: &str = "NAUTILUS_WORKER_THREADS";

/// Creates and configures a new multi-threaded Tokio runtime.
///
/// The number of OS threads is configured using the `NAUTILUS_WORKER_THREADS`
/// environment variable. If not set, all available logical CPUs will be used.
///
/// # Panics
///
/// Panics if the runtime could not be created, which typically indicates
/// an inability to spawn threads or allocate necessary resources.
fn initialize_runtime() -> NautilusRuntime {
    // Initialize Python if running as a Python extension module
    #[cfg(feature = "python")]
    {
        crate::python::runtime::initialize_python();
    }

    let worker_threads = std::env::var(NAUTILUS_WORKER_THREADS)
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .unwrap_or_default();

    let mut builder = Builder::new_multi_thread();

    if worker_threads > 0 {
        builder.worker_threads(worker_threads);
    }

    let runtime = builder
        .on_thread_start(|| NAUTILUS_RUNTIME_THREAD.set(true))
        .on_thread_stop(|| NAUTILUS_RUNTIME_THREAD.set(false))
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime");
    NautilusRuntime {
        runtime,
        injected: false,
    }
}

/// Sets a custom pre-built Tokio runtime as the global Nautilus runtime.
///
/// Must be called before the first [`get_runtime`] invocation (i.e. before
/// `LiveNode::build()` or any adapter/client usage). This gives callers who
/// own `main()` full control over worker threads, blocking threads, thread
/// names, stack sizes, and any other [`tokio::runtime::Builder`] options.
///
/// # Runtime Requirements
///
/// The supplied runtime must be multi-threaded and have all Tokio drivers
/// enabled with `tokio::runtime::Builder::enable_all()`.
///
/// # Errors
///
/// Returns `Err(runtime)` if the runtime is not multi-threaded or a runtime was already initialized.
pub fn set_runtime(runtime: tokio::runtime::Runtime) -> Result<(), tokio::runtime::Runtime> {
    if RUNTIME.get().is_some()
        || !matches!(
            runtime.handle().runtime_flavor(),
            tokio::runtime::RuntimeFlavor::MultiThread
        )
    {
        return Err(runtime);
    }

    RUNTIME
        .set(NautilusRuntime {
            runtime,
            injected: true,
        })
        .map_err(|runtime| runtime.runtime)
}

/// Returns a reference to the global Nautilus Tokio runtime.
///
/// The runtime is lazily initialized on the first call and reused thereafter.
/// If a custom runtime was previously installed via [`set_runtime`], that
/// runtime is returned instead.
pub fn get_runtime() -> &'static tokio::runtime::Runtime {
    &RUNTIME.get_or_init(initialize_runtime).runtime
}

/// Runs `f` with `block_in_place` on a thread owned by the Nautilus runtime.
///
/// # Panics
///
/// Panics from a `LocalSet` driven on a Nautilus-owned thread, including any `LocalSet` driven
/// against an injected Nautilus runtime, because Tokio does not permit `block_in_place` while
/// polling local tasks.
pub fn block_in_place_on_nautilus<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return f();
    };

    if is_on_nautilus_runtime(&handle) {
        tokio::task::block_in_place(f)
    } else {
        f()
    }
}

/// Blocks on `future` using the global Nautilus runtime.
///
/// The future must not contain tasks, timers, or I/O resources already bound to an ambient
/// runtime. Use [`block_on_nautilus_with`] when the operation can be constructed lazily.
///
/// # Panics
///
/// Panics when called from a current-thread runtime or a `LocalSet`. Moving a potentially
/// non-`Send` future out of those contexts is not possible; use [`block_on_nautilus_with`] for
/// operations whose future and output can cross a scoped thread boundary.
pub fn block_on_nautilus<F>(future: F) -> F::Output
where
    F: Future,
{
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return get_runtime().block_on(future);
    };

    assert!(
        matches!(
            handle.runtime_flavor(),
            tokio::runtime::RuntimeFlavor::MultiThread
        ),
        "block_on_nautilus cannot run inside a current-thread Tokio runtime; use block_on_nautilus_with"
    );

    tokio::task::block_in_place(|| get_runtime().block_on(future))
}

/// Constructs and blocks on a future using the global Nautilus runtime.
///
/// The factory runs after entering the Nautilus runtime so Tokio resources created by the
/// operation bind to that runtime rather than an ambient caller runtime. Resources captured by
/// the factory must not depend on the ambient runtime making progress.
///
/// Calls from a foreign Tokio runtime synchronously park the calling thread while the operation
/// runs. Tokio does not expose whether a foreign runtime is polling a `LocalSet`, so this bridge
/// cannot use `block_in_place` there without breaking supported `LocalSet` callers.
///
/// # Panics
///
/// Panics from a `LocalSet` driven on a Nautilus-owned thread. Tokio does not expose whether the
/// current multi-thread runtime context is polling local tasks, so the bridge cannot both preserve
/// scheduler progress with `block_in_place` and support that context. A `LocalSet` hosted by a
/// foreign runtime, or driven from an external thread against the default Nautilus runtime, is
/// supported. Same-runtime `LocalSet` calls are not supported with an injected runtime because its
/// already-built runtime has no thread-ownership callbacks.
pub fn block_on_nautilus_with<C, F>(create_future: C) -> F::Output
where
    C: FnOnce() -> F + Send,
    F: Future,
    F::Output: Send,
{
    let run = move || get_runtime().block_on(async move { create_future().await });
    let Ok(handle) = tokio::runtime::Handle::try_current() else {
        return run();
    };

    if is_on_nautilus_runtime(&handle) {
        return tokio::task::block_in_place(run);
    }

    std::thread::scope(|scope| {
        let task = scope.spawn(run);
        match task.join() {
            Ok(output) => output,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    })
}

fn is_on_nautilus_runtime(handle: &tokio::runtime::Handle) -> bool {
    RUNTIME.get().is_some_and(|runtime| {
        handle.id() == runtime.runtime.handle().id()
            && (runtime.injected || NAUTILUS_RUNTIME_THREAD.get())
    })
}

/// Provides a best-effort flush for runtime tasks during shutdown.
///
/// The function yields once to the Tokio scheduler and gives outstanding tasks a chance
/// to observe shutdown signals before Python finalizes the interpreter, which calls this via
/// an `atexit` hook.
pub fn shutdown_runtime(wait: Duration) {
    if let Some(runtime) = RUNTIME.get() {
        runtime.runtime.block_on(async {
            let _ = timeout(wait, async {
                task::yield_now().await;
            })
            .await;
        });
    }
}

#[cfg(test)]
#[expect(
    clippy::disallowed_types,
    reason = "tests exercise direct Tokio LocalSet interoperability"
)]
mod tests {
    use std::process::Command;

    use rstest::rstest;

    use super::*;

    const RUNTIME_CHILD_ENV: &str = "NAUTILUS_COMMON_RUNTIME_CHILD";

    #[rstest]
    fn test_custom_runtime_installation_and_rejection() {
        const MARKER: &str = "custom-runtime-installation";
        if !in_runtime_child(MARKER) {
            run_runtime_child("test_custom_runtime_installation_and_rejection", MARKER);
            return;
        }

        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("custom runtime should build");
        let installed_id = runtime.handle().id();

        assert!(set_runtime(runtime).is_ok());
        assert_eq!(get_runtime().handle().id(), installed_id);

        let duplicate = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("duplicate runtime should build");
        let duplicate_id = duplicate.handle().id();
        assert_ne!(duplicate_id, installed_id);

        let rejected = set_runtime(duplicate).expect_err("duplicate runtime should be rejected");
        assert_eq!(rejected.handle().id(), duplicate_id);
        assert_eq!(get_runtime().handle().id(), installed_id);
    }

    fn in_runtime_child(marker: &str) -> bool {
        std::env::var(RUNTIME_CHILD_ENV).as_deref() == Ok(marker)
    }

    fn run_runtime_child(test_name: &str, marker: &str) {
        let output = Command::new(std::env::current_exe().expect("test executable must exist"))
            .arg(test_name)
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(RUNTIME_CHILD_ENV, marker)
            .output()
            .expect("runtime child process must start");

        assert!(
            output.status.success(),
            "runtime child failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    #[rstest]
    fn set_runtime_rejects_current_thread_runtime() {
        const MARKER: &str = "reject-current-thread";
        if std::env::var(RUNTIME_CHILD_ENV).as_deref() != Ok(MARKER) {
            run_runtime_child("set_runtime_rejects_current_thread_runtime", MARKER);
            return;
        }
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let rejected = set_runtime(runtime).unwrap_err();

        assert_eq!(
            rejected.handle().runtime_flavor(),
            tokio::runtime::RuntimeFlavor::CurrentThread
        );
    }

    #[rstest]
    fn injected_runtime_drives_bridge_future() {
        const MARKER: &str = "injected-bridge";
        if std::env::var(RUNTIME_CHILD_ENV).as_deref() != Ok(MARKER) {
            run_runtime_child("injected_runtime_drives_bridge_future", MARKER);
            return;
        }
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let expected_id = runtime.handle().id();
        set_runtime(runtime).unwrap();

        let actual_id = block_on_nautilus_with(|| async { tokio::runtime::Handle::current().id() });

        assert_eq!(actual_id, expected_id);
    }

    #[rstest]
    fn block_on_nautilus_with_works_without_current_runtime() {
        let value = block_on_nautilus_with(|| async { 42 });

        assert_eq!(value, 42);
    }

    #[rstest]
    fn block_on_nautilus_with_works_inside_multi_thread_runtime() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let value = runtime.block_on(async {
            block_on_nautilus_with(|| async {
                tokio::time::sleep(Duration::from_millis(1)).await;
                42
            })
        });

        assert_eq!(value, 42);
    }

    #[rstest]
    fn block_on_nautilus_with_works_inside_current_thread_runtime() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let value = runtime.block_on(async {
            block_on_nautilus_with(|| async {
                tokio::time::sleep(Duration::from_millis(1)).await;
                42
            })
        });

        assert_eq!(value, 42);
    }

    #[rstest]
    fn block_on_nautilus_with_works_inside_multi_thread_local_set() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let local_set = tokio::task::LocalSet::new();
        let value = runtime.block_on(local_set.run_until(async {
            block_on_nautilus_with(|| async {
                tokio::time::sleep(Duration::from_millis(1)).await;
                42
            })
        }));

        assert_eq!(value, 42);
    }

    #[rstest]
    fn block_on_nautilus_works_inside_foreign_multi_thread_runtime() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let value = runtime.block_on(async { block_on_nautilus(async { 42 }) });

        assert_eq!(value, 42);
    }

    #[rstest]
    #[should_panic(expected = "block_on_nautilus cannot run inside a current-thread Tokio runtime")]
    fn block_on_nautilus_rejects_current_thread_runtime() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async { block_on_nautilus(async { 42 }) });
    }

    #[rstest]
    fn block_on_nautilus_with_works_inside_nautilus_worker() {
        let (caller_thread, factory_thread, value) = get_runtime().block_on(async {
            get_runtime()
                .spawn(async {
                    let caller_thread = std::thread::current().id();
                    let (factory_thread, value) = block_on_nautilus_with(|| async {
                        let factory_thread = std::thread::current().id();
                        tokio::time::sleep(Duration::from_millis(1)).await;
                        (factory_thread, 42)
                    });
                    (caller_thread, factory_thread, value)
                })
                .await
                .unwrap()
        });

        assert_eq!(factory_thread, caller_thread);
        assert_eq!(value, 42);
    }

    #[rstest]
    fn block_on_nautilus_with_works_inside_nautilus_blocking_thread() {
        let value = get_runtime().block_on(async {
            get_runtime()
                .spawn_blocking(|| {
                    block_on_nautilus_with(|| async {
                        tokio::time::sleep(Duration::from_millis(1)).await;
                        42
                    })
                })
                .await
                .unwrap()
        });

        assert_eq!(value, 42);
    }

    #[rstest]
    fn block_in_place_on_nautilus_works_inside_nautilus_local_set() {
        let local_set = tokio::task::LocalSet::new();
        let value = get_runtime()
            .block_on(local_set.run_until(async { block_in_place_on_nautilus(|| 42) }));

        assert_eq!(value, 42);
    }
}
