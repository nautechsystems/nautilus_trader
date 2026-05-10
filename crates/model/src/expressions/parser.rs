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

use super::{
    Result,
    error::ExpressionError,
    lexer::{Token, TokenKind},
};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Program {
    pub statements: Vec<Statement>,
    pub trailing_semicolon: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Statement {
    Assign { name: String, expr: Expr },
    Expr(Expr),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Expr {
    Number(f64),
    Bool(bool),
    Name(String),
    Input(usize),
    Unary {
        op: UnaryOp,
        expr: Box<Self>,
    },
    Binary {
        left: Box<Self>,
        op: BinaryOp,
        right: Box<Self>,
    },
    Call {
        name: String,
        args: Vec<Self>,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum UnaryOp {
    Neg,
    Not,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BinaryOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Eq,
    Neq,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

pub(super) fn parse(tokens: &[Token]) -> Result<Program> {
    Parser::new(tokens).parse_program()
}

struct Parser<'a> {
    tokens: &'a [Token],
    index: usize,
}

impl<'a> Parser<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, index: 0 }
    }

    fn parse_program(&mut self) -> Result<Program> {
        if matches!(self.current().kind, TokenKind::Eof) {
            return Err(ExpressionError::EmptyExpression);
        }

        let mut statements = Vec::new();
        let mut trailing_semicolon = false;

        loop {
            statements.push(self.parse_statement()?);

            if !matches!(self.current().kind, TokenKind::Semicolon) {
                break;
            }

            trailing_semicolon = true;
            self.advance();

            if matches!(self.current().kind, TokenKind::Eof) {
                break;
            }

            trailing_semicolon = false;
        }

        self.expect_eof()?;

        Ok(Program {
            statements,
            trailing_semicolon,
        })
    }

    fn parse_statement(&mut self) -> Result<Statement> {
        if let TokenKind::Ident(name) = &self.current().kind
            && matches!(self.peek().kind, TokenKind::Assign)
        {
            let name = name.clone();
            self.advance();
            self.advance();

            let expr = self.parse_expression(0)?;
            return Ok(Statement::Assign { name, expr });
        }

        Ok(Statement::Expr(self.parse_expression(0)?))
    }

    fn parse_expression(&mut self, min_precedence: u8) -> Result<Expr> {
        let mut expr = self.parse_prefix()?;

        while let Some((op, precedence, right_associative)) = self.current_binary_op() {
            if precedence < min_precedence {
                break;
            }

            self.advance();
            let next_min_precedence = if right_associative {
                precedence
            } else {
                precedence + 1
            };
            let right = self.parse_expression(next_min_precedence)?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }

        Ok(expr)
    }

    fn parse_prefix(&mut self) -> Result<Expr> {
        let token = self.current().clone();
        match token.kind {
            TokenKind::Number(value) => {
                self.advance();
                Ok(Expr::Number(value))
            }
            TokenKind::Bool(value) => {
                self.advance();
                Ok(Expr::Bool(value))
            }
            TokenKind::Binding(slot) => {
                self.advance();
                Ok(Expr::Input(slot))
            }
            TokenKind::Ident(name) => {
                self.advance();

                if matches!(self.current().kind, TokenKind::LeftParen) {
                    self.advance();
                    let args = self.parse_call_args()?;
                    Ok(Expr::Call { name, args })
                } else {
                    Ok(Expr::Name(name))
                }
            }
            TokenKind::LeftParen => {
                self.advance();
                let expr = self.parse_expression(0)?;
                self.expect_right_paren(token.position)?;
                Ok(expr)
            }
            TokenKind::Minus => {
                self.advance();
                Ok(Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(self.parse_expression(7)?),
                })
            }
            TokenKind::Bang => {
                self.advance();
                Ok(Expr::Unary {
                    op: UnaryOp::Not,
                    expr: Box::new(self.parse_expression(7)?),
                })
            }
            _ => Err(ExpressionError::UnexpectedToken {
                expected: "an expression",
                found: token.kind.description(),
                position: token.position,
            }),
        }
    }

    fn parse_call_args(&mut self) -> Result<Vec<Expr>> {
        let mut args = Vec::new();

        if matches!(self.current().kind, TokenKind::RightParen) {
            self.advance();
            return Ok(args);
        }

        loop {
            args.push(self.parse_expression(0)?);

            match self.current().kind {
                TokenKind::Comma => {
                    self.advance();
                }
                TokenKind::RightParen => {
                    self.advance();
                    return Ok(args);
                }
                _ => {
                    let token = self.current();
                    return Err(ExpressionError::UnexpectedToken {
                        expected: "`,` or `)`",
                        found: token.kind.description(),
                        position: token.position,
                    });
                }
            }
        }
    }

    fn current_binary_op(&self) -> Option<(BinaryOp, u8, bool)> {
        match self.current().kind {
            TokenKind::OrOr => Some((BinaryOp::Or, 1, false)),
            TokenKind::AndAnd => Some((BinaryOp::And, 2, false)),
            TokenKind::EqualEqual => Some((BinaryOp::Eq, 3, false)),
            TokenKind::BangEqual => Some((BinaryOp::Neq, 3, false)),
            TokenKind::Less => Some((BinaryOp::Lt, 4, false)),
            TokenKind::LessEqual => Some((BinaryOp::Le, 4, false)),
            TokenKind::Greater => Some((BinaryOp::Gt, 4, false)),
            TokenKind::GreaterEqual => Some((BinaryOp::Ge, 4, false)),
            TokenKind::Plus => Some((BinaryOp::Add, 5, false)),
            TokenKind::Minus => Some((BinaryOp::Sub, 5, false)),
            TokenKind::Star => Some((BinaryOp::Mul, 6, false)),
            TokenKind::Slash => Some((BinaryOp::Div, 6, false)),
            TokenKind::Percent => Some((BinaryOp::Mod, 6, false)),
            TokenKind::Caret => Some((BinaryOp::Pow, 7, true)),
            _ => None,
        }
    }

    fn expect_right_paren(&mut self, position: usize) -> Result<()> {
        if !matches!(self.current().kind, TokenKind::RightParen) {
            return Err(ExpressionError::MissingClosingParen { position });
        }

        self.advance();
        Ok(())
    }

    fn expect_eof(&self) -> Result<()> {
        if matches!(self.current().kind, TokenKind::Eof) {
            Ok(())
        } else {
            let token = self.current();
            Err(ExpressionError::UnexpectedToken {
                expected: "end of input",
                found: token.kind.description(),
                position: token.position,
            })
        }
    }

    fn current(&self) -> &'a Token {
        &self.tokens[self.index]
    }

    fn peek(&self) -> &'a Token {
        self.tokens
            .get(self.index + 1)
            .unwrap_or_else(|| self.tokens.last().expect("tokens always end with EOF"))
    }

    fn advance(&mut self) {
        if self.index + 1 < self.tokens.len() {
            self.index += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::expressions::{Bindings, lexer::tokenize};

    fn parse_program(source: &str, bindings: &Bindings) -> Program {
        let tokens = tokenize(source, bindings).unwrap();
        parse(&tokens).unwrap()
    }

    #[rstest]
    fn test_parse_operator_precedence_and_right_associative_power() {
        let bindings = Bindings::new();
        let program = parse_program("1 + 2 * 3 ^ 2", &bindings);

        assert_eq!(
            program,
            Program {
                statements: vec![Statement::Expr(Expr::Binary {
                    left: Box::new(Expr::Number(1.0)),
                    op: BinaryOp::Add,
                    right: Box::new(Expr::Binary {
                        left: Box::new(Expr::Number(2.0)),
                        op: BinaryOp::Mul,
                        right: Box::new(Expr::Binary {
                            left: Box::new(Expr::Number(3.0)),
                            op: BinaryOp::Pow,
                            right: Box::new(Expr::Number(2.0)),
                        }),
                    }),
                })],
                trailing_semicolon: false,
            }
        );
    }

    #[rstest]
    fn test_parse_unary_minus_after_power() {
        let bindings = Bindings::new();
        let program = parse_program("-2 ^ 2", &bindings);

        assert_eq!(
            program,
            Program {
                statements: vec![Statement::Expr(Expr::Unary {
                    op: UnaryOp::Neg,
                    expr: Box::new(Expr::Binary {
                        left: Box::new(Expr::Number(2.0)),
                        op: BinaryOp::Pow,
                        right: Box::new(Expr::Number(2.0)),
                    }),
                })],
                trailing_semicolon: false,
            }
        );
    }

    #[rstest]
    fn test_parse_assignments_function_calls_and_sequences() {
        let bindings = Bindings::new();
        let program = parse_program("spread = max(1, 2); spread + 3", &bindings);

        assert_eq!(
            program,
            Program {
                statements: vec![
                    Statement::Assign {
                        name: "spread".to_string(),
                        expr: Expr::Call {
                            name: "max".to_string(),
                            args: vec![Expr::Number(1.0), Expr::Number(2.0)],
                        },
                    },
                    Statement::Expr(Expr::Binary {
                        left: Box::new(Expr::Name("spread".to_string())),
                        op: BinaryOp::Add,
                        right: Box::new(Expr::Number(3.0)),
                    }),
                ],
                trailing_semicolon: false,
            }
        );
    }

    #[rstest]
    fn test_parse_supports_special_bindings() {
        let mut bindings = Bindings::new();
        bindings.add(0, "AUD/USD.SIM").unwrap();

        let program = parse_program("AUD/USD.SIM > 1", &bindings);

        assert_eq!(
            program,
            Program {
                statements: vec![Statement::Expr(Expr::Binary {
                    left: Box::new(Expr::Input(0)),
                    op: BinaryOp::Gt,
                    right: Box::new(Expr::Number(1.0)),
                })],
                trailing_semicolon: false,
            }
        );
    }

    #[rstest]
    fn test_parse_rejects_missing_closing_paren() {
        let bindings = Bindings::new();
        let tokens = tokenize("(1 + 2", &bindings).unwrap();
        let error = parse(&tokens).unwrap_err();

        assert_eq!(error, ExpressionError::MissingClosingParen { position: 0 });
    }
}
