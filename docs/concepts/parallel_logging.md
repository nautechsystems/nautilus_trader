# Parallel Thread Logging

The Nautilus logging system supports unified logging across parallel threads and async tasks. This ensures that all log messages from different execution contexts are properly formatted, filtered, and output through the same logging infrastructure.

## Overview

The Nautilus logger uses a multi-producer single-consumer (MPSC) channel architecture where:

- Multiple threads/tasks can send log events to a central logging thread
- All log messages are processed and formatted consistently
- The global logger sender is automatically available to all threads once initialized

## Key Functions

### `spawn_with_logging`

A convenience wrapper around `std::thread::spawn` that ensures the spawned thread can use the global logger:

```rust
use nautilus_common::logging::spawn_with_logging;

let handle = spawn_with_logging(|| {
    log::info!("Message from parallel thread");
    // Your thread work here
    42
});

let result = handle.join().unwrap();
```

### `spawn_task_with_logging`

A convenience wrapper around `tokio::spawn` that ensures the spawned task can use the global logger:

```rust
use nautilus_common::logging::spawn_task_with_logging;

let handle = spawn_task_with_logging(async {
    log::info!("Message from async task");
    // Your async work here
    "result"
});

let result = handle.await.unwrap();
```

### `spawn_task_on_runtime_with_logging`

A convenience wrapper around `get_runtime().spawn` that ensures the spawned task runs on the specific Nautilus runtime and can use the global logger. Use this when you need to ensure the task runs on the configured Nautilus runtime rather than the current runtime context:

```rust
use nautilus_common::logging::spawn_task_on_runtime_with_logging;

let handle = spawn_task_on_runtime_with_logging(async {
    log::info!("Message from async task on Nautilus runtime");
    // Your async work here
    "result"
});

let result = handle.await.unwrap();
```

### `get_logger_sender`

Returns a cloned sender for the global logger if it has been initialized:

```rust
use nautilus_common::logging::get_logger_sender;

if let Some(sender) = get_logger_sender() {
    // Logger is available - threads can log normally
    std::thread::spawn(|| {
        log::info!("Logging is available");
    });
}
```

### `init_thread_logging`

Checks if the global logger has been initialized and is available:

```rust
use nautilus_common::logging::init_thread_logging;

std::thread::spawn(|| {
    if init_thread_logging() {
        log::info!("Logging is available in this thread");
    } else {
        eprintln!("Logging not initialized");
    }
});
```

## How It Works

1. **Logger Initialization**: When the main logger is initialized via `init_logging()`, it:
   - Creates an MPSC channel for log events
   - Stores a cloned sender in a global static variable (`LOGGER_TX`)
   - Spawns a background thread to process log events

2. **Thread Logging**: When using standard Rust logging macros (`log::info!`, etc.):
   - The global logger automatically sends events through the MPSC channel
   - All threads inherit the same global logger configuration
   - No additional setup is required in most cases

3. **Convenience Functions**: The `spawn_with_logging` and `spawn_task_with_logging` functions:
   - Automatically check if logging is available
   - Log a debug message when the thread/task starts
   - Provide a consistent API for spawning logged threads/tasks

## Example Usage

```rust
use nautilus_common::logging::{
    init_logging, spawn_task_with_logging, spawn_with_logging, LoggerConfig,
};
use nautilus_common::logging::writer::FileWriterConfig;
use nautilus_core::UUID4;
use nautilus_model::identifiers::TraderId;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize the Nautilus logger
    let _log_guard = init_logging(
        TraderId::from("TRADER-001"),
        UUID4::new(),
        LoggerConfig::default(),
        FileWriterConfig::default(),
    )?;

    log::info!("Starting parallel work");

    // Spawn a standard thread with logging
    let thread_handle = spawn_with_logging(|| {
        log::info!("Working in parallel thread");
        42
    });

    // Spawn an async task with logging
    let task_handle = spawn_task_with_logging(async {
        log::info!("Working in async task");
        "result"
    });

    // Wait for completion
    let thread_result = thread_handle.join().unwrap();
    let task_result = task_handle.await.unwrap();

    log::info!("Results: {} and {}", thread_result, task_result);
    Ok(())
}
```

## Benefits

- **Unified Output**: All log messages go through the same formatting and filtering system
- **Consistent Configuration**: Single place to control log levels, colors, and output destinations
- **Thread Safety**: The MPSC channel ensures thread-safe logging without performance bottlenecks
- **Easy Integration**: Drop-in replacements for `std::thread::spawn` and `tokio::spawn`
- **Automatic Setup**: No manual logger initialization required in spawned threads

## Integration with Tracing

While Nautilus provides its own logging system optimized for trading applications, the tracing ecosystem offers additional features for async/parallel scenarios:

- **Structured Spans**: Context that can be passed between threads
- **Async-Aware**: Built-in support for async task instrumentation
- **Rich Ecosystem**: OpenTelemetry integration, metrics, profiling

For development and debugging of async Rust code, consider using both systems:

- Nautilus logging for user-facing operational logs
- Tracing for detailed development and debugging information

## Running the Example

To see parallel logging in action, run the provided example:

```bash
cargo run -p nautilus-common --example parallel_logging_example
```

This demonstrates logging from multiple standard threads and async tasks all being properly unified through the Nautilus logging system.
