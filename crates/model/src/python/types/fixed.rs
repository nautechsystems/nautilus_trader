// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use nautilus_core::python::to_pyvalue_err;
use pyo3::{
    Bound, PyErr,
    exceptions::{PyOverflowError, PyZeroDivisionError},
    types::{PyAny, PyAnyMethods},
};
use rust_decimal::Decimal;

use crate::types::{
    Money, Price, Quantity,
    fixed::{MAX_FLOAT_PRECISION, raw_scales_match},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ArithmeticError {
    Overflow,
    DivisionByZero,
    IncompatibleOperands,
    InvalidFloatPrecision(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ArithmeticOperation {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
}

pub(super) trait FloatArithmetic {
    fn as_f64_checked(&self) -> Result<f64, ArithmeticError>;
}

impl ArithmeticOperation {
    pub(super) fn checked_decimal(
        self,
        lhs: Decimal,
        rhs: Decimal,
    ) -> Result<Decimal, ArithmeticError> {
        if matches!(self, Self::Div | Self::Rem) && rhs.is_zero() {
            return Err(ArithmeticError::DivisionByZero);
        }

        let result = match self {
            Self::Add => lhs.checked_add(rhs),
            Self::Sub => lhs.checked_sub(rhs),
            Self::Mul => lhs.checked_mul(rhs),
            Self::Div => lhs.checked_div(rhs),
            Self::Rem => lhs.checked_rem(rhs),
        };
        result.ok_or(ArithmeticError::Overflow)
    }

    pub(super) fn checked_f64(self, lhs: f64, rhs: f64) -> Result<f64, ArithmeticError> {
        if matches!(self, Self::Div | Self::Rem) && rhs == 0.0 {
            return Err(ArithmeticError::DivisionByZero);
        }

        Ok(match self {
            Self::Add => lhs + rhs,
            Self::Sub => lhs - rhs,
            Self::Mul => lhs * rhs,
            Self::Div => lhs / rhs,
            Self::Rem => lhs % rhs,
        })
    }
}

impl FloatArithmetic for Price {
    fn as_f64_checked(&self) -> Result<f64, ArithmeticError> {
        check_float_precision(self.precision)?;
        Ok(self.as_f64())
    }
}

impl FloatArithmetic for Quantity {
    fn as_f64_checked(&self) -> Result<f64, ArithmeticError> {
        check_float_precision(self.precision)?;
        Ok(self.as_f64())
    }
}

impl FloatArithmetic for Money {
    fn as_f64_checked(&self) -> Result<f64, ArithmeticError> {
        check_float_precision(self.currency.precision)?;
        Ok(self.as_f64())
    }
}

pub(super) fn check_raw_scales(
    lhs_precision: u8,
    rhs_precision: u8,
) -> Result<(), ArithmeticError> {
    if raw_scales_match(lhs_precision, rhs_precision) {
        Ok(())
    } else {
        Err(ArithmeticError::IncompatibleOperands)
    }
}

pub(super) fn extract_arithmetic_decimal(value: &Bound<'_, PyAny>) -> Option<Decimal> {
    // PyO3's Decimal conversion falls back to string parsing, so reject domain types before
    // their numeric Display output can make cross-type arithmetic appear valid.
    if value.is_instance_of::<Price>()
        || value.is_instance_of::<Quantity>()
        || value.is_instance_of::<Money>()
    {
        None
    } else {
        value.extract().ok()
    }
}

fn check_float_precision(precision: u8) -> Result<(), ArithmeticError> {
    if precision <= MAX_FLOAT_PRECISION {
        Ok(())
    } else {
        Err(ArithmeticError::InvalidFloatPrecision(precision))
    }
}

impl From<ArithmeticError> for PyErr {
    fn from(error: ArithmeticError) -> Self {
        match error {
            ArithmeticError::Overflow => {
                PyOverflowError::new_err("Fixed-point arithmetic overflow")
            }
            ArithmeticError::DivisionByZero => {
                PyZeroDivisionError::new_err("Division or modulo by zero")
            }
            ArithmeticError::IncompatibleOperands => {
                to_pyvalue_err("Incompatible fixed-point scales")
            }
            ArithmeticError::InvalidFloatPrecision(precision) => to_pyvalue_err(format!(
                "Fixed-point precision {precision} exceeds maximum float precision {MAX_FLOAT_PRECISION}"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use proptest::{prelude::*, test_runner::Config as ProptestConfig};
    use pyo3::{Python, types::PyTypeMethods};
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    use super::{ArithmeticError, ArithmeticOperation, check_float_precision, check_raw_scales};
    use crate::types::fixed::{FIXED_PRECISION, MAX_FLOAT_PRECISION};

    #[rstest]
    fn test_check_raw_scales_accepts_matching_effective_scales() {
        assert_eq!(check_raw_scales(0, FIXED_PRECISION), Ok(()));
    }

    #[rstest]
    fn test_check_raw_scales_rejects_mixed_effective_scales() {
        assert_eq!(
            check_raw_scales(FIXED_PRECISION, FIXED_PRECISION + 1),
            Err(ArithmeticError::IncompatibleOperands)
        );
    }

    #[rstest]
    #[case(ArithmeticOperation::Add, dec!(1.25), dec!(2.75), dec!(4.00))]
    #[case(ArithmeticOperation::Sub, dec!(4.00), dec!(1.25), dec!(2.75))]
    #[case(ArithmeticOperation::Mul, dec!(1.25), dec!(2), dec!(2.50))]
    #[case(ArithmeticOperation::Div, dec!(5), dec!(2), dec!(2.5))]
    #[case(ArithmeticOperation::Rem, dec!(5), dec!(2), dec!(1))]
    fn test_checked_decimal_succeeds(
        #[case] operation: ArithmeticOperation,
        #[case] lhs: Decimal,
        #[case] rhs: Decimal,
        #[case] expected: Decimal,
    ) {
        assert_eq!(operation.checked_decimal(lhs, rhs), Ok(expected));
    }

    #[rstest]
    #[case(ArithmeticOperation::Add, Decimal::MAX, Decimal::MAX)]
    #[case(ArithmeticOperation::Sub, Decimal::MIN, Decimal::MAX)]
    #[case(ArithmeticOperation::Mul, Decimal::MAX, Decimal::TWO)]
    #[case(ArithmeticOperation::Div, Decimal::MAX, dec!(0.1))]
    fn test_checked_decimal_overflow_returns_error(
        #[case] operation: ArithmeticOperation,
        #[case] lhs: Decimal,
        #[case] rhs: Decimal,
    ) {
        assert_eq!(
            operation.checked_decimal(lhs, rhs),
            Err(ArithmeticError::Overflow)
        );
    }

    #[rstest]
    #[case(ArithmeticOperation::Add, Decimal::MIN, Decimal::MIN)]
    #[case(ArithmeticOperation::Sub, Decimal::MAX, Decimal::MIN)]
    #[case(ArithmeticOperation::Mul, Decimal::MIN, Decimal::TWO)]
    #[case(ArithmeticOperation::Div, Decimal::MIN, dec!(0.1))]
    fn test_checked_decimal_negative_overflow_returns_error(
        #[case] operation: ArithmeticOperation,
        #[case] lhs: Decimal,
        #[case] rhs: Decimal,
    ) {
        assert_eq!(
            operation.checked_decimal(lhs, rhs),
            Err(ArithmeticError::Overflow)
        );
    }

    #[rstest]
    #[case(ArithmeticOperation::Div)]
    #[case(ArithmeticOperation::Rem)]
    fn test_checked_decimal_zero_divisor_returns_error(#[case] operation: ArithmeticOperation) {
        assert_eq!(
            operation.checked_decimal(Decimal::ONE, Decimal::ZERO),
            Err(ArithmeticError::DivisionByZero)
        );
    }

    #[rstest]
    #[case(ArithmeticOperation::Add, 1.25, 2.75, 4.0)]
    #[case(ArithmeticOperation::Sub, 4.0, 1.25, 2.75)]
    #[case(ArithmeticOperation::Mul, 1.25, 2.0, 2.5)]
    #[case(ArithmeticOperation::Div, 5.0, 2.0, 2.5)]
    #[case(ArithmeticOperation::Rem, 5.0, 2.0, 1.0)]
    fn test_checked_f64_succeeds(
        #[case] operation: ArithmeticOperation,
        #[case] lhs: f64,
        #[case] rhs: f64,
        #[case] expected: f64,
    ) {
        assert_eq!(operation.checked_f64(lhs, rhs), Ok(expected));
    }

    #[rstest]
    #[case(ArithmeticOperation::Div, 0.0)]
    #[case(ArithmeticOperation::Div, -0.0)]
    #[case(ArithmeticOperation::Rem, 0.0)]
    #[case(ArithmeticOperation::Rem, -0.0)]
    fn test_checked_f64_zero_divisor_returns_error(
        #[case] operation: ArithmeticOperation,
        #[case] divisor: f64,
    ) {
        assert_eq!(
            operation.checked_f64(1.0, divisor),
            Err(ArithmeticError::DivisionByZero)
        );
    }

    #[rstest]
    #[case(
        ArithmeticError::Overflow,
        "OverflowError",
        "Fixed-point arithmetic overflow"
    )]
    #[case(
        ArithmeticError::DivisionByZero,
        "ZeroDivisionError",
        "Division or modulo by zero"
    )]
    #[case(
        ArithmeticError::IncompatibleOperands,
        "ValueError",
        "Incompatible fixed-point scales"
    )]
    #[case(
        ArithmeticError::InvalidFloatPrecision(18),
        "ValueError",
        "Fixed-point precision 18 exceeds maximum float precision 16"
    )]
    fn test_arithmetic_error_maps_to_exact_python_exception(
        #[case] error: ArithmeticError,
        #[case] expected_type: &str,
        #[case] expected_message: &str,
    ) {
        ensure_python_initialized();

        Python::attach(|py| {
            let py_err = pyo3::PyErr::from(error);

            assert_eq!(py_err.get_type(py).name().unwrap(), expected_type);
            assert_eq!(py_err.value(py).to_string(), expected_message);
        });
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(4_096))]

        #[rstest]
        fn prop_checked_decimal_add_sub_mul_match_integer_oracle(
            lhs in -1_000_000_000_i64..=1_000_000_000,
            rhs in -1_000_000_000_i64..=1_000_000_000,
            operation in add_sub_mul_strategy(),
        ) {
            let expected_mantissa = match operation {
                ArithmeticOperation::Add => i128::from(lhs) + i128::from(rhs),
                ArithmeticOperation::Sub => i128::from(lhs) - i128::from(rhs),
                ArithmeticOperation::Mul => i128::from(lhs) * i128::from(rhs),
                _ => unreachable!("strategy only generates add, sub, and mul"),
            };
            let expected = Decimal::from_i128_with_scale(expected_mantissa, 0);

            prop_assert_eq!(
                operation.checked_decimal(Decimal::from(lhs), Decimal::from(rhs)),
                Ok(expected)
            );
        }

        #[rstest]
        fn prop_checked_decimal_div_matches_exact_integer_oracle(
            quotient in -1_000_000_i64..=1_000_000,
            divisor in nonzero_i64_strategy(),
        ) {
            let dividend = i128::from(quotient) * i128::from(divisor);

            prop_assert_eq!(
                ArithmeticOperation::Div.checked_decimal(
                    Decimal::from_i128_with_scale(dividend, 0),
                    Decimal::from(divisor),
                ),
                Ok(Decimal::from(quotient))
            );
        }

        #[rstest]
        fn prop_checked_decimal_rem_matches_integer_oracle(
            lhs in any::<i64>(),
            rhs in nonzero_i64_strategy(),
        ) {
            let expected = Decimal::from_i128_with_scale(
                i128::from(lhs) % i128::from(rhs),
                0,
            );

            prop_assert_eq!(
                ArithmeticOperation::Rem.checked_decimal(
                    Decimal::from(lhs),
                    Decimal::from(rhs),
                ),
                Ok(expected)
            );
        }

        #[rstest]
        fn prop_checked_decimal_rejects_zero_at_every_scale(
            scale in 0_u32..=28,
            negative in any::<bool>(),
        ) {
            let zero = Decimal::from_parts(0, 0, 0, negative, scale);

            prop_assert_eq!(
                ArithmeticOperation::Div.checked_decimal(Decimal::ONE, zero),
                Err(ArithmeticError::DivisionByZero)
            );
            prop_assert_eq!(
                ArithmeticOperation::Rem.checked_decimal(Decimal::ONE, zero),
                Err(ArithmeticError::DivisionByZero)
            );
        }

        #[rstest]
        fn prop_checked_f64_matches_native_operator(
            lhs in any::<f64>(),
            rhs in any::<f64>(),
            operation in arithmetic_operation_strategy(),
        ) {
            let result = operation.checked_f64(lhs, rhs);

            if matches!(operation, ArithmeticOperation::Div | ArithmeticOperation::Rem)
                && rhs == 0.0
            {
                prop_assert_eq!(result, Err(ArithmeticError::DivisionByZero));
            } else {
                let actual = result.expect("non-zero float arithmetic should succeed");
                let expected = match operation {
                    ArithmeticOperation::Add => lhs + rhs,
                    ArithmeticOperation::Sub => lhs - rhs,
                    ArithmeticOperation::Mul => lhs * rhs,
                    ArithmeticOperation::Div => lhs / rhs,
                    ArithmeticOperation::Rem => lhs % rhs,
                };

                if expected.is_nan() {
                    prop_assert!(actual.is_nan());
                } else {
                    prop_assert_eq!(actual.to_bits(), expected.to_bits());
                }
            }
        }

        #[rstest]
        fn prop_check_float_precision_matches_supported_range(precision in any::<u8>()) {
            let expected = if precision <= MAX_FLOAT_PRECISION {
                Ok(())
            } else {
                Err(ArithmeticError::InvalidFloatPrecision(precision))
            };

            prop_assert_eq!(check_float_precision(precision), expected);
        }

        #[rstest]
        fn prop_check_raw_scales_matches_effective_scale(
            lhs_precision in any::<u8>(),
            rhs_precision in any::<u8>(),
        ) {
            let expected = if lhs_precision.max(FIXED_PRECISION)
                == rhs_precision.max(FIXED_PRECISION)
            {
                Ok(())
            } else {
                Err(ArithmeticError::IncompatibleOperands)
            };

            prop_assert_eq!(
                check_raw_scales(lhs_precision, rhs_precision),
                expected
            );
        }
    }

    fn add_sub_mul_strategy() -> impl Strategy<Value = ArithmeticOperation> {
        prop_oneof![
            Just(ArithmeticOperation::Add),
            Just(ArithmeticOperation::Sub),
            Just(ArithmeticOperation::Mul),
        ]
    }

    fn arithmetic_operation_strategy() -> impl Strategy<Value = ArithmeticOperation> {
        prop_oneof![
            Just(ArithmeticOperation::Add),
            Just(ArithmeticOperation::Sub),
            Just(ArithmeticOperation::Mul),
            Just(ArithmeticOperation::Div),
            Just(ArithmeticOperation::Rem),
        ]
    }

    fn nonzero_i64_strategy() -> impl Strategy<Value = i64> {
        any::<i64>().prop_filter("non-zero divisor", |value| *value != 0)
    }

    fn ensure_python_initialized() {
        static INIT: Once = Once::new();
        INIT.call_once(Python::initialize);
    }
}
