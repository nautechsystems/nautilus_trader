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
