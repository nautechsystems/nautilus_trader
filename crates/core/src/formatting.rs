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

//! Number formatting utilities.

fn separate_with(s: &str, sep: char) -> String {
    let (neg, digits) = if let Some(rest) = s.strip_prefix('-') {
        (true, rest)
    } else {
        (false, s)
    };

    let (int_part, dec_part) = match digits.find('.') {
        Some(pos) => (&digits[..pos], Some(&digits[pos..])),
        None => (digits, None),
    };

    let mut result = String::with_capacity(s.len() + int_part.len() / 3);

    if neg {
        result.push('-');
    }

    let chars: Vec<char> = int_part.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i).is_multiple_of(3) {
            result.push(sep);
        }
        result.push(*c);
    }

    if let Some(dec) = dec_part {
        result.push_str(dec);
    }

    result
}

/// Extension trait for formatting numbers with separators.
///
/// Drop-in replacement for the `thousands::Separable` trait.
pub trait Separable {
    /// Formats the number with commas as thousand separators.
    fn separate_with_commas(&self) -> String;

    /// Formats the number with underscores as thousand separators.
    fn separate_with_underscores(&self) -> String;
}

macro_rules! impl_separable {
    ($($t:ty),*) => {
        $(
            impl Separable for $t {
                fn separate_with_commas(&self) -> String {
                    separate_with(&self.to_string(), ',')
                }

                fn separate_with_underscores(&self) -> String {
                    separate_with(&self.to_string(), '_')
                }
            }
        )*
    };
}

impl_separable!(
    i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize, f32, f64
);

impl Separable for String {
    fn separate_with_commas(&self) -> String {
        separate_with(self, ',')
    }

    fn separate_with_underscores(&self) -> String {
        separate_with(self, '_')
    }
}

impl Separable for &str {
    fn separate_with_commas(&self) -> String {
        separate_with(self, ',')
    }

    fn separate_with_underscores(&self) -> String {
        separate_with(self, '_')
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(0, "0")]
    #[case(1, "1")]
    #[case(12, "12")]
    #[case(123, "123")]
    #[case(1234, "1,234")]
    #[case(12345, "12,345")]
    #[case(123456, "123,456")]
    #[case(1234567, "1,234,567")]
    #[case(-1234, "-1,234")]
    #[case(-1234567, "-1,234,567")]
    fn test_separate_with_commas(#[case] input: i64, #[case] expected: &str) {
        assert_eq!(input.separate_with_commas(), expected);
    }

    #[rstest]
    #[case(1234, "1_234")]
    #[case(1234567, "1_234_567")]
    fn test_separate_with_underscores(#[case] input: i64, #[case] expected: &str) {
        assert_eq!(input.separate_with_underscores(), expected);
    }

    #[rstest]
    fn test_float_with_decimal() {
        assert_eq!(1234.56_f64.separate_with_commas(), "1,234.56");
        assert_eq!(1234567.89_f64.separate_with_underscores(), "1_234_567.89");
    }

    #[rstest]
    fn test_string() {
        assert_eq!("1234567".separate_with_commas(), "1,234,567");
        assert_eq!("1234.5678".separate_with_underscores(), "1_234.5678");
    }
}
