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

//! Lighter Poseidon2 parameter set, defined verbatim from the Apache-2.0 Go
//! reference implementation [`elliottech/poseidon_crypto`] at the pinned
//! revision recorded in the fixture metadata. Each round constant, the
//! diagonal of the internal MDS matrix, and the round counts must agree
//! byte-for-byte with that reference; the fixture vectors under `test_data/`
//! verify equivalence.
//!
//! - State width `t = 12`, sponge rate `r = 8` and capacity `c = 4`.
//! - S-box exponent `D = 7`.
//! - 8 full external rounds (split 4 + 4 around the partial rounds), 22 partial
//!   internal rounds.
//! - Internal MDS matrix is `M_I = diag(MATRIX_DIAG_12) + J_12` where `J_12` is
//!   the all-ones matrix; the diagonal is taken from the Plonky3 Goldilocks
//!   parameter set the Lighter reference adopts.
//!
//! [`elliottech/poseidon_crypto`]: https://github.com/elliottech/poseidon_crypto

use crate::signing::field::Fp;

/// Sponge state width.
pub const WIDTH: usize = 12;

/// Sponge absorption rate (elements per absorbed block).
pub const RATE: usize = 8;

/// S-box exponent: `x -> x^7`. The S-box itself is hard-coded in the
/// permutation for performance; this constant exists to make the full
/// parameter set explicit alongside the round counts it pairs with.
#[allow(dead_code)]
pub const D: u64 = 7;

/// Number of full (external) rounds.
pub const ROUNDS_F: usize = 8;

/// Half the number of full rounds, applied before and after the partial rounds.
pub const ROUNDS_F_HALF: usize = ROUNDS_F / 2;

/// Number of partial (internal) rounds.
pub const ROUNDS_P: usize = 22;

/// Round constants applied at each external round (one row per round, one
/// constant per state position).
pub const EXTERNAL_CONSTANTS: [[Fp; WIDTH]; ROUNDS_F] = [
    [
        Fp::from_u64_reduce(15_492_826_721_047_263_190),
        Fp::from_u64_reduce(11_728_330_187_201_910_315),
        Fp::from_u64_reduce(8_836_021_247_773_420_868),
        Fp::from_u64_reduce(16_777_404_051_263_952_451),
        Fp::from_u64_reduce(5_510_875_212_538_051_896),
        Fp::from_u64_reduce(6_173_089_941_271_892_285),
        Fp::from_u64_reduce(2_927_757_366_422_211_339),
        Fp::from_u64_reduce(10_340_958_981_325_008_808),
        Fp::from_u64_reduce(8_541_987_352_684_552_425),
        Fp::from_u64_reduce(9_739_599_543_776_434_497),
        Fp::from_u64_reduce(15_073_950_188_101_532_019),
        Fp::from_u64_reduce(12_084_856_431_752_384_512),
    ],
    [
        Fp::from_u64_reduce(4_584_713_381_960_671_270),
        Fp::from_u64_reduce(8_807_052_963_476_652_830),
        Fp::from_u64_reduce(54_136_601_502_601_741),
        Fp::from_u64_reduce(4_872_702_333_905_478_703),
        Fp::from_u64_reduce(5_551_030_319_979_516_287),
        Fp::from_u64_reduce(12_889_366_755_535_460_989),
        Fp::from_u64_reduce(16_329_242_193_178_844_328),
        Fp::from_u64_reduce(412_018_088_475_211_848),
        Fp::from_u64_reduce(10_505_784_623_379_650_541),
        Fp::from_u64_reduce(9_758_812_378_619_434_837),
        Fp::from_u64_reduce(7_421_979_329_386_275_117),
        Fp::from_u64_reduce(375_240_370_024_755_551),
    ],
    [
        Fp::from_u64_reduce(3_331_431_125_640_721_931),
        Fp::from_u64_reduce(15_684_937_309_956_309_981),
        Fp::from_u64_reduce(578_521_833_432_107_983),
        Fp::from_u64_reduce(14_379_242_000_670_861_838),
        Fp::from_u64_reduce(17_922_409_828_154_900_976),
        Fp::from_u64_reduce(8_153_494_278_429_192_257),
        Fp::from_u64_reduce(15_904_673_920_630_731_971),
        Fp::from_u64_reduce(11_217_863_998_460_634_216),
        Fp::from_u64_reduce(3_301_540_195_510_742_136),
        Fp::from_u64_reduce(9_937_973_023_749_922_003),
        Fp::from_u64_reduce(3_059_102_938_155_026_419),
        Fp::from_u64_reduce(1_895_288_289_490_976_132),
    ],
    [
        Fp::from_u64_reduce(5_580_912_693_628_927_540),
        Fp::from_u64_reduce(10_064_804_080_494_788_323),
        Fp::from_u64_reduce(9_582_481_583_369_602_410),
        Fp::from_u64_reduce(10_186_259_561_546_797_986),
        Fp::from_u64_reduce(247_426_333_829_703_916),
        Fp::from_u64_reduce(13_193_193_905_461_376_067),
        Fp::from_u64_reduce(6_386_232_593_701_758_044),
        Fp::from_u64_reduce(17_954_717_245_501_896_472),
        Fp::from_u64_reduce(1_531_720_443_376_282_699),
        Fp::from_u64_reduce(2_455_761_864_255_501_970),
        Fp::from_u64_reduce(11_234_429_217_864_304_495),
        Fp::from_u64_reduce(4_746_959_618_548_874_102),
    ],
    [
        Fp::from_u64_reduce(13_571_697_342_473_846_203),
        Fp::from_u64_reduce(17_477_857_865_056_504_753),
        Fp::from_u64_reduce(15_963_032_953_523_553_760),
        Fp::from_u64_reduce(16_033_593_225_279_635_898),
        Fp::from_u64_reduce(14_252_634_232_868_282_405),
        Fp::from_u64_reduce(8_219_748_254_835_277_737),
        Fp::from_u64_reduce(7_459_165_569_491_914_711),
        Fp::from_u64_reduce(15_855_939_513_193_752_003),
        Fp::from_u64_reduce(16_788_866_461_340_278_896),
        Fp::from_u64_reduce(7_102_224_659_693_946_577),
        Fp::from_u64_reduce(3_024_718_005_636_976_471),
        Fp::from_u64_reduce(13_695_468_978_618_890_430),
    ],
    [
        Fp::from_u64_reduce(8_214_202_050_877_825_436),
        Fp::from_u64_reduce(2_670_727_992_739_346_204),
        Fp::from_u64_reduce(16_259_532_062_589_659_211),
        Fp::from_u64_reduce(11_869_922_396_257_088_411),
        Fp::from_u64_reduce(3_179_482_916_972_760_137),
        Fp::from_u64_reduce(13_525_476_046_633_427_808),
        Fp::from_u64_reduce(3_217_337_278_042_947_412),
        Fp::from_u64_reduce(14_494_689_598_654_046_340),
        Fp::from_u64_reduce(15_837_379_330_312_175_383),
        Fp::from_u64_reduce(8_029_037_639_801_151_344),
        Fp::from_u64_reduce(2_153_456_285_263_517_937),
        Fp::from_u64_reduce(8_301_106_462_311_849_241),
    ],
    [
        Fp::from_u64_reduce(13_294_194_396_455_217_955),
        Fp::from_u64_reduce(17_394_768_489_610_594_315),
        Fp::from_u64_reduce(12_847_609_130_464_867_455),
        Fp::from_u64_reduce(14_015_739_446_356_528_640),
        Fp::from_u64_reduce(5_879_251_655_839_607_853),
        Fp::from_u64_reduce(9_747_000_124_977_436_185),
        Fp::from_u64_reduce(8_950_393_546_890_284_269),
        Fp::from_u64_reduce(10_765_765_936_405_694_368),
        Fp::from_u64_reduce(14_695_323_910_334_139_959),
        Fp::from_u64_reduce(16_366_254_691_123_000_864),
        Fp::from_u64_reduce(15_292_774_414_889_043_182),
        Fp::from_u64_reduce(10_910_394_433_429_313_384),
    ],
    [
        Fp::from_u64_reduce(17_253_424_460_214_596_184),
        Fp::from_u64_reduce(3_442_854_447_664_030_446),
        Fp::from_u64_reduce(3_005_570_425_335_613_727),
        Fp::from_u64_reduce(10_859_158_614_900_201_063),
        Fp::from_u64_reduce(9_763_230_642_109_343_539),
        Fp::from_u64_reduce(6_647_722_546_511_515_039),
        Fp::from_u64_reduce(909_012_944_955_815_706),
        Fp::from_u64_reduce(18_101_204_076_790_399_111),
        Fp::from_u64_reduce(11_588_128_829_349_125_809),
        Fp::from_u64_reduce(15_863_878_496_612_806_566),
        Fp::from_u64_reduce(5_201_119_062_417_750_399),
        Fp::from_u64_reduce(176_665_553_780_565_743),
    ],
];

/// Round constants applied to `state[0]` at each partial (internal) round.
pub const INTERNAL_CONSTANTS: [Fp; ROUNDS_P] = [
    Fp::from_u64_reduce(11_921_381_764_981_422_944),
    Fp::from_u64_reduce(10_318_423_381_711_320_787),
    Fp::from_u64_reduce(8_291_411_502_347_000_766),
    Fp::from_u64_reduce(229_948_027_109_387_563),
    Fp::from_u64_reduce(9_152_521_390_190_983_261),
    Fp::from_u64_reduce(7_129_306_032_690_285_515),
    Fp::from_u64_reduce(15_395_989_607_365_232_011),
    Fp::from_u64_reduce(8_641_397_269_074_305_925),
    Fp::from_u64_reduce(17_256_848_792_241_043_600),
    Fp::from_u64_reduce(6_046_475_228_902_245_682),
    Fp::from_u64_reduce(12_041_608_676_381_094_092),
    Fp::from_u64_reduce(12_785_542_378_683_951_657),
    Fp::from_u64_reduce(14_546_032_085_337_914_034),
    Fp::from_u64_reduce(3_304_199_118_235_116_851),
    Fp::from_u64_reduce(16_499_627_707_072_547_655),
    Fp::from_u64_reduce(10_386_478_025_625_759_321),
    Fp::from_u64_reduce(13_475_579_315_436_919_170),
    Fp::from_u64_reduce(16_042_710_511_297_532_028),
    Fp::from_u64_reduce(1_411_266_850_385_657_080),
    Fp::from_u64_reduce(9_024_840_976_168_649_958),
    Fp::from_u64_reduce(14_047_056_970_978_379_368),
    Fp::from_u64_reduce(838_728_605_080_212_101),
];

/// Diagonal of the internal MDS matrix (the off-diagonal entries are all 1).
///
/// Sourced from the Plonky3 Poseidon2 Goldilocks parameter set the Lighter
/// reference adopts.
pub const MATRIX_DIAG_12: [Fp; WIDTH] = [
    Fp::from_u64_reduce(0xc3b6_c08e_23ba_9300),
    Fp::from_u64_reduce(0xd84b_5de9_4a32_4fb6),
    Fp::from_u64_reduce(0x0d0c_371c_5b35_b84f),
    Fp::from_u64_reduce(0x7964_f570_e718_8037),
    Fp::from_u64_reduce(0x5daf_18bb_d996_604b),
    Fp::from_u64_reduce(0x6743_bc47_b959_5257),
    Fp::from_u64_reduce(0x5528_b936_2c59_bb70),
    Fp::from_u64_reduce(0xac45_e25b_7127_b68b),
    Fp::from_u64_reduce(0xa207_7d7d_fbb6_06b5),
    Fp::from_u64_reduce(0xf3fa_ac6f_aee3_78ae),
    Fp::from_u64_reduce(0x0c63_88b5_1545_e883),
    Fp::from_u64_reduce(0xd27d_bb69_4491_7b60),
];
