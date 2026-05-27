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

//! Malformed plug-in fixture whose macro-generated init panics while building the manifest.

nautilus_plugin::nautilus_plugin! {
    name: panic_plugin_name(),
    vendor: "Nautech",
    version: env!("CARGO_PKG_VERSION"),
}

fn panic_plugin_name() -> &'static str {
    panic!("manifest init panic")
}

#[allow(dead_code)]
fn main() {}
