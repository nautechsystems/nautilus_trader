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

//! Cachegrind-stable instruction counts for the inner signing primitives.
//!
//! `Fp::mul`, `Fp5::mul`, `Fp5::invert`, and the curve `add` / `double` /
//! `lookup_ct` calls are invoked tens of thousands of times per signature.
//! Wall-clock criterion noise hides 1-2% regressions in any of them; iai
//! catches them deterministically by counting executed instructions.

use iai::black_box;
use nautilus_lighter::signing::curve::{AffinePoint, Point, Scalar, lookup_ct};

mod common;
use common::{fixed_k, fp_inputs, fp5_inputs};

fn bench_fp_mul() {
    let (a, b) = fp_inputs();
    black_box(black_box(a) * black_box(b));
}

fn bench_fp_square() {
    let (a, _) = fp_inputs();
    black_box(black_box(a).square());
}

fn bench_fp_invert() {
    let (a, _) = fp_inputs();
    black_box(black_box(a).invert());
}

fn bench_fp5_mul() {
    let (a, b) = fp5_inputs();
    black_box(black_box(a) * black_box(b));
}

fn bench_fp5_square() {
    let (a, _) = fp5_inputs();
    black_box(black_box(a).square());
}

fn bench_fp5_invert() {
    let (a, _) = fp5_inputs();
    black_box(black_box(a).invert());
}

fn bench_point_add() {
    let g = Point::GENERATOR;
    let g2 = g.double();
    black_box(black_box(g).add_point(black_box(g2)));
}

fn bench_point_add_affine() {
    let g = Point::GENERATOR;
    let g2 = g.double();
    let g2_affine = AffinePoint {
        x: g2.x * g2.z.invert(),
        u: g2.u * g2.t.invert(),
    };
    black_box(black_box(g).add_affine(black_box(g2_affine)));
}

fn bench_point_double() {
    let g = Point::GENERATOR;
    black_box(black_box(g).double());
}

fn bench_point_mdouble_5() {
    let g = Point::GENERATOR;
    black_box(black_box(g).mdouble(black_box(5)));
}

fn bench_lookup_ct() {
    let win = Point::GENERATOR.make_window_affine();
    black_box(lookup_ct(black_box(&win), black_box(7)));
}

fn bench_scalar_mul_ct() {
    let g = Point::GENERATOR;
    let s: Scalar = fixed_k();
    black_box(black_box(g).scalar_mul_ct(black_box(s)));
}

fn bench_fp5_to_scalar() {
    let (a, _) = fp5_inputs();
    black_box(Scalar::from_fp5(black_box(a)));
}

iai::main!(
    bench_fp_mul,
    bench_fp_square,
    bench_fp_invert,
    bench_fp5_mul,
    bench_fp5_square,
    bench_fp5_invert,
    bench_point_add,
    bench_point_add_affine,
    bench_point_double,
    bench_point_mdouble_5,
    bench_lookup_ct,
    bench_scalar_mul_ct,
    bench_fp5_to_scalar,
);
