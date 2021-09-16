// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

use uuid::Uuid;

fn uuid4() -> String {
    // UUID version 4
    Uuid::new_v4().to_string()
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uuid4() {
        let uuid_str = uuid4();

        assert_eq!(uuid_str.len(), 36);
    }
}
