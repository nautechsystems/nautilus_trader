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

// TODO: TBD if we need this trait

// This is a simplified interface - we'll need to implement an actual executor later
pub trait ActorExecutor: Send + Sync {
    fn queue_for_executor(&self, task: Box<dyn FnOnce() + Send>) -> String;
    fn run_in_executor(&self, task: Box<dyn FnOnce() + Send>) -> String;
    fn queued_task_ids(&self) -> Vec<String>;
    fn active_task_ids(&self) -> Vec<String>;
    fn has_queued_tasks(&self) -> bool;
    fn has_active_tasks(&self) -> bool;
    fn cancel_task(&self, task_id: &str);
    fn cancel_all_tasks(&self);
}
