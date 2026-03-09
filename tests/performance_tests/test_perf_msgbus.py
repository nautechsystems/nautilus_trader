import random

from nautilus_trader.common.component import is_matching_py


def generate_topics(n: int, seed: int) -> list[str]:
    random.seed(seed)

    cat = ["data", "info", "order"]
    model = ["quotes", "trades", "orderbooks", "depths"]
    venue = ["BINANCE", "BYBIT", "OKX", "FTX", "KRAKEN"]
    instrument = ["BTCUSDT", "ETHUSDT", "SOLUSDT", "XRPUSDT", "DOGEUSDT"]

    topics: list[str] = []

    for _ in range(n):
        c = random.choice(cat)  # noqa: S311
        m = random.choice(model)  # noqa: S311
        v = random.choice(venue)  # noqa: S311
        i = random.choice(instrument)  # noqa: S311
        topics.append(f"{c}.{m}.{v}.{i}")

    return topics


def test_topic_pattern_matching(benchmark) -> None:
    topics = generate_topics(1000, 42)
    pattern = "data.*.BINANCE.ETH???"

    def match_topics():
        for topic in topics:
            is_matching_py(pattern, topic)

    benchmark(match_topics)
