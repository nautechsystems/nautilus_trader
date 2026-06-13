use nautilus_model::types::{Price, Quantity};
use rstest::rstest;

use super::{super::*, support::*};

#[rstest]
#[case::p3_s2("token-a", Price::from("0.001"), Quantity::from("0.01"))]
#[case::p5_s4("token-b", Price::from("0.00001"), Quantity::from("0.0001"))]
fn cache_instrument_writes_both_maps(
    #[case] raw_symbol: &str,
    #[case] price_increment: Price,
    #[case] size_increment: Quantity,
) {
    let instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>> = Arc::new(AtomicMap::new());
    let token_meta: Arc<DashMap<Ustr, TokenMeta>> = Arc::new(DashMap::new());
    let inst = stub_instrument(raw_symbol, price_increment, size_increment);
    let expected_id = inst.id();
    let expected_token = Ustr::from(raw_symbol);
    let expected_price_precision = price_increment.precision;
    let expected_size_precision = size_increment.precision;

    cache_instrument(&instruments, &token_meta, &inst);

    let loaded = instruments.load();
    let cached = loaded
        .get(&expected_id)
        .expect("instrument inserted into live cache");
    assert_eq!(cached.id(), expected_id);
    assert_eq!(cached.raw_symbol().as_str(), raw_symbol);

    let meta = token_meta
        .get(&expected_token)
        .expect("token_meta inserted for raw_symbol");
    assert_eq!(meta.instrument_id, expected_id);
    assert_eq!(meta.price_precision, expected_price_precision);
    assert_eq!(meta.size_precision, expected_size_precision);
}

#[rstest]
fn cache_instrument_overwrites_precisions_on_second_call() {
    let instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>> = Arc::new(AtomicMap::new());
    let token_meta: Arc<DashMap<Ustr, TokenMeta>> = Arc::new(DashMap::new());
    let raw_symbol = "token-overwrite";

    let first = stub_instrument(raw_symbol, Price::from("0.01"), Quantity::from("0.1"));
    cache_instrument(&instruments, &token_meta, &first);

    let second = stub_instrument(raw_symbol, Price::from("0.0001"), Quantity::from("0.001"));
    cache_instrument(&instruments, &token_meta, &second);

    let meta = token_meta
        .get(&Ustr::from(raw_symbol))
        .expect("token_meta present after overwrite");
    assert_eq!(meta.price_precision, 4);
    assert_eq!(meta.size_precision, 3);
    assert_eq!(token_meta.len(), 1);
    assert_eq!(instruments.load().len(), 1);
}

#[rstest]
fn cache_instrument_maintains_dual_cache_invariant() {
    let instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>> = Arc::new(AtomicMap::new());
    let token_meta: Arc<DashMap<Ustr, TokenMeta>> = Arc::new(DashMap::new());

    let samples = [
        stub_instrument("token-1", Price::from("0.001"), Quantity::from("0.01")),
        stub_instrument("token-2", Price::from("0.0001"), Quantity::from("0.01")),
        stub_instrument("token-3", Price::from("0.00001"), Quantity::from("0.001")),
    ];

    for inst in &samples {
        cache_instrument(&instruments, &token_meta, inst);
    }

    let loaded = instruments.load();
    assert_eq!(loaded.len(), samples.len());
    for inst in loaded.values() {
        let token_id = Ustr::from(inst.raw_symbol().as_str());
        let meta = token_meta
            .get(&token_id)
            .unwrap_or_else(|| panic!("missing token_meta for {token_id}"));
        assert_eq!(meta.instrument_id, inst.id());
    }
}
