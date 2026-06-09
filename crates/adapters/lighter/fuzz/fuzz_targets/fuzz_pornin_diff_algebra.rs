#![no_main]

//! Differential soak for Lighter's field, scalar, and curve algebra against
//! Pornin's upstream Rust reference (`pornin/ecgfp5`, MIT-licensed).
//!
//! This target keeps the git-sourced reference dependency in the publish=false
//! fuzz crate, outside the crates.io-publishable `nautilus-lighter` package
//! graph.

use ecgfp5::{
    curve::Point as PorninPoint, field::GFp5 as PorninFp5, scalar::Scalar as PorninScalar,
};
use libfuzzer_sys::fuzz_target;
use nautilus_lighter::signing::{
    curve::{Point, SCALAR_BYTES, Scalar},
    field::Fp5,
};

const FP5_BYTES: usize = 40;
const INPUT_LEN: usize = 4 * SCALAR_BYTES;

fn fp5_from_slice(bytes: &[u8]) -> Fp5 {
    let mut limbs = [0u64; 5];
    for (i, slot) in limbs.iter_mut().enumerate() {
        let mut chunk = [0u8; 8];
        chunk.copy_from_slice(&bytes[i * 8..(i + 1) * 8]);
        *slot = u64::from_le_bytes(chunk);
    }
    Fp5::from_u64s_reduce(limbs)
}

fn scalar_from_slice(bytes: &[u8]) -> Scalar {
    let mut buf = [0u8; SCALAR_BYTES];
    buf.copy_from_slice(bytes);
    Scalar::from_le_bytes_reduce(buf)
}

fn fp5_to_pornin(ours: Fp5) -> PorninFp5 {
    let (gfp5, ok) = PorninFp5::decode(&ours.to_le_bytes());
    assert_eq!(
        ok,
        u64::MAX,
        "our canonical Fp5 bytes must decode under Pornin's reference",
    );
    gfp5
}

fn scalar_to_pornin(ours: Scalar) -> PorninScalar {
    PorninScalar::decode_reduce(&ours.to_le_bytes())
}

fuzz_target!(|data: &[u8]| {
    if data.len() < INPUT_LEN {
        return;
    }

    let fp5_a = fp5_from_slice(&data[..FP5_BYTES]);
    let fp5_b = fp5_from_slice(&data[FP5_BYTES..2 * FP5_BYTES]);
    let scalar_a = scalar_from_slice(&data[2 * SCALAR_BYTES..3 * SCALAR_BYTES]);
    let scalar_b = scalar_from_slice(&data[3 * SCALAR_BYTES..4 * SCALAR_BYTES]);

    let pornin_fp5_a = fp5_to_pornin(fp5_a);
    let pornin_fp5_b = fp5_to_pornin(fp5_b);
    let pornin_scalar_a = scalar_to_pornin(scalar_a);
    let pornin_scalar_b = scalar_to_pornin(scalar_b);

    assert_eq!(
        (fp5_a + fp5_b).to_le_bytes(),
        (pornin_fp5_a + pornin_fp5_b).encode(),
        "Fp5 add diverged from Pornin reference",
    );
    assert_eq!(
        (fp5_a - fp5_b).to_le_bytes(),
        (pornin_fp5_a - pornin_fp5_b).encode(),
        "Fp5 sub diverged from Pornin reference",
    );
    assert_eq!(
        (fp5_a * fp5_b).to_le_bytes(),
        (pornin_fp5_a * pornin_fp5_b).encode(),
        "Fp5 mul diverged from Pornin reference",
    );
    assert_eq!(
        (-fp5_a).to_le_bytes(),
        (-pornin_fp5_a).encode(),
        "Fp5 neg diverged from Pornin reference",
    );

    if !fp5_a.is_zero() {
        assert_eq!(
            fp5_a.invert().to_le_bytes(),
            pornin_fp5_a.invert().encode(),
            "Fp5 invert diverged from Pornin reference",
        );
    }

    assert_eq!(
        (scalar_a + scalar_b).to_le_bytes(),
        (pornin_scalar_a + pornin_scalar_b).encode(),
        "Scalar add diverged from Pornin reference",
    );
    assert_eq!(
        (scalar_a - scalar_b).to_le_bytes(),
        (pornin_scalar_a - pornin_scalar_b).encode(),
        "Scalar sub diverged from Pornin reference",
    );
    assert_eq!(
        (scalar_a * scalar_b).to_le_bytes(),
        (pornin_scalar_a * pornin_scalar_b).encode(),
        "Scalar mul diverged from Pornin reference",
    );
    assert_eq!(
        (-scalar_a).to_le_bytes(),
        (-pornin_scalar_a).encode(),
        "Scalar neg diverged from Pornin reference",
    );

    let ours_decoded = Point::decode(fp5_a);
    let (pornin_decoded, pornin_ok) = PorninPoint::decode(pornin_fp5_a);
    match (ours_decoded, pornin_ok) {
        (Some(ours), u64::MAX) => assert_eq!(
            ours.encode().to_le_bytes(),
            pornin_decoded.encode().encode(),
            "Point::decode value diverged from Pornin reference",
        ),
        (None, 0) => {}
        (ours, mask) => panic!(
            "Point::decode decision diverged: ours={} theirs_ok={:#x}",
            ours.is_some(),
            mask,
        ),
    }

    let ours_a = Point::GENERATOR * scalar_a;
    let ours_b = Point::GENERATOR * scalar_b;
    let pornin_a = PorninPoint::mulgen(pornin_scalar_a);
    let pornin_b = PorninPoint::mulgen(pornin_scalar_b);

    assert_eq!(
        Point::mulgen(scalar_a).encode().to_le_bytes(),
        pornin_a.encode().encode(),
        "Point::mulgen diverged from Pornin reference",
    );
    assert_eq!(
        Point::mulgen_ct(scalar_a).encode().to_le_bytes(),
        pornin_a.encode().encode(),
        "Point::mulgen_ct diverged from Pornin reference",
    );
    assert_eq!(
        ours_a.double().encode().to_le_bytes(),
        pornin_a.double().encode().encode(),
        "Point::double diverged from Pornin reference",
    );
    assert_eq!(
        (ours_a + ours_b).encode().to_le_bytes(),
        (pornin_a + pornin_b).encode().encode(),
        "Point add diverged from Pornin reference",
    );
    assert_eq!(
        (ours_a * scalar_b).encode().to_le_bytes(),
        (pornin_a * pornin_scalar_b).encode().encode(),
        "Point scalar_mul diverged from Pornin reference",
    );
});
