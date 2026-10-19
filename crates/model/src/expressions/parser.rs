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

/// Maximum syntax nesting and constructed AST depth accepted by the parser,
/// counting the top-level expression as one level.
///
/// Bounds both the parser's own recursion (parentheses, prefix operators,
/// right-associative `^`, call arguments) and the depth of the tree it
/// constructs (a left-associated operator chain builds an O(n)-deep tree with
/// O(1) parser recursion), so parsing, `compile_expr`, and drop glue all stay
/// within a fixed stack budget. On a 2 MiB thread in debug builds the walks
/// overflow at depths of roughly 300 (nested parentheses and prefix chains),
/// 400 (compiling and dropping a left-associated chain), and 1200 (a
/// right-associative chain); release builds clear depth 1024 on every walk.
/// 128 keeps better than a 2x margin below the lowest of those cliffs. The
/// documented eight-component weighted sum has depth 9.
const MAX_EXPRESSION_DEPTH: usize = 128;

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

struct ParsedExpr {
    expr: Expr,
    depth: usize,
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

            let expr = self.parse_expression(0, 1)?.expr;
            return Ok(Statement::Assign { name, expr });
        }

        Ok(Statement::Expr(self.parse_expression(0, 1)?.expr))
    }

    fn parse_expression(&mut self, min_precedence: u8, syntax_depth: usize) -> Result<ParsedExpr> {
        let mut parsed = self.parse_prefix(syntax_depth)?;

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
            let child_syntax_depth = Self::next_syntax_depth(syntax_depth)?;
            let right = self.parse_expression(next_min_precedence, child_syntax_depth)?;
            let depth = Self::parent_depth(parsed.depth.max(right.depth))?;
            parsed = ParsedExpr {
                expr: Expr::Binary {
                    left: Box::new(parsed.expr),
                    op,
                    right: Box::new(right.expr),
                },
                depth,
            };
        }

        Ok(parsed)
    }

    fn parse_prefix(&mut self, syntax_depth: usize) -> Result<ParsedExpr> {
        let token = self.current().clone();
        match token.kind {
            TokenKind::Number(value) => {
                self.advance();
                Ok(ParsedExpr {
                    expr: Expr::Number(value),
                    depth: 1,
                })
            }
            TokenKind::Bool(value) => {
                self.advance();
                Ok(ParsedExpr {
                    expr: Expr::Bool(value),
                    depth: 1,
                })
            }
            TokenKind::Binding(slot) => {
                self.advance();
                Ok(ParsedExpr {
                    expr: Expr::Input(slot),
                    depth: 1,
                })
            }
            TokenKind::Ident(name) => {
                self.advance();

                if matches!(self.current().kind, TokenKind::LeftParen) {
                    self.advance();
                    let child_depth = Self::next_syntax_depth(syntax_depth)?;
                    let args = self.parse_call_args(child_depth)?;
                    let max_arg_depth = args.iter().map(|arg| arg.depth).max().unwrap_or(0);
                    let depth = Self::parent_depth(max_arg_depth)?;
                    Ok(ParsedExpr {
                        expr: Expr::Call {
                            name,
                            args: args.into_iter().map(|arg| arg.expr).collect(),
                        },
                        depth,
                    })
                } else {
                    Ok(ParsedExpr {
                        expr: Expr::Name(name),
                        depth: 1,
                    })
                }
            }
            TokenKind::LeftParen => {
                self.advance();
                let depth = Self::next_syntax_depth(syntax_depth)?;
                let expr = self.parse_expression(0, depth)?;
                self.expect_right_paren(token.position)?;
                Ok(expr)
            }
            TokenKind::Minus => {
                self.advance();
                let syntax_depth = Self::next_syntax_depth(syntax_depth)?;
                let child = self.parse_expression(7, syntax_depth)?;
                let depth = Self::parent_depth(child.depth)?;
                Ok(ParsedExpr {
                    expr: Expr::Unary {
                        op: UnaryOp::Neg,
                        expr: Box::new(child.expr),
                    },
                    depth,
                })
            }
            TokenKind::Bang => {
                self.advance();
                let syntax_depth = Self::next_syntax_depth(syntax_depth)?;
                let child = self.parse_expression(7, syntax_depth)?;
                let depth = Self::parent_depth(child.depth)?;
                Ok(ParsedExpr {
                    expr: Expr::Unary {
                        op: UnaryOp::Not,
                        expr: Box::new(child.expr),
                    },
                    depth,
                })
            }
            _ => Err(ExpressionError::UnexpectedToken {
                expected: "an expression",
                found: token.kind.description(),
                position: token.position,
            }),
        }
    }

    fn parse_call_args(&mut self, syntax_depth: usize) -> Result<Vec<ParsedExpr>> {
        let mut args = Vec::new();

        if matches!(self.current().kind, TokenKind::RightParen) {
            self.advance();
            return Ok(args);
        }

        loop {
            args.push(self.parse_expression(0, syntax_depth)?);

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

    fn next_syntax_depth(depth: usize) -> Result<usize> {
        Self::parent_depth(depth)
    }

    fn parent_depth(child_depth: usize) -> Result<usize> {
        let depth = child_depth + 1;
        if depth > MAX_EXPRESSION_DEPTH {
            Err(ExpressionError::ExpressionDepthExceeded {
                depth,
                max: MAX_EXPRESSION_DEPTH,
            })
        } else {
            Ok(depth)
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
    use std::thread;

    use rstest::rstest;

    use super::*;
    use crate::expressions::{Bindings, eval, lexer::tokenize};

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

    #[rstest]
    fn test_expression_depth_limit_on_constrained_stack() {
        thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let bindings = Bindings::new();

                let parens = format!(
                    "{}1{}",
                    "(".repeat(MAX_EXPRESSION_DEPTH - 1),
                    ")".repeat(MAX_EXPRESSION_DEPTH - 1)
                );
                drop(parse_program(&parens, &bindings));

                let prefixes = format!("{}1", "-".repeat(MAX_EXPRESSION_DEPTH - 1));
                let prefix_tokens = tokenize(&prefixes, &bindings).unwrap();
                let prefix_program = parse(&prefix_tokens).unwrap();
                drop(prefix_program);

                let spine = std::iter::repeat_n("1", MAX_EXPRESSION_DEPTH)
                    .collect::<Vec<_>>()
                    .join(" + ");
                let spine_tokens = tokenize(&spine, &bindings).unwrap();
                let spine_program = parse(&spine_tokens).unwrap();
                let compiled = eval::compile(&spine_program, &bindings).unwrap();
                drop(compiled);
                drop(spine_program);

                let drop_tokens = tokenize(&spine, &bindings).unwrap();
                let drop_program = parse(&drop_tokens).unwrap();
                drop(drop_program);

                let right_spine = std::iter::repeat_n("2", MAX_EXPRESSION_DEPTH)
                    .collect::<Vec<_>>()
                    .join(" ^ ");
                let right_tokens = tokenize(&right_spine, &bindings).unwrap();
                let right_program = parse(&right_tokens).unwrap();
                drop(right_program);

                // A call nests both the syntax depth and the argument's own depth, so
                // `MAX_EXPRESSION_DEPTH - 1` nested calls sit exactly at the limit.
                let calls = format!(
                    "{}1{}",
                    "abs(".repeat(MAX_EXPRESSION_DEPTH - 1),
                    ")".repeat(MAX_EXPRESSION_DEPTH - 1)
                );
                let call_tokens = tokenize(&calls, &bindings).unwrap();
                let call_program = parse(&call_tokens).unwrap();
                let call_compiled = eval::compile(&call_program, &bindings).unwrap();
                drop(call_compiled);
                drop(call_program);

                let excessive_spine = format!("{spine} + 1");
                let excessive_tokens = tokenize(&excessive_spine, &bindings).unwrap();
                assert_eq!(
                    parse(&excessive_tokens).unwrap_err(),
                    ExpressionError::ExpressionDepthExceeded {
                        depth: MAX_EXPRESSION_DEPTH + 1,
                        max: MAX_EXPRESSION_DEPTH,
                    }
                );

                let excessive_right_spine = format!("{right_spine} ^ 2");
                let excessive_tokens = tokenize(&excessive_right_spine, &bindings).unwrap();
                assert_eq!(
                    parse(&excessive_tokens).unwrap_err(),
                    ExpressionError::ExpressionDepthExceeded {
                        depth: MAX_EXPRESSION_DEPTH + 1,
                        max: MAX_EXPRESSION_DEPTH,
                    }
                );

                let excessive_parens = format!(
                    "{}1{}",
                    "(".repeat(MAX_EXPRESSION_DEPTH),
                    ")".repeat(MAX_EXPRESSION_DEPTH)
                );
                let excessive_tokens = tokenize(&excessive_parens, &bindings).unwrap();
                assert_eq!(
                    parse(&excessive_tokens).unwrap_err(),
                    ExpressionError::ExpressionDepthExceeded {
                        depth: MAX_EXPRESSION_DEPTH + 1,
                        max: MAX_EXPRESSION_DEPTH,
                    }
                );

                let excessive_calls = format!(
                    "{}1{}",
                    "abs(".repeat(MAX_EXPRESSION_DEPTH),
                    ")".repeat(MAX_EXPRESSION_DEPTH)
                );
                let excessive_tokens = tokenize(&excessive_calls, &bindings).unwrap();
                assert_eq!(
                    parse(&excessive_tokens).unwrap_err(),
                    ExpressionError::ExpressionDepthExceeded {
                        depth: MAX_EXPRESSION_DEPTH + 1,
                        max: MAX_EXPRESSION_DEPTH,
                    }
                );
            })
            .unwrap()
            .join()
            .unwrap();
    }

    #[rstest]
    fn test_deep_expression_returns_error_without_crashing_process() {
        thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(|| {
                let bindings = Bindings::new();
                let source = format!("{}1", "-".repeat(10_000));
                let tokens = tokenize(&source, &bindings).unwrap();

                assert!(matches!(
                    parse(&tokens),
                    Err(ExpressionError::ExpressionDepthExceeded { .. })
                ));

                let source = std::iter::repeat_n("2", 10_000)
                    .collect::<Vec<_>>()
                    .join(" ^ ");
                let tokens = tokenize(&source, &bindings).unwrap();

                assert!(matches!(
                    parse(&tokens),
                    Err(ExpressionError::ExpressionDepthExceeded { .. })
                ));
            })
            .unwrap()
            .join()
            .unwrap();
    }
}
