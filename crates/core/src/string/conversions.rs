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

//! String case conversions (`snake_case`, Title Case).

/// Converts a string from any common case to `snake_case`.
///
/// Word boundaries are detected at:
/// - Non-alphanumeric characters (spaces, hyphens, underscores, colons, etc.)
/// - Transitions from lowercase or digit to uppercase (`camelCase` -> `camel_case`)
/// - Within consecutive uppercase letters, before the last if followed by lowercase
///   (`XMLParser` -> `xml_parser`)
#[must_use]
pub fn to_snake_case(s: &str) -> String {
    if s.is_ascii() {
        to_snake_case_ascii(s.as_bytes())
    } else {
        to_snake_case_unicode(s)
    }
}

fn to_snake_case_ascii(bytes: &[u8]) -> String {
    // Single pass over bytes. Mode tracks the case of the last cased character
    // within the current alphanumeric run, matching heck's word-boundary rules.
    const BOUNDARY: u8 = 0;
    const LOWER: u8 = 1;
    const UPPER: u8 = 2;

    let len = bytes.len();
    let mut result = String::with_capacity(len + len / 4);
    let mut first_word = true;
    let mut mode: u8 = BOUNDARY;
    let mut word_start = 0;
    let mut i = 0;

    while i < len {
        let b = bytes[i];

        if !b.is_ascii_alphanumeric() {
            if word_start < i {
                push_lower_ascii(&mut result, &bytes[word_start..i], &mut first_word);
            }
            word_start = i + 1;
            mode = BOUNDARY;
            i += 1;
            continue;
        }

        let next_mode = if b.is_ascii_lowercase() {
            LOWER
        } else if b.is_ascii_uppercase() {
            UPPER
        } else {
            mode
        };

        if i + 1 < len && bytes[i + 1].is_ascii_alphanumeric() {
            let next = bytes[i + 1];

            if next_mode == LOWER && next.is_ascii_uppercase() {
                push_lower_ascii(&mut result, &bytes[word_start..=i], &mut first_word);
                word_start = i + 1;
                mode = BOUNDARY;
            } else if mode == UPPER && b.is_ascii_uppercase() && next.is_ascii_lowercase() {
                if word_start < i {
                    push_lower_ascii(&mut result, &bytes[word_start..i], &mut first_word);
                }
                word_start = i;
                mode = BOUNDARY;
            } else {
                mode = next_mode;
            }
        }

        i += 1;
    }

    if word_start < len && bytes[word_start].is_ascii_alphanumeric() {
        push_lower_ascii(&mut result, &bytes[word_start..], &mut first_word);
    }

    result
}

fn push_lower_ascii(result: &mut String, word: &[u8], first_word: &mut bool) {
    if word.is_empty() {
        *first_word = false;
        return;
    }

    if !*first_word {
        result.push('_');
    }
    *first_word = false;

    for &b in word {
        result.push(char::from(b.to_ascii_lowercase()));
    }
}

fn to_snake_case_unicode(s: &str) -> String {
    #[derive(Clone, Copy, PartialEq)]
    enum Mode {
        Boundary,
        Lowercase,
        Uppercase,
    }

    let mut result = String::with_capacity(s.len() + s.len() / 4);
    let mut first_word = true;

    for word in s.split(|c: char| !c.is_alphanumeric()) {
        let mut char_indices = word.char_indices().peekable();
        let mut init = 0;
        let mut mode = Mode::Boundary;

        while let Some((i, c)) = char_indices.next() {
            if let Some(&(next_i, next)) = char_indices.peek() {
                let next_mode = if c.is_lowercase() {
                    Mode::Lowercase
                } else if c.is_uppercase() {
                    Mode::Uppercase
                } else {
                    mode
                };

                if next_mode == Mode::Lowercase && next.is_uppercase() {
                    push_lower_unicode(&mut result, &word[init..next_i], &mut first_word);
                    init = next_i;
                    mode = Mode::Boundary;
                } else if mode == Mode::Uppercase && c.is_uppercase() && next.is_lowercase() {
                    push_lower_unicode(&mut result, &word[init..i], &mut first_word);
                    init = i;
                    mode = Mode::Boundary;
                } else {
                    mode = next_mode;
                }
            } else {
                push_lower_unicode(&mut result, &word[init..], &mut first_word);
                break;
            }
        }
    }

    result
}

fn push_lower_unicode(result: &mut String, word: &str, first_word: &mut bool) {
    if word.is_empty() {
        *first_word = false;
        return;
    }

    if !*first_word {
        result.push('_');
    }
    *first_word = false;

    for c in word.chars() {
        for lc in c.to_lowercase() {
            result.push(lc);
        }
    }
}

/// Title-cases `s` by capitalizing the first letter of each alphabetic run.
///
/// Mirrors Python's `str.title()`: word boundaries fall at any non-alphabetic
/// character, the first letter of each run is uppercased, and the rest are
/// lowercased.
///
/// # Examples
///
/// ```
/// use nautilus_core::string::conversions::title_case;
///
/// assert_eq!(title_case("example"), "Example");
/// assert_eq!(title_case("hello_world"), "Hello_World");
/// assert_eq!(title_case("hello world"), "Hello World");
/// assert_eq!(title_case(""), "");
/// ```
#[must_use]
pub fn title_case(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_alpha = false;

    for ch in s.chars() {
        if ch.is_alphabetic() {
            if prev_alpha {
                out.extend(ch.to_lowercase());
            } else {
                out.extend(ch.to_uppercase());
            }
            prev_alpha = true;
        } else {
            out.push(ch);
            prev_alpha = false;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("CamelCase", "camel_case")]
    #[case("This is Human case.", "this_is_human_case")]
    #[case(
        "MixedUP CamelCase, with some Spaces",
        "mixed_up_camel_case_with_some_spaces"
    )]
    #[case(
        "mixed_up_ snake_case with some _spaces",
        "mixed_up_snake_case_with_some_spaces"
    )]
    #[case("kebab-case", "kebab_case")]
    #[case("SHOUTY_SNAKE_CASE", "shouty_snake_case")]
    #[case("snake_case", "snake_case")]
    #[case("XMLHttpRequest", "xml_http_request")]
    #[case("FIELD_NAME11", "field_name11")]
    #[case("99BOTTLES", "99bottles")]
    #[case("abc123def456", "abc123def456")]
    #[case("abc123DEF456", "abc123_def456")]
    #[case("abc123Def456", "abc123_def456")]
    #[case("abc123DEf456", "abc123_d_ef456")]
    #[case("ABC123def456", "abc123def456")]
    #[case("ABC123DEF456", "abc123def456")]
    #[case("ABC123Def456", "abc123_def456")]
    #[case("ABC123DEf456", "abc123d_ef456")]
    #[case("ABC123dEEf456FOO", "abc123d_e_ef456_foo")]
    #[case("abcDEF", "abc_def")]
    #[case("ABcDE", "a_bc_de")]
    #[case("", "")]
    #[case("A", "a")]
    #[case("AB", "ab")]
    #[case("PascalCase", "pascal_case")]
    #[case("camelCase", "camel_case")]
    #[case("getHTTPResponse", "get_http_response")]
    #[case("Level1", "level1")]
    #[case("OrderBookDelta", "order_book_delta")]
    #[case("IOError", "io_error")]
    #[case("SimpleHTTPServer", "simple_http_server")]
    #[case("version2Release", "version2_release")]
    #[case("ALLCAPS", "allcaps")]
    #[case("nautilus_model::data::bar::Bar", "nautilus_model_data_bar_bar")] // nautilus-import-ok
    fn test_to_snake_case(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(to_snake_case(input), expected);
    }

    #[rstest]
    #[case("", "")]
    #[case("a", "A")]
    #[case("example", "Example")]
    #[case("EXAMPLE", "Example")]
    #[case("hello_world", "Hello_World")]
    #[case("hello-world", "Hello-World")]
    #[case("hello world", "Hello World")]
    #[case("hELLO wORLD", "Hello World")]
    #[case("123abc", "123Abc")]
    #[case("_leading", "_Leading")]
    fn test_title_case(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(title_case(input), expected);
    }
}
