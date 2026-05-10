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

use thiserror::Error;

use super::ValueType;

pub(crate) type Result<T> = std::result::Result<T, ExpressionError>;

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub(crate) enum ExpressionError {
    #[error("Expression is empty")]
    EmptyExpression,
    #[error("Binding names cannot be empty")]
    EmptyBindingName,
    #[error("Unexpected character `{found}` at position {position}")]
    UnexpectedCharacter { found: char, position: usize },
    #[error("Unexpected token `{found}` at position {position}, expected {expected}")]
    UnexpectedToken {
        expected: &'static str,
        found: String,
        position: usize,
    },
    #[error("Unterminated block comment starting at position {position}")]
    UnterminatedBlockComment { position: usize },
    #[error("Missing closing `)` for `(` at position {position}")]
    MissingClosingParen { position: usize },
    #[error("Unknown symbol `{name}`")]
    UnknownSymbol { name: String },
    #[error("Unknown function `{name}`")]
    UnknownFunction { name: String },
    #[error(
        "Binding `{name}` is already assigned to input slot {existing_slot}, cannot reuse it for input slot {slot}"
    )]
    DuplicateBinding {
        name: String,
        existing_slot: usize,
        slot: usize,
    },
    #[error("Function `{name}` expected {expected} argument(s), found {actual}")]
    InvalidArgumentCount {
        name: &'static str,
        expected: String,
        actual: usize,
    },
    #[error("Expected {expected} in {context}, found {actual}")]
    TypeMismatch {
        context: &'static str,
        expected: ValueType,
        actual: ValueType,
    },
    #[error("Expected matching types in `{context}`, found {left} and {right}")]
    BinaryTypeMismatch {
        context: &'static str,
        left: ValueType,
        right: ValueType,
    },
    #[error("Input length mismatch: expected at least {expected}, found {actual}")]
    InputCountMismatch { expected: usize, actual: usize },
    #[error("Expression requires {depth} stack slots, maximum is {max}")]
    StackOverflow { depth: usize, max: usize },
    #[error("Expression defines {count} local variables, maximum is {max}")]
    TooManyLocals { count: usize, max: usize },
    #[error("Expression result is empty")]
    EmptyResult,
    #[error("Expression result must be numeric, found {actual}")]
    NonNumericResult { actual: ValueType },
}
