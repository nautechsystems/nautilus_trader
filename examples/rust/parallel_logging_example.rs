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

//! Example demonstrating parallel thread logging in Nautilus.
//!
//! This example shows how to use the logging utilities to ensure that
//! parallel threads and async tasks can log to the unified Nautilus logger.

use std::time::Duration;

use nautilus_common::logging::{
    LoggerConfig, init_logging, spawn_task_with_logging, spawn_with_logging,
    writer::FileWriterConfig,
};
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

    log::info!("Starting parallel logging example");

    // Example 1: Standard thread with logging
    log::info!("Spawning standard thread with logging");
    let thread_handle = spawn_with_logging(|| {
        log::info!("Hello from standard thread!");
        log::debug!("This is a debug message from thread");

        // Simulate some work
        std::thread::sleep(Duration::from_millis(100));

        log::info!("Standard thread work completed");
        42
    });

    // Example 2: Tokio task with logging
    log::info!("Spawning tokio task with logging");
    let task_handle = spawn_task_with_logging(async {
        log::info!("Hello from tokio task!");
        log::debug!("This is a debug message from async task");

        // Simulate some async work
        tokio::time::sleep(Duration::from_millis(100)).await;

        log::info!("Async task work completed");
        "task_result"
    });

    // Example 3: Multiple parallel threads
    log::info!("Spawning multiple parallel threads");
    let mut handles = Vec::new();

    for i in 0..3 {
        let handle = spawn_with_logging(move || {
            log::info!("Worker thread {} starting", i);

            // Simulate different amounts of work
            std::thread::sleep(Duration::from_millis(50 * (i + 1)));

            log::info!("Worker thread {} completed", i);
            i * 10
        });
        handles.push(handle);
    }

    // Example 4: Multiple async tasks
    log::info!("Spawning multiple async tasks");
    let mut task_handles = Vec::new();

    for i in 0..3 {
        let handle = spawn_task_with_logging(async move {
            log::info!("Async worker {} starting", i);

            // Simulate different amounts of async work
            tokio::time::sleep(Duration::from_millis(30 * (i + 1))).await;

            log::info!("Async worker {} completed", i);
            format!("async_result_{}", i)
        });
        task_handles.push(handle);
    }

    // Wait for all work to complete
    log::info!("Waiting for all work to complete...");

    // Wait for standard thread
    let thread_result = thread_handle.join().unwrap();
    log::info!("Standard thread returned: {}", thread_result);

    // Wait for tokio task
    let task_result = task_handle.await.unwrap();
    log::info!("Tokio task returned: {}", task_result);

    // Wait for all worker threads
    for (i, handle) in handles.into_iter().enumerate() {
        let result = handle.join().unwrap();
        log::info!("Worker thread {} returned: {}", i, result);
    }

    // Wait for all async tasks
    for (i, handle) in task_handles.into_iter().enumerate() {
        let result = handle.await.unwrap();
        log::info!("Async worker {} returned: {}", i, result);
    }

    log::info!("All parallel work completed successfully!");
    log::info!("Example finished - check that all log messages appeared in order");

    Ok(())
}
