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

pub fn precision_from_str(s: &str) -> u8 {
    let lower_s = s.to_lowercase();
    if lower_s.find(".").is_none() {
        return 0;
    }
    return lower_s.split(".").last().unwrap().len() as u8;
    // TODO(cs): Implement scientific notation parsing
}

#[cfg(test)]
mod tests {
    use crate::text::precision_from_str;

    #[test]
    fn test_precision_from_str() {
        assert_eq!(precision_from_str("1"), 0);
        assert_eq!(precision_from_str("2.1"), 1);
        assert_eq!(precision_from_str("2.204622"), 6);
    }
}
