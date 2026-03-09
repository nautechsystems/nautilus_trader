use iai::black_box;
use nautilus_common::msgbus::matching::{is_matching, is_matching_backtracking};

fn bench_matching_exact() {
    let topic = b"data.quotes.BINANCE.ETHUSDT";
    let pattern = b"data.quotes.BINANCE.ETHUSDT";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_star_end() {
    let topic = b"data.quotes.BINANCE.ETHUSDT";
    let pattern = b"data.quotes.BINANCE.*";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_star_middle() {
    let topic = b"data.quotes.BINANCE.ETHUSDT";
    let pattern = b"data.*.BINANCE.ETHUSDT";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_multi_star() {
    let topic = b"data.quotes.BINANCE.ETHUSDT";
    let pattern = b"data.*.BINANCE.*";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_question() {
    let topic = b"data.quotes.BINANCE.ETHUSD";
    let pattern = b"data.quotes.BINANCE.ETHUS?";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_multi_question() {
    let topic = b"data.quotes.BINANCE.ETHUSD";
    let pattern = b"data.quotes.BINANCE.ETH???";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_mixed() {
    let topic = b"data.quotes.BINANCE.ETHUSD";
    let pattern = b"data.*.BINANCE.ETH???";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_realistic() {
    let topic = b"data.trades.BINANCE.ETHUSDT";
    let pattern = b"data.*.BINANCE.ETH*";
    black_box(is_matching(topic, pattern));
}

fn bench_matching_no_match() {
    let topic = b"data.quotes.BINANCE.ETHUSDT";
    let pattern = b"data.trades.BYBIT.*";
    black_box(is_matching(topic, pattern));
}

fn bench_mstr_matching_exact() {
    black_box(is_matching_backtracking(
        "data.quotes.BINANCE.ETHUSDT".into(),
        "data.quotes.BINANCE.ETHUSDT".into(),
    ));
}

fn bench_mstr_matching_star_middle() {
    black_box(is_matching_backtracking(
        "data.quotes.BINANCE.ETHUSDT".into(),
        "data.*.BINANCE.ETHUSDT".into(),
    ));
}

fn bench_mstr_matching_mixed() {
    black_box(is_matching_backtracking(
        "data.quotes.BINANCE.ETHUSD".into(),
        "data.*.BINANCE.ETH???".into(),
    ));
}

fn bench_mstr_matching_realistic() {
    black_box(is_matching_backtracking(
        "data.trades.BINANCE.ETHUSDT".into(),
        "data.*.BINANCE.ETH*".into(),
    ));
}

iai::main!(
    bench_matching_exact,
    bench_matching_star_end,
    bench_matching_star_middle,
    bench_matching_multi_star,
    bench_matching_question,
    bench_matching_multi_question,
    bench_matching_mixed,
    bench_matching_realistic,
    bench_matching_no_match,
    bench_mstr_matching_exact,
    bench_mstr_matching_star_middle,
    bench_mstr_matching_mixed,
    bench_mstr_matching_realistic,
);
