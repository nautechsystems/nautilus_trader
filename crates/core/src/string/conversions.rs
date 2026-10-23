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

/// Titlecases `s`, treating each maximal run of cased characters as a word.
///
/// Follows Python's `str.title()` word-boundary rules: the first character of each word uses its
/// Unicode titlecase mapping, and the rest are lowercased. Unlike Python, a word-final `Σ`
/// lowercases to `σ`, not `ς`.
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
    if s.is_ascii() {
        title_case_ascii(s.as_bytes())
    } else {
        title_case_unicode(s)
    }
}

fn title_case_ascii(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut prev_cased = false;
    for &byte in bytes {
        let byte = if prev_cased {
            byte.to_ascii_lowercase()
        } else {
            byte.to_ascii_uppercase()
        };

        out.push(char::from(byte));
        prev_cased = byte.is_ascii_alphabetic();
    }

    out
}

fn title_case_unicode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_cased = false;
    for ch in s.chars() {
        if prev_cased {
            out.extend(ch.to_lowercase());
        } else {
            push_titlecase(&mut out, ch);
        }

        prev_cased = is_cased(ch);
    }

    out
}

fn push_titlecase(out: &mut String, ch: char) {
    // Stable Rust does not expose `char::to_titlecase`. These Unicode 17 mappings from
    // UnicodeData.txt and SpecialCasing.txt cannot be reproduced by keeping the first scalar of
    // the uppercase mapping and lowercasing the rest.
    match ch {
        '\u{149}' => out.push_str("\u{2BC}N"),
        '\u{1C4}'..='\u{1C6}' => out.push('\u{1C5}'),
        '\u{1C7}'..='\u{1C9}' => out.push('\u{1C8}'),
        '\u{1CA}'..='\u{1CC}' => out.push('\u{1CB}'),
        '\u{1F1}'..='\u{1F3}' => out.push('\u{1F2}'),
        '\u{10D0}'..='\u{10FA}'
        | '\u{10FD}'..='\u{10FF}'
        | '\u{1F88}'..='\u{1F8F}'
        | '\u{1F98}'..='\u{1F9F}'
        | '\u{1FA8}'..='\u{1FAF}' => out.push(ch),
        '\u{1F80}'..='\u{1F87}' | '\u{1F90}'..='\u{1F97}' | '\u{1FA0}'..='\u{1FA7}' => {
            out.push(
                char::from_u32(ch as u32 + 8).expect("titlecase mapping must be valid Unicode"),
            );
        }
        '\u{1FB2}' => out.push_str("\u{1FBA}\u{345}"),
        '\u{1FB3}' | '\u{1FBC}' => out.push('\u{1FBC}'),
        '\u{1FB4}' => out.push_str("\u{386}\u{345}"),
        '\u{1FB7}' => out.push_str("\u{391}\u{342}\u{345}"),
        '\u{1FC2}' => out.push_str("\u{1FCA}\u{345}"),
        '\u{1FC3}' | '\u{1FCC}' => out.push('\u{1FCC}'),
        '\u{1FC4}' => out.push_str("\u{389}\u{345}"),
        '\u{1FC7}' => out.push_str("\u{397}\u{342}\u{345}"),
        '\u{1FF2}' => out.push_str("\u{1FFA}\u{345}"),
        '\u{1FF3}' | '\u{1FFC}' => out.push('\u{1FFC}'),
        '\u{1FF4}' => out.push_str("\u{38F}\u{345}"),
        '\u{1FF7}' => out.push_str("\u{3A9}\u{342}\u{345}"),
        _ => {
            let mut uppercase = ch.to_uppercase();
            if let Some(first) = uppercase.next() {
                out.push(first);
            }

            for ch in uppercase {
                out.extend(ch.to_lowercase());
            }
        }
    }
}

fn is_cased(ch: char) -> bool {
    ch.is_lowercase() || ch.is_uppercase() || ch.to_lowercase().next() != Some(ch)
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
    #[case("é--x", "é_x")]
    #[case("nautilus_model::data::bar::Bar", "nautilus_model_data_bar_bar")] // nautilus-import-ok
    fn test_to_snake_case(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(to_snake_case(input), expected);
    }

    #[rstest]
    #[case("", "")]
    #[case("a", "A")]
    #[case("example", "Example")]
    #[case("EXAMPLE", "Example")]
    #[case("ΟΣ", "Οσ")]
    #[case("hello_world", "Hello_World")]
    #[case("hello-world", "Hello-World")]
    #[case("hello world", "Hello World")]
    #[case("hELLO wORLD", "Hello World")]
    #[case("123abc", "123Abc")]
    #[case("_leading", "_Leading")]
    #[case("\u{1C6}", "\u{1C5}")]
    #[case("\u{1C9}", "\u{1C8}")]
    #[case("\u{1CC}", "\u{1CB}")]
    #[case("ǳ", "ǲ")]
    #[case("ﬃ", "Ffi")]
    #[case("\u{149}", "\u{2BC}N")]
    #[case("\u{10D0}", "\u{10D0}")]
    #[case("\u{1F80}", "\u{1F88}")]
    #[case("\u{1FB2}", "\u{1FBA}\u{345}")]
    #[case("\u{1FB3}", "\u{1FBC}")]
    #[case("\u{1FB4}", "\u{386}\u{345}")]
    #[case("\u{1FB7}", "\u{391}\u{342}\u{345}")]
    #[case("\u{1FC2}", "\u{1FCA}\u{345}")]
    #[case("\u{1FC3}", "\u{1FCC}")]
    #[case("\u{1FC4}", "\u{389}\u{345}")]
    #[case("\u{1FC7}", "\u{397}\u{342}\u{345}")]
    #[case("\u{1FF2}", "\u{1FFA}\u{345}")]
    #[case("\u{1FF3}", "\u{1FFC}")]
    #[case("\u{1FF4}", "\u{38F}\u{345}")]
    #[case("\u{1FF7}", "\u{3A9}\u{342}\u{345}")]
    #[case("\u{4E2D}a", "\u{4E2D}A")]
    #[case("ªa", "ªa")]
    #[case("ǲa", "ǲa")]
    fn test_title_case(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(title_case(input), expected);
    }

    #[rstest]
    fn test_title_case_unicode_version() {
        assert_eq!(char::UNICODE_VERSION, (17, 0, 0));
    }
}
