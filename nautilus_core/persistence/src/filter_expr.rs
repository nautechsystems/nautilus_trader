// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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





use arrow2::scalar::{Scalar, Utf8Scalar, PrimitiveScalar};
use arrow2::array::{Array,PrimitiveArray, Utf8Array, BooleanArray, Offset};
use arrow2::types::NativeType;
use arrow2::datatypes::DataType;
use arrow2::compute::comparison::{
    eq_scalar,
    neq_scalar,
    gt_scalar,
    gt_eq_scalar,
    lt_scalar,
    lt_eq_scalar,
};
use arrow2::error::Result as ArrowResult;
use arrow2::compute::filter::filter;
use num_traits::ToPrimitive;
use num_traits::cast::NumCast;

///////////////////////////////////////////////////
#[derive(Debug, Copy, Clone, PartialEq)]
enum Operator { EQ, NE, GE, GT, LE, LT }
impl From<&str> for Operator {
    fn from(s: &str) -> Self {
        match s {
            "==" => Operator::EQ,
            "!=" => Operator::NE,
            ">" => Operator::GT,
            ">=" => Operator::GE,
            "<" => Operator::LT,
            "<=" => Operator::LE,
            _ => panic!("Invalid `Expression Operator` value, was {s}"),
        }
    }
}
///////////////////////////////////////////////////
/// 
#[derive(Debug)]
struct Expression {
    lhs: String,
    operator: Operator,
    rhs: Box<dyn Scalar>,
}

trait ExpressionCalc<T> {
    fn filter(&self, expression: &Expression) -> Result<Box<dyn Array>, &'static str>;
}

// Filter a Primitive Array using an Expression
impl<T: NativeType + NumCast> ExpressionCalc<T> for PrimitiveArray<T> 
{
    fn filter(&self, expression: &Expression) -> Result<Box<dyn Array>, &'static str>
    {   
        expression.to::<T>()
        .ok_or::<&'static str>("Unsafe Cast")
        .unwrap()
        .filter_array(self)
        .map_err(|_| "Error during Expression.filter_array")
    }
}

// Filter a Utf8Array using an Expression
impl<T: Offset> ExpressionCalc<T> for Utf8Array<T>
{     
    fn filter(&self, expression: &Expression) -> Result<Box<dyn Array>, &'static str>
    {   
        expression
        .filter_array(self)
        .map_err(|_| "Error during Expression.filter_array")
    }
}

impl Expression {
    
    fn filter_array(&self, array: &dyn Array) -> ArrowResult<Box<dyn Array>> {
        let filter_mask = self.compute_mask(array);
        filter(array, &filter_mask)
    }
    fn compute_mask(&self, array: &dyn Array) -> BooleanArray {
        let scalar = self.rhs.as_ref();
        match self.operator {
            Operator::EQ => eq_scalar(array, scalar),
            Operator::NE => neq_scalar(array, scalar),
            Operator::GT => gt_scalar(array, scalar),
            Operator::GE => gt_eq_scalar(array, scalar),
            Operator::LT => lt_scalar(array, scalar),
            Operator::LE => lt_eq_scalar(array, scalar)
        }
    }
    fn to<T>(&self) -> Option<Self>
    where
        T: NativeType + NumCast,
    {

        // Convert RHS to new type.
        let value = match self.rhs.data_type() {
            DataType::Int8 => {self.cast_rhs::<i8, T>()},
            DataType::Int16 => {self.cast_rhs::<i16, T>()},
            DataType::Int32 => {self.cast_rhs::<i32, T>()},
            DataType::Int64 => {self.cast_rhs::<i64, T>()},
            DataType::UInt8 => {self.cast_rhs::<u8, T>()},
            DataType::UInt32 => {self.cast_rhs::<u32, T>()},
            DataType::UInt64 => {self.cast_rhs::<u64, T>()},
            DataType::Float32 => {self.cast_rhs::<f32, T>()},
            DataType::Float64 => {self.cast_rhs::<f64, T>()},
            _ => todo!(),
        }; 
        
        Some(
            Expression {
                lhs: self.lhs.clone(),
                operator: self.operator,
                rhs: Box::new(PrimitiveScalar::<T>::from(value))
            }
        )

    }

    fn cast_rhs<U, T>(&self) -> Option<T> 
    where
        U: NativeType + ToPrimitive,
        T: NativeType + NumCast,
    {
        let value = (*self.rhs)
                    .as_any()
                    .downcast_ref::<PrimitiveScalar<U>>()
                    .unwrap()
                    .value()
                    .unwrap();
        T::from::<U>(value)
    }
    
 
}
     
impl TryFrom<&String> for Expression {

    type Error = &'static str;

    fn try_from(value: &String) -> std::result::Result<Self, Self::Error> {
        
        let parts = value.trim()
                        .trim_start_matches("(")
                        .trim_end_matches(")")
                        .split(" ")
                        .map(|s|s.trim())
                        .collect::<Vec<&str>>();

        if parts.len() != 3 {
            return Err("Invalid count of items found in expression_str.")
        }

        let lhs = parts[0].trim_start_matches("\"").trim_end_matches("\"");
        
        let operator = Operator::from(parts[1]);
        
        let rhs = parts[2];

        let scalar = string_to_scalar(rhs)?;

        // Check operator is valid for Utf8
        if scalar.data_type().to_owned() == DataType::Utf8 {
            let invalid_operator = operator != Operator::EQ && operator != Operator::NE;
            if invalid_operator { 
                return Err("Invalid operator for Utf8 Expression. Valid operators are EQ, NE.")
            }
        }

        return Ok(Expression { 
            lhs: lhs.to_string(),
            operator: operator,
            rhs: scalar
        })
    }

    
}

fn string_to_scalar(value: &str) -> std::result::Result<Box<dyn Scalar>, &'static str>
{
    let value = value.trim();
    // Parse the string to a Scalar object.
    if is_quoted_string(value) { 
        return Ok(Box::new(Utf8Scalar::<i64>::from(Some(value))));
    }

    let is_numeric_string = value.replacen(".", "", 1).chars().all(char::is_numeric);
    if !is_numeric_string {
        return Err("Failed to parse numeric value as string.")
    }

    let is_float = value.contains(".");
    if is_float {
        Ok(Box::new(PrimitiveScalar::from(Some(value.parse::<f64>().unwrap()))))
    } else { // is integer
        Ok(Box::new(PrimitiveScalar::from(Some(value.parse::<i64>().unwrap()))))
    }
}

pub fn is_quoted_string(s: &str) -> bool {
    s.trim_start_matches("\"") != s && s.trim_end_matches("\"") != s
}

///////////////////////////////////////////////////


#[cfg(test)]
mod tests {
    use super::*;
    use arrow2::array::{UInt64Array};
        
    
    #[test]
    fn test_primitive_array_operator_returns_expected() {
        let test_data = [
            (Operator::EQ, &PrimitiveArray::<u64>::from_slice(vec![4])),
            (Operator::NE, &PrimitiveArray::<u64>::from_slice(vec![1, 2, 3, 5])),
            (Operator::GT, &PrimitiveArray::<u64>::from_slice(vec![5])),
            (Operator::GE, &PrimitiveArray::<u64>::from_slice(vec![4, 5])),
            (Operator::LT, &PrimitiveArray::<u64>::from_slice(vec![1, 2, 3])),
            (Operator::LE, &PrimitiveArray::<u64>::from_slice(vec![1, 2, 3, 4])),
        ];
        for data in test_data {
            let (operator, expected) = data;
            let expression = Expression {
                lhs: "some_field".to_owned(),
                operator: operator,
                rhs: Box::new(PrimitiveScalar::<u64>::from(Some(4 as u64)))
            };
            let result = UInt64Array::from_vec(vec![1, 2, 3, 4, 5]).filter(&expression).unwrap();
            let result = result.as_any().downcast_ref::<PrimitiveArray<u64>>().unwrap();
            assert_eq!(result, expected);
        };
    }

    #[allow(dead_code)]
    fn test_utf8_array_operator_returns_expected() {
        todo!()
    }

    #[allow(dead_code)]
    fn test_expression_from_str() {
        todo!()
    }
    
    #[allow(dead_code)]
    fn test_utf8_expression_from_str_invalid_operator_raises() {
        todo!()
    }
    
    #[allow(dead_code)]
    fn test_primitive_array_unsafe_cast_raises() {
        todo!()
    }

