//WIP: Filter the arrays according to the filter_expr(s) argument.
use arrow2;
use arrow2::{
    array::{UInt64Array},
    datatypes::DataType,
};

use arrow2::scalar::PrimitiveScalar;
use arrow2::compute::comparison::primitive::eq_scalar;

fn main() {
    let bid = UInt64Array::from_vec(vec![1, 2, 3, 4, 5]);
    let boolean_mask = eq_scalar(&bid, 2 as u64);
}