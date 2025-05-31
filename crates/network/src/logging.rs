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

/// Logs that a task has started using `tracing::debug!`.
pub fn log_task_started(task_name: &str) {
    tracing::debug!("Started task '{task_name}'");
}

/// Logs that a task has stopped using `tracing::debug!`.
pub fn log_task_stopped(task_name: &str) {
    tracing::debug!("Stopped task '{task_name}'");
}

/// Logs that a task was aborted using `tracing::debug!`.
pub fn log_task_aborted(task_name: &str) {
    tracing::debug!("Aborted task '{task_name}'");
}
