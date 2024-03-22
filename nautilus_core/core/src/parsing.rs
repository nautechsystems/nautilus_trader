// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

/// Returns the decimal precision inferred from the given string.
#[must_use]
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

/// Returns a usize from the given bytes.
pub fn bytes_to_usize(bytes: &[u8]) -> anyhow::Result<usize> {
    // Check bytes width
    if bytes.len() >= std::mem::size_of::<usize>() {
        let mut buffer = [0u8; std::mem::size_of::<usize>()];
        buffer.copy_from_slice(&bytes[..std::mem::size_of::<usize>()]);

        Ok(usize::from_le_bytes(buffer))
    } else {
        Err(anyhow::anyhow!("Not enough bytes to represent a `usize`"))
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("", 0)]
    #[case("0", 0)]
    #[case("1.0", 1)]
    #[case("1.00", 2)]
    #[case("1.23456789", 8)]
    #[case("123456.789101112", 9)]
    #[case("0.000000001", 9)]
    #[case("1e-1", 1)]
    #[case("1e-2", 2)]
    #[case("1e-3", 3)]
    #[case("1e8", 0)]
    fn test_precision_from_str(#[case] s: &str, #[case] expected: u8) {
        let result = precision_from_str(s);
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_bytes_to_usize_empty() {
        let payload: Vec<u8> = vec![];
        let result = bytes_to_usize(&payload);
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "Not enough bytes to represent a `usize`"
        );
    }

    #[rstest]
    fn test_bytes_to_usize_invalid() {
        let payload: Vec<u8> = vec![0x01, 0x02, 0x03];
        let result = bytes_to_usize(&payload);
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "Not enough bytes to represent a `usize`"
        );
    }

    #[rstest]
    fn test_bytes_to_usize_valid() {
        let payload: Vec<u8> = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let result = bytes_to_usize(&payload).unwrap();
        assert_eq!(result, 0x0807_0605_0403_0201);
        assert_eq!(result, 578_437_695_752_307_201);
    }
}
