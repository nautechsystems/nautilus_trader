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

use super::{Bindings, Result, error::ExpressionError, is_ident_continue, is_ident_start};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Token {
    pub kind: TokenKind,
    pub position: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum TokenKind {
    Number(f64),
    Bool(bool),
    Ident(String),
    Binding(usize),
    LeftParen,
    RightParen,
    Comma,
    Semicolon,
    Assign,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    Bang,
    EqualEqual,
    BangEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    AndAnd,
    OrOr,
    Eof,
}

impl TokenKind {
    pub(super) fn description(&self) -> String {
        match self {
            Self::Number(number) => number.to_string(),
            Self::Bool(value) => value.to_string(),
            Self::Ident(name) => name.clone(),
            Self::Binding(slot) => format!("binding[{slot}]"),
            Self::LeftParen => "(".to_string(),
            Self::RightParen => ")".to_string(),
            Self::Comma => ",".to_string(),
            Self::Semicolon => ";".to_string(),
            Self::Assign => "=".to_string(),
            Self::Plus => "+".to_string(),
            Self::Minus => "-".to_string(),
            Self::Star => "*".to_string(),
            Self::Slash => "/".to_string(),
            Self::Percent => "%".to_string(),
            Self::Caret => "^".to_string(),
            Self::Bang => "!".to_string(),
            Self::EqualEqual => "==".to_string(),
            Self::BangEqual => "!=".to_string(),
            Self::Less => "<".to_string(),
            Self::LessEqual => "<=".to_string(),
            Self::Greater => ">".to_string(),
            Self::GreaterEqual => ">=".to_string(),
            Self::AndAnd => "&&".to_string(),
            Self::OrOr => "||".to_string(),
            Self::Eof => "end of input".to_string(),
        }
    }
}

pub(super) fn tokenize(source: &str, bindings: &Bindings) -> Result<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut position = 0;

    while position < source.len() {
        position = skip_whitespace_and_comments(source, position)?;
        if position >= source.len() {
            break;
        }

        let (kind, len) = next_token(source, position, bindings)?;
        tokens.push(Token { kind, position });
        position += len;
    }

    tokens.push(Token {
        kind: TokenKind::Eof,
        position: source.len(),
    });

    Ok(tokens)
}

fn next_token(source: &str, position: usize, bindings: &Bindings) -> Result<(TokenKind, usize)> {
    let ch = source[position..]
        .chars()
        .next()
        .ok_or(ExpressionError::EmptyExpression)?;

    let special = best_special_match(source, position, bindings);
    let number = scan_number(source, position);
    let ident = scan_ident(source, position);

    let best = [special, number, ident]
        .into_iter()
        .flatten()
        .max_by_key(|candidate| candidate.len);

    if let Some(candidate) = best {
        return Ok((candidate.kind, candidate.len));
    }

    match ch {
        '(' => Ok((TokenKind::LeftParen, 1)),
        ')' => Ok((TokenKind::RightParen, 1)),
        ',' => Ok((TokenKind::Comma, 1)),
        ';' => Ok((TokenKind::Semicolon, 1)),
        '+' => Ok((TokenKind::Plus, 1)),
        '-' => Ok((TokenKind::Minus, 1)),
        '*' => Ok((TokenKind::Star, 1)),
        '/' => Ok((TokenKind::Slash, 1)),
        '%' => Ok((TokenKind::Percent, 1)),
        '^' => Ok((TokenKind::Caret, 1)),
        '!' => {
            if source[position + 1..].starts_with('=') {
                Ok((TokenKind::BangEqual, 2))
            } else {
                Ok((TokenKind::Bang, 1))
            }
        }
        '=' => {
            if source[position + 1..].starts_with('=') {
                Ok((TokenKind::EqualEqual, 2))
            } else {
                Ok((TokenKind::Assign, 1))
            }
        }
        '<' => {
            if source[position + 1..].starts_with('=') {
                Ok((TokenKind::LessEqual, 2))
            } else {
                Ok((TokenKind::Less, 1))
            }
        }
        '>' => {
            if source[position + 1..].starts_with('=') {
                Ok((TokenKind::GreaterEqual, 2))
            } else {
                Ok((TokenKind::Greater, 1))
            }
        }
        '&' => {
            if source[position + 1..].starts_with('&') {
                Ok((TokenKind::AndAnd, 2))
            } else {
                Err(ExpressionError::UnexpectedToken {
                    expected: "`&&`",
                    found: "&".to_string(),
                    position,
                })
            }
        }
        '|' => {
            if source[position + 1..].starts_with('|') {
                Ok((TokenKind::OrOr, 2))
            } else {
                Err(ExpressionError::UnexpectedToken {
                    expected: "`||`",
                    found: "|".to_string(),
                    position,
                })
            }
        }
        _ => Err(ExpressionError::UnexpectedCharacter {
            found: ch,
            position,
        }),
    }
}

#[derive(Clone)]
struct Candidate {
    kind: TokenKind,
    len: usize,
}

fn best_special_match(source: &str, position: usize, bindings: &Bindings) -> Option<Candidate> {
    let first = source[position..].chars().next()?;
    let candidates = bindings.special_candidates(first)?;

    candidates.iter().find_map(|candidate| {
        let name = candidate.name.as_ref();
        if !source[position..].starts_with(name) {
            return None;
        }

        let len = name.len();
        if let Some(next) = source[position + len..].chars().next()
            && !is_binding_boundary(next)
        {
            return None;
        }

        Some(Candidate {
            kind: TokenKind::Binding(candidate.slot),
            len,
        })
    })
}

fn is_binding_boundary(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '(' | ')'
                | ','
                | ';'
                | '+'
                | '-'
                | '*'
                | '/'
                | '%'
                | '^'
                | '!'
                | '='
                | '<'
                | '>'
                | '&'
                | '|'
        )
}

fn scan_ident(source: &str, position: usize) -> Option<Candidate> {
    let first = source[position..].chars().next()?;
    if !is_ident_start(first) {
        return None;
    }

    let mut end = position + first.len_utf8();
    while let Some(ch) = source[end..].chars().next() {
        if !is_ident_continue(ch) {
            break;
        }
        end += ch.len_utf8();
    }

    let ident = &source[position..end];
    let kind = match ident {
        "true" => TokenKind::Bool(true),
        "false" => TokenKind::Bool(false),
        _ => TokenKind::Ident(ident.to_string()),
    };

    Some(Candidate {
        kind,
        len: end - position,
    })
}

fn scan_number(source: &str, position: usize) -> Option<Candidate> {
    let bytes = source.as_bytes();
    let mut index = position;
    let mut saw_digit = false;

    while index < source.len() && bytes[index].is_ascii_digit() {
        index += 1;
        saw_digit = true;
    }

    if index < source.len() && bytes[index] == b'.' {
        index += 1;
        while index < source.len() && bytes[index].is_ascii_digit() {
            index += 1;
            saw_digit = true;
        }
    }

    if !saw_digit {
        return None;
    }

    if index < source.len() && matches!(bytes[index], b'e' | b'E') {
        let exponent_start = index;
        index += 1;

        if index < source.len() && matches!(bytes[index], b'+' | b'-') {
            index += 1;
        }

        let exponent_digits_start = index;
        while index < source.len() && bytes[index].is_ascii_digit() {
            index += 1;
        }

        if exponent_digits_start == index {
            index = exponent_start;
        }
    }

    let literal = &source[position..index];
    let number = literal.parse::<f64>().ok()?;

    Some(Candidate {
        kind: TokenKind::Number(number),
        len: index - position,
    })
}

fn skip_whitespace_and_comments(source: &str, mut position: usize) -> Result<usize> {
    while position < source.len() {
        let remaining = &source[position..];

        if remaining.starts_with("//") {
            position += 2;
            while position < source.len() {
                let ch = source[position..]
                    .chars()
                    .next()
                    .ok_or(ExpressionError::EmptyExpression)?;
                position += ch.len_utf8();
                if ch == '\n' {
                    break;
                }
            }
            continue;
        }

        if remaining.starts_with("/*") {
            let start = position;
            position += 2;
            let Some(offset) = source[position..].find("*/") else {
                return Err(ExpressionError::UnterminatedBlockComment { position: start });
            };
            position += offset + 2;
            continue;
        }

        let Some(ch) = remaining.chars().next() else {
            break;
        };

        if !ch.is_whitespace() {
            break;
        }
        position += ch.len_utf8();
    }

    Ok(position)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::expressions::Bindings;

    fn tokenize_kinds(source: &str, bindings: &Bindings) -> Vec<TokenKind> {
        tokenize(source, bindings)
            .unwrap()
            .into_iter()
            .map(|token| token.kind)
            .collect()
    }

    #[rstest]
    fn test_tokenize_special_bindings_and_comments() {
        let mut bindings = Bindings::new();
        bindings.add(0, "AUD/USD.SIM").unwrap();
        bindings.add(1, "BTCUSDT.BINANCE").unwrap();

        let tokens = tokenize_kinds(
            "AUD/USD.SIM + BTCUSDT.BINANCE // trailing comment\n/* block */ - 1.5",
            &bindings,
        );

        assert_eq!(
            tokens,
            vec![
                TokenKind::Binding(0),
                TokenKind::Plus,
                TokenKind::Binding(1),
                TokenKind::Minus,
                TokenKind::Number(1.5),
                TokenKind::Eof,
            ]
        );
    }

    #[rstest]
    fn test_tokenize_prefers_longest_special_binding_match() {
        let mut bindings = Bindings::new();
        bindings.add(0, "ETH").unwrap();
        bindings.add(1, "ETH-USDT-SWAP.OKX").unwrap();

        let tokens = tokenize_kinds("ETH-USDT-SWAP.OKX - 1", &bindings);

        assert_eq!(
            tokens,
            vec![
                TokenKind::Binding(1),
                TokenKind::Minus,
                TokenKind::Number(1.0),
                TokenKind::Eof,
            ]
        );
    }

    #[rstest]
    fn test_tokenize_rejects_partial_special_binding_matches() {
        let mut bindings = Bindings::new();
        bindings.add(0, "AUD/USD").unwrap();

        let error = tokenize("AUD/USD.SIM + 1", &bindings).unwrap_err();

        assert_eq!(
            error,
            ExpressionError::UnexpectedCharacter {
                found: '.',
                position: 7,
            }
        );
    }

    #[rstest]
    fn test_tokenize_rejects_single_ampersand_and_pipe() {
        let bindings = Bindings::new();

        let ampersand_error = tokenize("true & false", &bindings).unwrap_err();
        let pipe_error = tokenize("true | false", &bindings).unwrap_err();

        assert_eq!(
            ampersand_error,
            ExpressionError::UnexpectedToken {
                expected: "`&&`",
                found: "&".to_string(),
                position: 5,
            }
        );
        assert_eq!(
            pipe_error,
            ExpressionError::UnexpectedToken {
                expected: "`||`",
                found: "|".to_string(),
                position: 5,
            }
        );
    }

    #[rstest]
    fn test_tokenize_plain_identifiers_bool_and_scientific_notation() {
        let bindings = Bindings::new();
        let tokens = tokenize_kinds("flag && true || foo_1 + 1.2e-3", &bindings);

        assert_eq!(
            tokens,
            vec![
                TokenKind::Ident("flag".to_string()),
                TokenKind::AndAnd,
                TokenKind::Bool(true),
                TokenKind::OrOr,
                TokenKind::Ident("foo_1".to_string()),
                TokenKind::Plus,
                TokenKind::Number(0.0012),
                TokenKind::Eof,
            ]
        );
    }

    #[rstest]
    fn test_tokenize_rejects_unterminated_block_comment() {
        let bindings = Bindings::new();
        let error = tokenize("1 /* missing", &bindings).unwrap_err();

        assert_eq!(
            error,
            ExpressionError::UnterminatedBlockComment { position: 2 }
        );
    }

    #[rstest]
    fn test_token_kind_description_for_end_of_input() {
        assert_eq!(TokenKind::Eof.description(), "end of input");
    }
}
