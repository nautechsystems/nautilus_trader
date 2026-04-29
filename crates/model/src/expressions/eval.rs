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

use ahash::AHashMap;

use super::{
    Bindings, Result, ValueType,
    error::ExpressionError,
    parser::{BinaryOp, Expr, Program, Statement, UnaryOp},
};

const MAX_STACK: usize = 32;
const MAX_LOCALS: usize = 16;

/// A compiled formula ready for repeated evaluation against f64 input slots.
///
/// Created by [`compile`](super::compile) or [`compile_numeric`](super::compile_numeric).
/// The formula is compiled once and evaluated many times with different inputs.
/// Evaluation uses a fixed-size inline stack with no heap allocation.
#[derive(Clone, Debug)]
pub(crate) struct CompiledExpression {
    code: Vec<Instruction>,
    input_len: usize,
    result_type: ValueType,
}

impl Default for CompiledExpression {
    fn default() -> Self {
        Self {
            code: Vec::new(),
            input_len: 0,
            result_type: ValueType::Empty,
        }
    }
}

impl CompiledExpression {
    #[must_use]
    pub const fn result_type(&self) -> ValueType {
        self.result_type
    }

    /// # Errors
    ///
    /// Returns an error if the compiled expression does not produce a numeric result.
    #[inline]
    pub fn eval_number(&self, inputs: &[f64]) -> Result<f64> {
        self.eval_core(inputs, None)
    }

    #[cfg(test)]
    fn eval_number_trace(&self, inputs: &[f64]) -> Result<(f64, Vec<usize>)> {
        let mut trace = Vec::new();
        let value = self.eval_core(inputs, Some(&mut trace))?;
        Ok((value, trace))
    }

    fn eval_core(&self, inputs: &[f64], mut trace: Option<&mut Vec<usize>>) -> Result<f64> {
        if self.result_type == ValueType::Empty {
            return Err(ExpressionError::EmptyResult);
        }

        if inputs.len() < self.input_len {
            return Err(ExpressionError::InputCountMismatch {
                expected: self.input_len,
                actual: inputs.len(),
            });
        }

        let mut stack = [0.0_f64; MAX_STACK];
        let mut sp: usize = 0;
        let mut locals = [0.0_f64; MAX_LOCALS];
        let mut pc = 0;

        while pc < self.code.len() {
            if let Some(ref mut t) = trace {
                t.push(pc);
            }

            match &self.code[pc] {
                Instruction::PushNumber(v) => {
                    debug_assert!(sp < MAX_STACK, "stack overflow at pc={pc}");
                    stack[sp] = *v;
                    sp += 1;
                }
                Instruction::PushBool(v) => {
                    debug_assert!(sp < MAX_STACK, "stack overflow at pc={pc}");
                    stack[sp] = if *v { 1.0 } else { 0.0 };
                    sp += 1;
                }
                Instruction::LoadInput(slot) => {
                    debug_assert!(sp < MAX_STACK, "stack overflow at pc={pc}");
                    debug_assert!(*slot < inputs.len(), "input slot {slot} out of bounds");
                    stack[sp] = inputs[*slot];
                    sp += 1;
                }
                Instruction::LoadLocal(slot) => {
                    debug_assert!(sp < MAX_STACK, "stack overflow at pc={pc}");
                    debug_assert!(*slot < MAX_LOCALS, "local slot {slot} out of bounds");
                    stack[sp] = locals[*slot];
                    sp += 1;
                }
                Instruction::StoreLocal(slot) => {
                    debug_assert!(sp > 0, "stack underflow at pc={pc}");
                    debug_assert!(*slot < MAX_LOCALS, "local slot {slot} out of bounds");
                    sp -= 1;
                    locals[*slot] = stack[sp];
                }
                Instruction::Pop => {
                    debug_assert!(sp > 0, "stack underflow at pc={pc}");
                    sp -= 1;
                }
                Instruction::Unary(op) => {
                    debug_assert!(sp >= 1, "stack underflow at pc={pc}");
                    let top = stack[sp - 1];
                    stack[sp - 1] = eval_unary_fast(*op, top);
                }
                Instruction::Binary(op) => {
                    debug_assert!(sp >= 2, "stack underflow at pc={pc}");
                    sp -= 1;
                    let right = stack[sp];
                    let left = stack[sp - 1];
                    stack[sp - 1] = eval_binary_fast(*op, left, right);
                }
                Instruction::Jump(target) => {
                    debug_assert!(
                        *target <= self.code.len(),
                        "jump target {target} out of bounds"
                    );
                    pc = *target;
                    continue;
                }
                Instruction::JumpIfFalse(target) => {
                    debug_assert!(sp > 0, "stack underflow at pc={pc}");
                    debug_assert!(
                        *target <= self.code.len(),
                        "jump target {target} out of bounds"
                    );
                    sp -= 1;

                    if stack[sp] == 0.0 {
                        pc = *target;
                        continue;
                    }
                }
                Instruction::JumpIfFalsePeek(target) => {
                    debug_assert!(sp >= 1, "stack underflow at pc={pc}");
                    debug_assert!(
                        *target <= self.code.len(),
                        "jump target {target} out of bounds"
                    );

                    if stack[sp - 1] == 0.0 {
                        pc = *target;
                        continue;
                    }
                }
                Instruction::JumpIfTruePeek(target) => {
                    debug_assert!(sp >= 1, "stack underflow at pc={pc}");
                    debug_assert!(
                        *target <= self.code.len(),
                        "jump target {target} out of bounds"
                    );

                    if stack[sp - 1] != 0.0 {
                        pc = *target;
                        continue;
                    }
                }
                Instruction::Call { builtin, argc } => {
                    debug_assert!(sp >= *argc, "stack underflow at pc={pc}");
                    sp -= argc;
                    let result = eval_builtin_fast(*builtin, &stack[sp..sp + argc]);
                    stack[sp] = result;
                    sp += 1;
                }
            }

            pc += 1;
        }

        if sp == 0 {
            return Err(ExpressionError::EmptyResult);
        }

        Ok(stack[sp - 1])
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Instruction {
    PushNumber(f64),
    PushBool(bool),
    LoadInput(usize),
    LoadLocal(usize),
    StoreLocal(usize),
    Pop,
    Unary(UnaryInstruction),
    Binary(BinaryInstruction),
    Jump(usize),
    JumpIfFalse(usize),
    JumpIfFalsePeek(usize),
    JumpIfTruePeek(usize),
    Call { builtin: Builtin, argc: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum UnaryInstruction {
    Neg,
    Not,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BinaryInstruction {
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Builtin {
    Abs,
    Ceil,
    Floor,
    Round,
    Min,
    Max,
    If,
}

impl Builtin {
    fn from_name(name: &str) -> Option<Self> {
        match name {
            "abs" => Some(Self::Abs),
            "ceil" => Some(Self::Ceil),
            "floor" => Some(Self::Floor),
            "round" => Some(Self::Round),
            "min" => Some(Self::Min),
            "max" => Some(Self::Max),
            "if" => Some(Self::If),
            _ => None,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Abs => "abs",
            Self::Ceil => "ceil",
            Self::Floor => "floor",
            Self::Round => "round",
            Self::Min => "min",
            Self::Max => "max",
            Self::If => "if",
        }
    }
}

pub(super) fn compile(program: &Program, bindings: &Bindings) -> Result<CompiledExpression> {
    Compiler::new(bindings).compile(program)
}

#[derive(Clone, Debug)]
struct LocalInfo {
    slot: usize,
    value_type: ValueType,
}

struct Compiler<'a> {
    bindings: &'a Bindings,
    code: Vec<Instruction>,
    locals: AHashMap<String, LocalInfo>,
    next_local_slot: usize,
    stack_depth: usize,
    max_stack_depth: usize,
}

impl<'a> Compiler<'a> {
    fn new(bindings: &'a Bindings) -> Self {
        Self {
            bindings,
            code: Vec::new(),
            locals: AHashMap::new(),
            next_local_slot: 0,
            stack_depth: 0,
            max_stack_depth: 0,
        }
    }

    fn compile(mut self, program: &Program) -> Result<CompiledExpression> {
        let is_value_position =
            |index: usize| index + 1 == program.statements.len() && !program.trailing_semicolon;

        let mut result_type = ValueType::Empty;

        for (index, statement) in program.statements.iter().enumerate() {
            match statement {
                Statement::Assign { name, expr } => {
                    let expr_type = self.compile_expr(expr)?;
                    let slot = self.local_slot(name, expr_type)?;
                    self.code.push(Instruction::StoreLocal(slot));
                    self.pop_n(1);

                    if is_value_position(index) {
                        result_type = ValueType::Empty;
                    }
                }
                Statement::Expr(expr) => {
                    let expr_type = self.compile_expr(expr)?;

                    if is_value_position(index) {
                        result_type = expr_type;
                    } else {
                        self.code.push(Instruction::Pop);
                        self.pop_n(1);
                    }
                }
            }
        }

        if self.max_stack_depth > MAX_STACK {
            return Err(ExpressionError::StackOverflow {
                depth: self.max_stack_depth,
                max: MAX_STACK,
            });
        }

        if self.next_local_slot > MAX_LOCALS {
            return Err(ExpressionError::TooManyLocals {
                count: self.next_local_slot,
                max: MAX_LOCALS,
            });
        }

        Ok(CompiledExpression {
            code: self.code,
            input_len: self.bindings.input_len(),
            result_type,
        })
    }

    fn compile_expr(&mut self, expr: &Expr) -> Result<ValueType> {
        match expr {
            Expr::Number(value) => {
                self.code.push(Instruction::PushNumber(*value));
                self.push();
                Ok(ValueType::Number)
            }
            Expr::Bool(value) => {
                self.code.push(Instruction::PushBool(*value));
                self.push();
                Ok(ValueType::Bool)
            }
            Expr::Input(slot) => {
                self.code.push(Instruction::LoadInput(*slot));
                self.push();
                Ok(ValueType::Number)
            }
            Expr::Name(name) => self.compile_name(name),
            Expr::Unary { op, expr } => self.compile_unary(*op, expr),
            Expr::Binary { left, op, right } => self.compile_binary(left, *op, right),
            Expr::Call { name, args } => self.compile_call(name, args),
        }
    }

    fn compile_name(&mut self, name: &str) -> Result<ValueType> {
        if let Some(local) = self.locals.get(name) {
            let slot = local.slot;
            let value_type = local.value_type;
            self.code.push(Instruction::LoadLocal(slot));
            self.push();
            return Ok(value_type);
        }

        if let Some(slot) = self.bindings.resolve_plain(name) {
            self.code.push(Instruction::LoadInput(slot));
            self.push();
            return Ok(ValueType::Number);
        }

        Err(ExpressionError::UnknownSymbol {
            name: name.to_string(),
        })
    }

    fn compile_unary(&mut self, op: UnaryOp, expr: &Expr) -> Result<ValueType> {
        let expr_type = self.compile_expr(expr)?;

        match op {
            UnaryOp::Neg => {
                ensure_type(expr_type, ValueType::Number, "unary `-`")?;
                self.code.push(Instruction::Unary(UnaryInstruction::Neg));
                Ok(ValueType::Number)
            }
            UnaryOp::Not => {
                ensure_type(expr_type, ValueType::Bool, "unary `!`")?;
                self.code.push(Instruction::Unary(UnaryInstruction::Not));
                Ok(ValueType::Bool)
            }
        }
    }

    fn compile_binary(&mut self, left: &Expr, op: BinaryOp, right: &Expr) -> Result<ValueType> {
        match op {
            BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::Pow => {
                let left_type = self.compile_expr(left)?;
                let right_type = self.compile_expr(right)?;
                ensure_type(left_type, ValueType::Number, op_context(op))?;
                ensure_type(right_type, ValueType::Number, op_context(op))?;
                self.code
                    .push(Instruction::Binary(to_binary_instruction(op)));
                self.pop_n(1); // Pops 2, pushes 1
                Ok(ValueType::Number)
            }
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge => {
                let left_type = self.compile_expr(left)?;
                let right_type = self.compile_expr(right)?;
                ensure_type(left_type, ValueType::Number, op_context(op))?;
                ensure_type(right_type, ValueType::Number, op_context(op))?;
                self.code
                    .push(Instruction::Binary(to_binary_instruction(op)));
                self.pop_n(1);
                Ok(ValueType::Bool)
            }
            BinaryOp::Eq | BinaryOp::Neq => {
                let left_type = self.compile_expr(left)?;
                let right_type = self.compile_expr(right)?;
                if left_type != right_type {
                    return Err(ExpressionError::BinaryTypeMismatch {
                        context: op_context(op),
                        left: left_type,
                        right: right_type,
                    });
                }
                self.code
                    .push(Instruction::Binary(to_binary_instruction(op)));
                self.pop_n(1);
                Ok(ValueType::Bool)
            }
            BinaryOp::And => {
                let left_type = self.compile_expr(left)?;
                ensure_type(left_type, ValueType::Bool, op_context(op))?;
                let jump_index = self.emit_jump(Instruction::JumpIfFalsePeek(usize::MAX));
                self.code.push(Instruction::Pop);
                self.pop_n(1);
                let right_type = self.compile_expr(right)?;
                ensure_type(right_type, ValueType::Bool, op_context(op))?;
                self.patch_jump_target(jump_index, self.code.len());
                Ok(ValueType::Bool)
            }
            BinaryOp::Or => {
                let left_type = self.compile_expr(left)?;
                ensure_type(left_type, ValueType::Bool, op_context(op))?;
                let jump_index = self.emit_jump(Instruction::JumpIfTruePeek(usize::MAX));
                self.code.push(Instruction::Pop);
                self.pop_n(1);
                let right_type = self.compile_expr(right)?;
                ensure_type(right_type, ValueType::Bool, op_context(op))?;
                self.patch_jump_target(jump_index, self.code.len());
                Ok(ValueType::Bool)
            }
        }
    }

    fn compile_call(&mut self, name: &str, args: &[Expr]) -> Result<ValueType> {
        let Some(builtin) = Builtin::from_name(name) else {
            return Err(ExpressionError::UnknownFunction {
                name: name.to_string(),
            });
        };
        let builtin_name = builtin.name();

        if builtin == Builtin::If {
            return self.compile_if(args);
        }

        let arg_types: Vec<ValueType> = args
            .iter()
            .map(|expr| self.compile_expr(expr))
            .collect::<Result<_>>()?;

        let result_type = match builtin {
            Builtin::Abs | Builtin::Ceil | Builtin::Floor | Builtin::Round => {
                ensure_arg_count(builtin_name, args.len(), 1)?;
                ensure_type(arg_types[0], ValueType::Number, builtin_name)?;
                ValueType::Number
            }
            Builtin::Min | Builtin::Max => {
                ensure_min_arg_count(builtin_name, args.len(), 1)?;

                for arg_type in &arg_types {
                    ensure_type(*arg_type, ValueType::Number, builtin_name)?;
                }
                ValueType::Number
            }
            Builtin::If => unreachable!("handled above"),
        };

        self.code.push(Instruction::Call {
            builtin,
            argc: args.len(),
        });

        // Call pops argc args, pushes 1 result
        if args.len() > 1 {
            self.pop_n(args.len() - 1);
        }

        Ok(result_type)
    }

    fn compile_if(&mut self, args: &[Expr]) -> Result<ValueType> {
        ensure_arg_count("if", args.len(), 3)?;

        let condition_type = self.compile_expr(&args[0])?;
        ensure_type(condition_type, ValueType::Bool, "if")?;

        // JumpIfFalse pops the condition
        let jump_if_false = self.emit_jump(Instruction::JumpIfFalse(usize::MAX));
        self.pop_n(1);

        let branch_base = self.stack_depth;
        let then_type = self.compile_expr(&args[1])?;
        let jump_end = self.emit_jump(Instruction::Jump(usize::MAX));

        // Reset to branch base for the else path
        self.stack_depth = branch_base;
        let else_start = self.code.len();
        self.patch_jump_target(jump_if_false, else_start);

        let else_type = self.compile_expr(&args[2])?;
        if then_type != else_type {
            return Err(ExpressionError::BinaryTypeMismatch {
                context: "if",
                left: then_type,
                right: else_type,
            });
        }

        self.patch_jump_target(jump_end, self.code.len());
        Ok(then_type)
    }

    fn emit_jump(&mut self, instruction: Instruction) -> usize {
        let index = self.code.len();
        self.code.push(instruction);
        index
    }

    fn patch_jump_target(&mut self, index: usize, target: usize) {
        match &mut self.code[index] {
            Instruction::Jump(existing)
            | Instruction::JumpIfFalse(existing)
            | Instruction::JumpIfFalsePeek(existing)
            | Instruction::JumpIfTruePeek(existing) => *existing = target,
            _ => unreachable!("only jump instructions can be patched"),
        }
    }

    fn push(&mut self) {
        self.stack_depth += 1;

        if self.stack_depth > self.max_stack_depth {
            self.max_stack_depth = self.stack_depth;
        }
    }

    fn pop_n(&mut self, n: usize) {
        self.stack_depth -= n;
    }

    fn local_slot(&mut self, name: &str, value_type: ValueType) -> Result<usize> {
        if let Some(local) = self.locals.get(name) {
            if local.value_type != value_type {
                return Err(ExpressionError::BinaryTypeMismatch {
                    context: "assignment",
                    left: local.value_type,
                    right: value_type,
                });
            }
            return Ok(local.slot);
        }

        let slot = self.next_local_slot;
        self.next_local_slot += 1;
        self.locals
            .insert(name.to_string(), LocalInfo { slot, value_type });
        Ok(slot)
    }
}

fn ensure_arg_count(name: &'static str, actual: usize, expected: usize) -> Result<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(ExpressionError::InvalidArgumentCount {
            name,
            expected: expected.to_string(),
            actual,
        })
    }
}

fn ensure_min_arg_count(name: &'static str, actual: usize, minimum: usize) -> Result<()> {
    if actual >= minimum {
        Ok(())
    } else {
        Err(ExpressionError::InvalidArgumentCount {
            name,
            expected: format!("at least {minimum}"),
            actual,
        })
    }
}

fn ensure_type(actual: ValueType, expected: ValueType, context: &'static str) -> Result<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(ExpressionError::TypeMismatch {
            context,
            expected,
            actual,
        })
    }
}

fn op_context(op: BinaryOp) -> &'static str {
    match op {
        BinaryOp::Add => "`+`",
        BinaryOp::Sub => "`-`",
        BinaryOp::Mul => "`*`",
        BinaryOp::Div => "`/`",
        BinaryOp::Mod => "`%`",
        BinaryOp::Pow => "`^`",
        BinaryOp::Eq => "`==`",
        BinaryOp::Neq => "`!=`",
        BinaryOp::Lt => "`<`",
        BinaryOp::Le => "`<=`",
        BinaryOp::Gt => "`>`",
        BinaryOp::Ge => "`>=`",
        BinaryOp::And => "`&&`",
        BinaryOp::Or => "`||`",
    }
}

fn to_binary_instruction(op: BinaryOp) -> BinaryInstruction {
    match op {
        BinaryOp::Add => BinaryInstruction::Add,
        BinaryOp::Sub => BinaryInstruction::Sub,
        BinaryOp::Mul => BinaryInstruction::Mul,
        BinaryOp::Div => BinaryInstruction::Div,
        BinaryOp::Mod => BinaryInstruction::Mod,
        BinaryOp::Pow => BinaryInstruction::Pow,
        BinaryOp::Eq => BinaryInstruction::Eq,
        BinaryOp::Neq => BinaryInstruction::Neq,
        BinaryOp::Lt => BinaryInstruction::Lt,
        BinaryOp::Le => BinaryInstruction::Le,
        BinaryOp::Gt => BinaryInstruction::Gt,
        BinaryOp::Ge => BinaryInstruction::Ge,
        BinaryOp::And => BinaryInstruction::And,
        BinaryOp::Or => BinaryInstruction::Or,
    }
}

#[inline(always)]
fn eval_unary_fast(op: UnaryInstruction, value: f64) -> f64 {
    match op {
        UnaryInstruction::Neg => -value,
        UnaryInstruction::Not => {
            if value == 0.0 {
                1.0
            } else {
                0.0
            }
        }
    }
}

#[inline(always)]
fn eval_binary_fast(op: BinaryInstruction, left: f64, right: f64) -> f64 {
    match op {
        BinaryInstruction::Add => left + right,
        BinaryInstruction::Sub => left - right,
        BinaryInstruction::Mul => left * right,
        BinaryInstruction::Div => left / right,
        BinaryInstruction::Mod => left % right,
        BinaryInstruction::Pow => left.powf(right),
        BinaryInstruction::Eq => {
            if left == right {
                1.0
            } else {
                0.0
            }
        }
        BinaryInstruction::Neq => {
            if left == right {
                0.0
            } else {
                1.0
            }
        }
        BinaryInstruction::Lt => {
            if left < right {
                1.0
            } else {
                0.0
            }
        }
        BinaryInstruction::Le => {
            if left <= right {
                1.0
            } else {
                0.0
            }
        }
        BinaryInstruction::Gt => {
            if left > right {
                1.0
            } else {
                0.0
            }
        }
        BinaryInstruction::Ge => {
            if left >= right {
                1.0
            } else {
                0.0
            }
        }
        BinaryInstruction::And => {
            if left != 0.0 && right != 0.0 {
                1.0
            } else {
                0.0
            }
        }
        BinaryInstruction::Or => {
            if left != 0.0 || right != 0.0 {
                1.0
            } else {
                0.0
            }
        }
    }
}

#[inline(always)]
fn eval_builtin_fast(builtin: Builtin, args: &[f64]) -> f64 {
    match builtin {
        Builtin::Abs => args[0].abs(),
        Builtin::Ceil => args[0].ceil(),
        Builtin::Floor => args[0].floor(),
        Builtin::Round => args[0].round(),
        Builtin::Min => args.iter().copied().fold(f64::INFINITY, f64::min),
        Builtin::Max => args.iter().copied().fold(f64::NEG_INFINITY, f64::max),
        Builtin::If => {
            if args[0] == 0.0 {
                args[2]
            } else {
                args[1]
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::expressions::{Bindings, compile, compile_numeric};

    fn bindings() -> Bindings {
        let mut bindings = Bindings::new();
        bindings.add(0, "x").unwrap();
        bindings.add(1, "AUD/USD.SIM").unwrap();
        bindings
    }

    #[rstest]
    fn test_eval_numeric_expression_with_assignments_and_special_bindings() {
        let compiled =
            compile_numeric("spread = AUD/USD.SIM - x; spread / 2", &bindings()).unwrap();
        let value = compiled.eval_number(&[2.0, 6.0]).unwrap();

        assert_eq!(value, 2.0);
    }

    #[rstest]
    fn test_eval_boolean_expression_with_functions() {
        let compiled = compile("if(x > 2, max(1, x), 0) >= 3", &bindings()).unwrap();

        assert_eq!(compiled.result_type(), ValueType::Bool);

        // Bool result encoded as 1.0 (true) in the f64 fast path
        let value = compiled.eval_number(&[3.0, 0.0]).unwrap();
        assert_eq!(value, 1.0);
    }

    #[rstest]
    fn test_eval_unary_minus_after_power() {
        let compiled = compile_numeric("-2 ^ 2", &Bindings::new()).unwrap();
        let value = compiled.eval_number(&[]).unwrap();

        assert_eq!(value, -4.0);
    }

    #[rstest]
    fn test_compile_short_circuits_if() {
        let compiled = compile_numeric("if(x > 0, x, AUD/USD.SIM)", &bindings()).unwrap();

        assert!(
            compiled
                .code
                .iter()
                .any(|instruction| matches!(instruction, Instruction::JumpIfFalse(_)))
        );
        assert!(
            compiled
                .code
                .iter()
                .any(|instruction| matches!(instruction, Instruction::Jump(_)))
        );
        assert!(!compiled.code.iter().any(|instruction| matches!(
            instruction,
            Instruction::Call {
                builtin: Builtin::If,
                ..
            }
        )));
    }

    #[rstest]
    fn test_compile_short_circuits_logical_operators() {
        let compiled = compile("x > 0 && AUD/USD.SIM > 0 || false", &bindings()).unwrap();

        assert!(
            compiled
                .code
                .iter()
                .any(|instruction| matches!(instruction, Instruction::JumpIfFalsePeek(_)))
        );
        assert!(
            compiled
                .code
                .iter()
                .any(|instruction| matches!(instruction, Instruction::JumpIfTruePeek(_)))
        );
        assert!(
            compiled
                .code
                .iter()
                .filter(|instruction| matches!(instruction, Instruction::Pop))
                .count()
                >= 2
        );
    }

    #[rstest]
    fn test_compile_rejects_read_before_assignment() {
        let error = compile_numeric("spread + 1", &Bindings::new()).unwrap_err();

        assert_eq!(
            error,
            ExpressionError::UnknownSymbol {
                name: "spread".to_string(),
            }
        );
    }

    #[rstest]
    fn test_compile_rejects_local_type_change() {
        let error = compile("x = true; x = 1", &Bindings::new()).unwrap_err();

        assert_eq!(
            error,
            ExpressionError::BinaryTypeMismatch {
                context: "assignment",
                left: ValueType::Bool,
                right: ValueType::Number,
            }
        );
    }

    #[rstest]
    fn test_compile_numeric_rejects_boolean_results() {
        let error = compile_numeric("x > 1", &bindings()).unwrap_err();

        assert_eq!(
            error,
            ExpressionError::NonNumericResult {
                actual: ValueType::Bool,
            }
        );
    }

    #[rstest]
    fn test_eval_rejects_missing_inputs() {
        let compiled = compile_numeric("x + 1", &bindings()).unwrap();
        let error = compiled.eval_number(&[]).unwrap_err();

        assert_eq!(
            error,
            ExpressionError::InputCountMismatch {
                expected: 2,
                actual: 0,
            }
        );
    }

    #[rstest]
    fn test_ensure_arg_count_accepts_new_exact_arities() {
        assert!(ensure_arg_count("test", 2, 2).is_ok());

        let error = ensure_arg_count("test", 1, 2).unwrap_err();

        assert_eq!(
            error,
            ExpressionError::InvalidArgumentCount {
                name: "test",
                expected: "2".to_string(),
                actual: 1,
            }
        );
    }

    #[rstest]
    #[case("x + 1", &[5.0, 0.0], 6.0)]
    #[case("x - 1", &[5.0, 0.0], 4.0)]
    #[case("x * 3", &[5.0, 0.0], 15.0)]
    #[case("x / 2", &[10.0, 0.0], 5.0)]
    #[case("x % 3", &[7.0, 0.0], 1.0)]
    #[case("x ^ 3", &[2.0, 0.0], 8.0)]
    fn test_eval_arithmetic_operators(
        #[case] formula: &str,
        #[case] inputs: &[f64],
        #[case] expected: f64,
    ) {
        let compiled = compile_numeric(formula, &bindings()).unwrap();
        let value = compiled.eval_number(inputs).unwrap();

        assert_eq!(value, expected);
    }

    #[rstest]
    #[case("x < 10", &[5.0, 0.0], 1.0)]
    #[case("x < 10", &[15.0, 0.0], 0.0)]
    #[case("x <= 5", &[5.0, 0.0], 1.0)]
    #[case("x <= 5", &[6.0, 0.0], 0.0)]
    #[case("x > 5", &[5.0, 0.0], 0.0)]
    #[case("x > 5", &[6.0, 0.0], 1.0)]
    #[case("x >= 5", &[5.0, 0.0], 1.0)]
    #[case("x >= 5", &[4.0, 0.0], 0.0)]
    #[case("x == 5", &[5.0, 0.0], 1.0)]
    #[case("x == 5", &[6.0, 0.0], 0.0)]
    #[case("x != 5", &[5.0, 0.0], 0.0)]
    #[case("x != 5", &[6.0, 0.0], 1.0)]
    fn test_eval_comparison_operators(
        #[case] formula: &str,
        #[case] inputs: &[f64],
        #[case] expected: f64,
    ) {
        let compiled = compile(formula, &bindings()).unwrap();
        let value = compiled.eval_number(inputs).unwrap();

        assert_eq!(value, expected);
    }

    #[rstest]
    #[case("true && false", 0.0)]
    #[case("true && true", 1.0)]
    #[case("false || true", 1.0)]
    #[case("false || false", 0.0)]
    #[case("!false", 1.0)]
    #[case("!true", 0.0)]
    fn test_eval_logical_operators(#[case] formula: &str, #[case] expected: f64) {
        let compiled = compile(formula, &Bindings::new()).unwrap();
        let value = compiled.eval_number(&[]).unwrap();

        assert_eq!(value, expected);
    }

    #[rstest]
    #[case("abs(x)", &[-3.0, 0.0], 3.0)]
    #[case("abs(x)", &[3.0, 0.0], 3.0)]
    #[case("ceil(x)", &[2.3, 0.0], 3.0)]
    #[case("ceil(x)", &[-2.3, 0.0], -2.0)]
    #[case("floor(x)", &[2.7, 0.0], 2.0)]
    #[case("floor(x)", &[-2.7, 0.0], -3.0)]
    #[case("round(x)", &[2.5, 0.0], 3.0)]
    #[case("round(x)", &[2.4, 0.0], 2.0)]
    #[case("min(x, 10)", &[3.0, 0.0], 3.0)]
    #[case("min(x, 10)", &[20.0, 0.0], 10.0)]
    #[case("min(x, AUD/USD.SIM, 100)", &[5.0, 3.0], 3.0)]
    #[case("max(x, 10)", &[3.0, 0.0], 10.0)]
    #[case("max(x, 10)", &[20.0, 0.0], 20.0)]
    #[case("max(x, AUD/USD.SIM, 0)", &[5.0, 8.0], 8.0)]
    #[case("if(x > 0, x, 10)", &[5.0, 0.0], 5.0)]
    #[case("if(x > 0, x, 10)", &[-5.0, 0.0], 10.0)]
    fn test_eval_builtin_functions(
        #[case] formula: &str,
        #[case] inputs: &[f64],
        #[case] expected: f64,
    ) {
        let compiled = compile_numeric(formula, &bindings()).unwrap();
        let value = compiled.eval_number(inputs).unwrap();

        assert_eq!(value, expected);
    }

    #[rstest]
    fn test_compile_rejects_stack_overflow() {
        let bindings = Bindings::new();

        // min(1, 1, ..., 1) with MAX_STACK + 1 args pushes all before the call
        let args = vec!["1"; MAX_STACK + 1].join(", ");
        let formula = format!("min({args})");
        let error = compile_numeric(&formula, &bindings).unwrap_err();

        assert_eq!(
            error,
            ExpressionError::StackOverflow {
                depth: MAX_STACK + 1,
                max: MAX_STACK,
            }
        );
    }

    #[rstest]
    fn test_compile_rejects_too_many_locals() {
        let bindings = Bindings::new();

        // Build "a0 = 1; a1 = 1; ... aN = 1; aN" with MAX_LOCALS + 1 locals
        let count = MAX_LOCALS + 1;
        let assignments: Vec<String> = (0..count).map(|i| format!("a{i} = 1")).collect();
        let formula = format!("{}; a0", assignments.join("; "));
        let error = compile_numeric(&formula, &bindings).unwrap_err();

        assert_eq!(
            error,
            ExpressionError::TooManyLocals {
                count,
                max: MAX_LOCALS,
            }
        );
    }

    #[rstest]
    fn test_if_short_circuits_untaken_branch_at_runtime() {
        let mut b = Bindings::new();
        b.add(0, "x").unwrap();
        b.add(1, "AUD/USD.SIM").unwrap();

        let compiled = compile_numeric("if(x > 0, AUD/USD.SIM / x, 42)", &b).unwrap();
        let skipped_index = find_instruction_index(&compiled, |instruction| {
            matches!(instruction, Instruction::LoadInput(1))
        });
        let (value, trace) = compiled.eval_number_trace(&[0.0, 99.0]).unwrap();

        assert_eq!(value, 42.0);
        assert!(!trace.contains(&skipped_index));
    }

    #[rstest]
    fn test_and_short_circuits_when_left_is_false() {
        let mut b = Bindings::new();
        b.add(0, "x").unwrap();
        b.add(1, "AUD/USD.SIM").unwrap();

        let compiled = compile("x > 0 && AUD/USD.SIM > 0", &b).unwrap();
        let skipped_index = find_instruction_index(&compiled, |instruction| {
            matches!(instruction, Instruction::LoadInput(1))
        });
        let (value, trace) = compiled.eval_number_trace(&[0.0, 99.0]).unwrap();

        assert_eq!(value, 0.0);
        assert!(!trace.contains(&skipped_index));
    }

    #[rstest]
    fn test_or_short_circuits_when_left_is_true() {
        let mut b = Bindings::new();
        b.add(0, "x").unwrap();
        b.add(1, "AUD/USD.SIM").unwrap();

        let compiled = compile("x > 0 || AUD/USD.SIM > 0", &b).unwrap();
        let skipped_index = find_instruction_index(&compiled, |instruction| {
            matches!(instruction, Instruction::LoadInput(1))
        });
        let (value, trace) = compiled.eval_number_trace(&[5.0, 99.0]).unwrap();

        assert_eq!(value, 1.0);
        assert!(!trace.contains(&skipped_index));
    }

    fn find_instruction_index(
        compiled: &CompiledExpression,
        predicate: impl Fn(&Instruction) -> bool,
    ) -> usize {
        compiled
            .code
            .iter()
            .enumerate()
            .find_map(|(index, instruction)| predicate(instruction).then_some(index))
            .unwrap()
    }
}
