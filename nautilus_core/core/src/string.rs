// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
    // Handle scientific notation
    if lower_s.contains("e-") {
        return lower_s.split("e-").last().unwrap().parse::<u8>().unwrap();
    }
    if !lower_s.contains('.') {
        return 0;
    }
    return lower_s.split('.').last().unwrap().len() as u8;
}

#[cfg(test)]
mod tests {
    use crate::string::precision_from_str;

    #[test]
    fn test_precision_from_str() {
        assert_eq!(precision_from_str(""), 0);
        assert_eq!(precision_from_str("0"), 0);
        assert_eq!(precision_from_str("1"), 0);
        assert_eq!(precision_from_str("1.0"), 1);
        assert_eq!(precision_from_str("2.1"), 1);
        assert_eq!(precision_from_str("2.204622"), 6);
        assert_eq!(precision_from_str("0.000000001"), 9);
        assert_eq!(precision_from_str("1e-8"), 8);
        assert_eq!(precision_from_str("2e-9"), 9);
        assert_eq!(precision_from_str("1e8"), 0);
        assert_eq!(precision_from_str("2e8"), 0);
    }
}
