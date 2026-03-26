from __future__ import annotations

import re
import tomllib
from pathlib import Path

ACTIVE_STRATEGY_CLASS = "maker_v4"
ACTIVE_PARAM_SET = "makerv4"
EQUITIES_MAKER_MAX_QUOTE_AGE_MS = 60_000
EQUITIES_IBKR_MAX_QUOTE_AGE_MS = 300_000
ACTIVE_STRATEGIES = (
    {
        "symbol": "AAPL",
        "strategy_id": "aapl_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "AAPL.NASDAQ",
    },
    {
        "symbol": "AMD",
        "strategy_id": "amd_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:AMD-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "AMD.NASDAQ",
    },
    {
        "symbol": "AMZN",
        "strategy_id": "amzn_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:AMZN-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "AMZN.NASDAQ",
    },
    {
        "symbol": "BABA",
        "strategy_id": "baba_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:BABA-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "BABA.NYSE",
    },
    {
        "symbol": "COIN",
        "strategy_id": "coin_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:COIN-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "COIN.NASDAQ",
    },
    {
        "symbol": "CRCL",
        "strategy_id": "crcl_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:CRCL-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "CRCL.NYSE",
    },
    {
        "symbol": "CRWV",
        "strategy_id": "crwv_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:CRWV-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "CRWV.NASDAQ",
    },
    {
        "symbol": "GOOGL",
        "strategy_id": "googl_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:GOOGL-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "GOOGL.NASDAQ",
    },
    {
        "symbol": "HOOD",
        "strategy_id": "hood_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:HOOD-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "HOOD.NASDAQ",
    },
    {
        "symbol": "INTC",
        "strategy_id": "intc_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:INTC-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "INTC.NASDAQ",
    },
    {
        "symbol": "META",
        "strategy_id": "meta_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:META-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "META.NASDAQ",
    },
    {
        "symbol": "MSTR",
        "strategy_id": "mstr_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:MSTR-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "MSTR.NASDAQ",
    },
    {
        "symbol": "MSFT",
        "strategy_id": "msft_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:MSFT-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "MSFT.NASDAQ",
    },
    {
        "symbol": "MU",
        "strategy_id": "mu_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:MU-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "MU.NASDAQ",
    },
    {
        "symbol": "NFLX",
        "strategy_id": "nflx_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:NFLX-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "NFLX.NASDAQ",
    },
    {
        "symbol": "NVDA",
        "strategy_id": "nvda_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:NVDA-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "NVDA.NASDAQ",
    },
    {
        "symbol": "ORCL",
        "strategy_id": "orcl_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:ORCL-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "ORCL.NYSE",
    },
    {
        "symbol": "PLTR",
        "strategy_id": "pltr_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:PLTR-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "PLTR.NASDAQ",
    },
    {
        "symbol": "RIVN",
        "strategy_id": "rivn_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:RIVN-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "RIVN.NASDAQ",
    },
    {
        "symbol": "SNDK",
        "strategy_id": "sndk_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:SNDK-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "SNDK.NASDAQ",
    },
    {
        "symbol": "TSM",
        "strategy_id": "tsm_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:TSM-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "TSM.NYSE",
    },
    {
        "symbol": "TSLA",
        "strategy_id": "tsla_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:TSLA-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "TSLA.NASDAQ",
    },
    {
        "symbol": "USAR",
        "strategy_id": "usar_tradexyz_makerv4",
        "hyperliquid_instrument_id": "xyz:USAR-USD-PERP.HYPERLIQUID",
        "ibkr_instrument_id": "USAR.NASDAQ",
    },
)
ACTIVE_STRATEGY_IDS = [entry["strategy_id"] for entry in ACTIVE_STRATEGIES]
ACTIVE_HYPERLIQUID_INSTRUMENT_IDS = {
    entry["hyperliquid_instrument_id"]
    for entry in ACTIVE_STRATEGIES
}
ACTIVE_IBKR_INSTRUMENT_IDS = {
    entry["ibkr_instrument_id"]
    for entry in ACTIVE_STRATEGIES
}
CORE_PROD_STRATEGY_IDS = (
    "aapl_tradexyz_makerv4",
    "amd_tradexyz_makerv4",
    "amzn_tradexyz_makerv4",
    "baba_tradexyz_makerv4",
    "coin_tradexyz_makerv4",
    "crcl_tradexyz_makerv4",
    "crwv_tradexyz_makerv4",
    "googl_tradexyz_makerv4",
    "hood_tradexyz_makerv4",
    "intc_tradexyz_makerv4",
    "meta_tradexyz_makerv4",
    "msft_tradexyz_makerv4",
    "mstr_tradexyz_makerv4",
    "mu_tradexyz_makerv4",
    "nflx_tradexyz_makerv4",
    "nvda_tradexyz_makerv4",
    "orcl_tradexyz_makerv4",
    "pltr_tradexyz_makerv4",
    "rivn_tradexyz_makerv4",
    "sndk_tradexyz_makerv4",
    "tsla_tradexyz_makerv4",
    "tsm_tradexyz_makerv4",
    "usar_tradexyz_makerv4",
)
DEPLOYED_MULTIVENUE_ROUTE_IDS = (
    "amzn_binance_perp_makerv4",
    "coin_binance_perp_makerv4",
    "crcl_binance_perp_makerv4",
    "ewy_binance_perp_makerv4",
    "hood_binance_perp_makerv4",
    "intc_binance_perp_makerv4",
    "mstr_binance_perp_makerv4",
    "pltr_binance_perp_makerv4",
    "tsla_binance_perp_makerv4",
)
LIVE_ENROLLED_STRATEGY_IDS = (
    "aapl_tradexyz_makerv4",
    "amd_tradexyz_makerv4",
    "amzn_binance_perp_makerv4",
    "amzn_tradexyz_makerv4",
    "baba_tradexyz_makerv4",
    "coin_binance_perp_makerv4",
    "coin_tradexyz_makerv4",
    "crcl_binance_perp_makerv4",
    "crcl_tradexyz_makerv4",
    "crwv_tradexyz_makerv4",
    "ewy_binance_perp_makerv4",
    "googl_tradexyz_makerv4",
    "hood_binance_perp_makerv4",
    "hood_tradexyz_makerv4",
    "intc_binance_perp_makerv4",
    "intc_tradexyz_makerv4",
    "meta_tradexyz_makerv4",
    "msft_tradexyz_makerv4",
    "mstr_binance_perp_makerv4",
    "mstr_tradexyz_makerv4",
    "mu_tradexyz_makerv4",
    "nflx_tradexyz_makerv4",
    "nvda_tradexyz_makerv4",
    "orcl_tradexyz_makerv4",
    "pltr_binance_perp_makerv4",
    "pltr_tradexyz_makerv4",
    "rivn_tradexyz_makerv4",
    "sndk_tradexyz_makerv4",
    "tsla_binance_perp_makerv4",
    "tsla_tradexyz_makerv4",
    "tsm_tradexyz_makerv4",
    "usar_tradexyz_makerv4",
)
REMOVED_MAKERV3_STRATEGY_IDS = (
    "aapl_tradexyz_makerv3",
    "baba_tradexyz_makerv3",
    "coin_tradexyz_makerv3",
    "crcl_tradexyz_makerv3",
    "crwv_tradexyz_makerv3",
    "hood_tradexyz_makerv3",
    "hyundai_tradexyz_makerv3",
    "intc_tradexyz_makerv3",
    "mstr_tradexyz_makerv3",
    "mu_tradexyz_makerv3",
    "nflx_tradexyz_makerv3",
    "rivn_tradexyz_makerv3",
    "sndk_tradexyz_makerv3",
    "tsm_tradexyz_makerv3",
    "usar_tradexyz_makerv3",
)
CORE_PROD_STRATEGIES = tuple(
    entry for entry in ACTIVE_STRATEGIES if entry["strategy_id"] in CORE_PROD_STRATEGY_IDS
)
CORE_PROD_HYPERLIQUID_INSTRUMENT_IDS = {
    entry["hyperliquid_instrument_id"]
    for entry in CORE_PROD_STRATEGIES
}
CORE_PROD_IBKR_INSTRUMENT_IDS = {
    entry["ibkr_instrument_id"]
    for entry in CORE_PROD_STRATEGIES
}
DECLARED_MULTIVENUE_ROUTE_CONTRACTS = {
    "amzn_binance_perp_makerv4": (
        "AMZN",
        "BINANCE_PERP",
        "AMZNUSDT",
        "perp",
        "AMZNUSDT-PERP.BINANCE_PERP",
        "AMZN.NASDAQ",
    ),
    "coin_binance_perp_makerv4": (
        "COIN",
        "BINANCE_PERP",
        "COINUSDT",
        "perp",
        "COINUSDT-PERP.BINANCE_PERP",
        "COIN.NASDAQ",
    ),
    "crcl_binance_perp_makerv4": (
        "CRCL",
        "BINANCE_PERP",
        "CRCLUSDT",
        "perp",
        "CRCLUSDT-PERP.BINANCE_PERP",
        "CRCL.NYSE",
    ),
    "ewy_binance_perp_makerv4": (
        "EWY",
        "BINANCE_PERP",
        "EWYUSDT",
        "perp",
        "EWYUSDT-PERP.BINANCE_PERP",
        "EWY.NYSE",
    ),
    "hood_binance_perp_makerv4": (
        "HOOD",
        "BINANCE_PERP",
        "HOODUSDT",
        "perp",
        "HOODUSDT-PERP.BINANCE_PERP",
        "HOOD.NASDAQ",
    ),
    "intc_binance_perp_makerv4": (
        "INTC",
        "BINANCE_PERP",
        "INTCUSDT",
        "perp",
        "INTCUSDT-PERP.BINANCE_PERP",
        "INTC.NASDAQ",
    ),
    "mstr_binance_perp_makerv4": (
        "MSTR",
        "BINANCE_PERP",
        "MSTRUSDT",
        "perp",
        "MSTRUSDT-PERP.BINANCE_PERP",
        "MSTR.NASDAQ",
    ),
    "pltr_binance_perp_makerv4": (
        "PLTR",
        "BINANCE_PERP",
        "PLTRUSDT",
        "perp",
        "PLTRUSDT-PERP.BINANCE_PERP",
        "PLTR.NASDAQ",
    ),
    "tsla_binance_perp_makerv4": (
        "TSLA",
        "BINANCE_PERP",
        "TSLAUSDT",
        "perp",
        "TSLAUSDT-PERP.BINANCE_PERP",
        "TSLA.NASDAQ",
    ),
}
DECLARED_EXTRA_SHARED_CONTRACTS = {
    ("binance_perp", "AMZNUSDT-PERP.BINANCE_PERP"),
    ("binance_perp", "COINUSDT-PERP.BINANCE_PERP"),
    ("binance_perp", "CRCLUSDT-PERP.BINANCE_PERP"),
    ("binance_perp", "EWYUSDT-PERP.BINANCE_PERP"),
    ("binance_perp", "HOODUSDT-PERP.BINANCE_PERP"),
    ("binance_perp", "INTCUSDT-PERP.BINANCE_PERP"),
    ("binance_perp", "MSTRUSDT-PERP.BINANCE_PERP"),
    ("binance_perp", "PLTRUSDT-PERP.BINANCE_PERP"),
    ("binance_perp", "TSLAUSDT-PERP.BINANCE_PERP"),
    ("ibkr", "EWY.NYSE"),
}


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _read(path: Path) -> str:
    return path.read_text(encoding="utf-8")


def _load_toml(path: Path) -> dict:
    return tomllib.load(path.open("rb"))


def _extract_markdown_code_bullets(text: str, heading: str, *, level: int) -> tuple[str, ...]:
    lines = text.splitlines()
    heading_prefix = "#" * level

    try:
        start = lines.index(f"{heading_prefix} {heading}") + 1
    except ValueError as exc:
        raise AssertionError(f"Missing heading: {heading}") from exc

    items: list[str] = []
    for line in lines[start:]:
        if line.startswith("#"):
            break
        match = re.fullmatch(r"- `([^`]+)`", line)
        if match:
            items.append(match.group(1))

    assert items, f"Missing bullet list under heading: {heading}"
    return tuple(items)


def _extract_markdown_numbered_list(text: str, heading: str, *, level: int) -> tuple[str, ...]:
    lines = text.splitlines()
    heading_prefix = "#" * level

    try:
        start = lines.index(f"{heading_prefix} {heading}") + 1
    except ValueError as exc:
        raise AssertionError(f"Missing heading: {heading}") from exc

    items: list[str] = []
    for line in lines[start:]:
        if line.startswith("#"):
            break
        match = re.fullmatch(r"\d+\. (.+)", line)
        if match:
            items.append(match.group(1))

    assert items, f"Missing numbered list under heading: {heading}"
    return tuple(items)


def test_equities_live_config_uses_dedicated_portfolio_and_allowlists() -> None:
    config = _load_toml(_repo_root() / "deploy/equities/equities.live.toml")

    assert config["portfolio"]["portfolio_id"] == "equities"
    assert config["api"]["strategy_class"] == ACTIVE_STRATEGY_CLASS
    assert config["api"]["strategy_groups"] == "equities"
    assert config["api"]["param_set"] == ACTIVE_PARAM_SET
    assert config["api"]["equities_strategy_ids"] == list(LIVE_ENROLLED_STRATEGY_IDS)
    assert config["api"]["equities_required_strategy_ids"] == list(LIVE_ENROLLED_STRATEGY_IDS)


def test_equities_live_config_decommissions_hyundai() -> None:
    repo_root = _repo_root()
    config = _load_toml(repo_root / "deploy/equities/equities.live.toml")
    readme = _read(repo_root / "deploy/equities/README.md")
    target = _read(repo_root / "deploy/equities/systemd/flux-equities.target")
    sudoers = _read(repo_root / "deploy/equities/systemd/flux-pulse.sudoers")

    assert "hyundai_tradexyz_makerv3" not in config["api"]["equities_strategy_ids"]
    assert "hyundai_tradexyz_makerv3" not in config["api"]["equities_required_strategy_ids"]
    assert "hyundai_tradexyz_makerv3" not in {
        row["strategy_id"] for row in config["strategy_contracts"]
    }
    assert "xyz:HYUNDAI-USD-PERP.HYPERLIQUID" not in {
        row["instrument_id"] for row in config["contracts"] if row["exchange"] == "hyperliquid"
    }
    assert "005380.KRX" not in {
        row["instrument_id"] for row in config["contracts"] if row["exchange"] == "ibkr"
    }
    assert not (repo_root / "deploy/equities/strategies/hyundai_tradexyz_makerv3.toml").exists()
    assert not (repo_root / "deploy/equities/strategies/hyundai_tradexyz_makerv3.toml.disabled").exists()
    assert "hyundai_tradexyz_makerv3" in readme
    assert "hyundai_tradexyz_makerv3" not in target
    assert "hyundai_tradexyz_makerv3" not in sudoers


def test_equities_strategy_template_uses_hyperliquid_xyz_and_equities_group() -> None:
    template_path = _repo_root() / "deploy/equities/strategies/equities.strategy.template.toml"
    template = _read(template_path)
    config = _load_toml(template_path)
    hyperliquid = config["node"]["venues"]["HYPERLIQUID"]
    ibkr = config["node"]["venues"]["IBKR"]
    strategy = config["strategy"]
    identity = config["identity"]

    assert 'execution_venue = "HYPERLIQUID"' in template
    assert 'reference_venue = "IBKR"' in template
    assert 'reference_symbol = "AAPL/USD"' in template
    assert 'dex = "xyz"' in template
    assert 'instrument_id = "xyz:AAPL-USD-PERP.HYPERLIQUID"' in template
    assert '[node.venues.IBKR]' in template
    assert 'instrument_id = "AAPL.NASDAQ"' in template
    assert '[node.venues.IBKR.dockerized_gateway]' in template
    assert 'trading_mode = "live"' in template
    assert 'read_only_api = true' in template
    assert "manage_container = false" in template
    assert 'twofa_timeout_action = "exit"' not in template
    assert identity["strategy_id"] == "symbol_tradexyz_makerv4"
    assert identity["strategy_instance_id"] == "symbol_tradexyz_makerv4"
    assert identity["external_strategy_id"] == "symbol_tradexyz_makerv4"
    assert hyperliquid["private_key_env"] == "TRADE_XYZ_AGENT_PK"
    assert hyperliquid["account_address_env"] == "TRADE_XYZ_ACCOUNT_ADDRESS"
    assert hyperliquid["vault_address_env"] == "TRADE_XYZ_VAULT_ADDRESS"
    assert ibkr["instrument_id"] == "AAPL.NASDAQ"
    assert ibkr["use_regular_trading_hours"] is False
    assert strategy["strategy_groups"] == "equities"
    assert strategy["order_qty"] == "1"
    assert strategy["qty"] == "1"
    assert strategy["manage_stop"] is False
    assert strategy.get("param_set") in {None, ACTIVE_PARAM_SET}
    assert strategy["max_qty_global"] == 100.0
    assert strategy["max_skew_bps_global"] == 10.0
    assert strategy["bid_edge1"] == 5.0
    assert strategy["ask_edge1"] == 5.0
    assert strategy["place_edge1"] == 1.0
    assert strategy["distance1"] == 2.0
    assert strategy["n_orders1"] == 3


def test_equities_strategy_template_documents_binance_perp_credentials() -> None:
    template = _read(_repo_root() / "deploy/equities/strategies/equities.strategy.template.toml")

    assert 'execution_venue = "BINANCE_PERP"' in template
    assert '[node.venues.BINANCE_PERP]' in template
    assert 'instrument_id = "AAPLUSDT-PERP.BINANCE_PERP"' in template
    assert 'api_key_env = "EQUITIES_BINANCE_API_KEY"' in template
    assert 'api_secret_env = "EQUITIES_BINANCE_API_SECRET"' in template
    assert 'account_type = "USDT_FUTURES"' in template


def test_equities_live_config_only_keeps_shared_contract_values() -> None:
    live_config_path = _repo_root() / "deploy/equities/equities.live.toml"
    live_config = _read(live_config_path)
    config = _load_toml(live_config_path)
    contracts = {(entry["exchange"], entry["instrument_id"]) for entry in config["contracts"]}

    assert set(config) == {
        "flux",
        "identity",
        "redis",
        "venues",
        "bridge",
        "api",
        "portfolio",
        "account_scopes",
        "strategy_contracts",
        "contracts",
    }
    assert "[node]" not in live_config
    assert "[strategy]" not in live_config
    assert "[[strategy_contracts]]" in live_config
    assert 'exchange = "hyperliquid"' in live_config
    assert 'exchange = "binance_perp"' in live_config
    assert 'exchange = "ibkr"' in live_config
    assert contracts == {
        *{
            ("hyperliquid", instrument_id)
            for instrument_id in CORE_PROD_HYPERLIQUID_INSTRUMENT_IDS
        },
        *DECLARED_EXTRA_SHARED_CONTRACTS,
        *{
            ("ibkr", instrument_id)
            for instrument_id in CORE_PROD_IBKR_INSTRUMENT_IDS
        },
    }


def test_equities_active_strategy_contracts_use_makerv4_semantics_with_active_ids() -> None:
    repo_root = _repo_root()
    for entry in CORE_PROD_STRATEGIES:
        active_path = repo_root / f"deploy/equities/strategies/{entry['strategy_id']}.toml"
        assert active_path.exists()
        config = _load_toml(active_path)
        assert config["identity"]["strategy_id"] == entry["strategy_id"]
        assert config["identity"]["strategy_instance_id"] == entry["strategy_id"]
        assert config["identity"]["external_strategy_id"] == entry["strategy_id"]
        assert config["strategy"]["strategy_id"] == entry["strategy_id"]
        assert config["strategy"].get("param_set") in {None, ACTIVE_PARAM_SET}
        assert config["strategy"]["manage_stop"] is False
        assert config["strategy"]["force_bot_off_on_start"] is True
        assert config["strategy"]["outside_rth_hedge_enabled"] is True
        assert config["strategy"]["max_age_ms"] == EQUITIES_MAKER_MAX_QUOTE_AGE_MS
        assert config["strategy"]["max_ibkr_quote_age_ms"] == EQUITIES_IBKR_MAX_QUOTE_AGE_MS
        expected_primary_exchange = entry["ibkr_instrument_id"].rsplit(".", 1)[1]
        assert config["strategy"]["ibkr_primary_exchange"] == expected_primary_exchange
        assert config["node"]["enable_execution"] is False
        assert (
            config["node"]["venues"]["HYPERLIQUID"]["instrument_id"]
            == entry["hyperliquid_instrument_id"]
        )
        assert config["node"]["venues"]["IBKR"]["instrument_id"] == entry["ibkr_instrument_id"]
        assert config["node"]["venues"]["IBKR"]["use_regular_trading_hours"] is False
        assert config["node"]["venues"]["IBKR"]["dockerized_gateway"]["manage_container"] is False
        assert (
            config["node"]["venues"]["HYPERLIQUID"]["vault_address_env"]
            == "TRADE_XYZ_VAULT_ADDRESS"
        )
        assert "twofa_timeout_action" not in config["node"]["venues"]["IBKR"]["dockerized_gateway"]
    for strategy_id in REMOVED_MAKERV3_STRATEGY_IDS:
        assert not (repo_root / f"deploy/equities/strategies/{strategy_id}.toml").exists()
        assert not (repo_root / f"deploy/equities/strategies/{strategy_id}.toml.disabled").exists()


def test_equities_discovery_matches_live_enrolled_makerv4_routes() -> None:
    repo_root = _repo_root()
    strategies_dir = repo_root / "deploy/equities/strategies"
    active_strategy_ids = sorted(
        path.stem
        for path in strategies_dir.glob("*.toml")
        if path.name != "equities.strategy.template.toml"
    )

    assert active_strategy_ids == sorted(LIVE_ENROLLED_STRATEGY_IDS)
    for strategy_id in REMOVED_MAKERV3_STRATEGY_IDS:
        assert not (strategies_dir / f"{strategy_id}.toml").exists()
        assert not (strategies_dir / f"{strategy_id}.toml.disabled").exists()


def test_equities_node_execution_contract_is_safe_in_toml_and_opt_in_in_stack() -> None:
    repo_root = _repo_root()
    active_path = repo_root / f"deploy/equities/strategies/{ACTIVE_STRATEGIES[0]['strategy_id']}.toml"
    template_path = repo_root / "deploy/equities/strategies/equities.strategy.template.toml"
    active_config = _load_toml(active_path)
    template_config = _load_toml(template_path)
    active_text = _read(active_path)
    template_text = _read(template_path)
    stack_script = _read(repo_root / "ops/scripts/deploy/equities_stack.sh")
    install_script = _read(repo_root / "ops/scripts/deploy/install_equities_systemd.sh")

    assert active_config["node"]["enable_execution"] is False
    assert template_config["node"]["enable_execution"] is False
    assert template_config["strategy"]["max_age_ms"] == EQUITIES_MAKER_MAX_QUOTE_AGE_MS
    assert template_config["strategy"]["max_ibkr_quote_age_ms"] == EQUITIES_IBKR_MAX_QUOTE_AGE_MS
    assert (
        "# Checked-in strategy configs stay safe-off; explicit runtime --enable-execution"
        in active_text
    )
    assert (
        "# Checked-in strategy configs stay safe-off; explicit runtime --enable-execution"
        in template_text
    )
    assert 'ENABLE_EXECUTION="${EQUITIES_ENABLE_EXECUTION:-0}"' in stack_script
    assert (
        'runtime override: EQUITIES_ENABLE_EXECUTION=1 passes --enable-execution even when'
        in stack_script
    )
    assert 'exec_flag+=(--enable-execution)' in stack_script
    assert (
        '${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_node --config'
        in install_script
    )
    assert "--enable-execution" in install_script


def test_equities_shared_contract_catalog_matches_core_prod_strategy_routes() -> None:
    repo_root = _repo_root()
    shared_config = _load_toml(repo_root / "deploy/equities/equities.live.toml")
    shared_contracts = {
        (entry["exchange"], entry["instrument_id"])
        for entry in shared_config["contracts"]
    }
    for entry in CORE_PROD_STRATEGIES:
        active_config = _load_toml(
            repo_root / f"deploy/equities/strategies/{entry['strategy_id']}.toml",
        )
        assert (
            "hyperliquid",
            active_config["node"]["venues"]["HYPERLIQUID"]["instrument_id"],
        ) in shared_contracts
        assert (
            "ibkr",
            active_config["node"]["venues"]["IBKR"]["instrument_id"],
        ) in shared_contracts
    assert ("binance_perp", "PLTRUSDT-PERP.BINANCE_PERP") in shared_contracts


def test_equities_live_config_declares_strategy_contracts_with_portfolio_asset_ids() -> None:
    config = _load_toml(_repo_root() / "deploy/equities/equities.live.toml")
    contracts = config["strategy_contracts"]
    aapl = next(item for item in contracts if item["strategy_id"] == "aapl_tradexyz_makerv4")

    assert aapl["portfolio_asset_id"] == "AAPL"
    assert aapl["maker_venue"] == "HYPERLIQUID"
    assert aapl["maker_symbol"] == "AAPL"
    assert aapl["market_type"] == "perp"
    assert aapl["maker_instrument_id"] == "xyz:AAPL-USD-PERP.HYPERLIQUID"
    assert aapl["reference_instrument_id"] == "AAPL.NASDAQ"
    assert aapl["execution_account_scope_id"] == "hyperliquid.xyz.main"
    assert aapl["reference_account_scope_id"] == "ibkr.reference.main"
    assert aapl["hedge_account_scope_id"] == "ibkr.hedge.main"


def test_equities_live_config_allows_multiple_routes_for_same_portfolio_asset() -> None:
    config = _load_toml(_repo_root() / "deploy/equities/equities.live.toml")
    rows = config["strategy_contracts"]
    pltr_rows = [row for row in rows if row["portfolio_asset_id"] == "PLTR"]

    assert {row["maker_venue"] for row in pltr_rows} == {"HYPERLIQUID", "BINANCE_PERP"}
    assert len({row["strategy_id"] for row in pltr_rows}) == len(pltr_rows)


def test_equities_live_config_declares_shared_account_scopes() -> None:
    config = _load_toml(_repo_root() / "deploy/equities/equities.live.toml")
    scopes = {row["scope_id"]: row for row in config["account_scopes"]}
    reference_gateway = scopes["ibkr.reference.main"]["dockerized_gateway"]
    hedge_gateway = scopes["ibkr.hedge.main"]["dockerized_gateway"]

    assert scopes["hyperliquid.xyz.main"]["provider"] == "hyperliquid"
    assert scopes["hyperliquid.xyz.main"]["venue"] == "HYPERLIQUID"
    assert scopes["binance.futures.main"]["provider"] == "binance"
    assert scopes["binance.futures.main"]["venue"] == "BINANCE_PERP"
    assert scopes["binance.futures.main"]["api_key_env"] == "EQUITIES_BINANCE_API_KEY"
    assert scopes["binance.futures.main"]["api_secret_env"] == "EQUITIES_BINANCE_API_SECRET"
    assert scopes["binance.futures.main"]["account_type"] == "USDT_FUTURES"
    assert scopes["binance.futures.main"]["private_api_family"] == "PORTFOLIO_MARGIN"
    assert scopes["ibkr.reference.main"]["provider"] == "ibkr"
    assert scopes["ibkr.reference.main"]["venue"] == "IBKR"
    assert scopes["ibkr.reference.main"]["ibg_client_id"] == 107
    assert scopes["ibkr.reference.main"]["account_id"] == "U10015777"
    assert reference_gateway["manage_container"] is True
    assert reference_gateway["relogin_after_twofa_timeout"] is False
    assert reference_gateway["twofa_timeout_action"] == "exit"
    assert scopes["ibkr.hedge.main"]["provider"] == "ibkr"
    assert scopes["ibkr.hedge.main"]["venue"] == "IBKR"
    assert scopes["ibkr.hedge.main"]["ibg_client_id"] == 208
    assert scopes["ibkr.hedge.main"]["account_id"] == "U10015777"
    assert hedge_gateway["manage_container"] is False
    assert "twofa_timeout_action" not in hedge_gateway


def test_equities_live_config_strategy_contracts_cover_core_prod_routes_and_deployed_multivenue_rows(
) -> None:
    config = _load_toml(_repo_root() / "deploy/equities/equities.live.toml")
    rows = config["strategy_contracts"]
    strategy_ids = [entry["strategy_id"] for entry in rows]

    assert len(rows) == len(CORE_PROD_STRATEGIES) + len(DEPLOYED_MULTIVENUE_ROUTE_IDS)
    assert len(strategy_ids) == len(set(strategy_ids))

    contracts = {
        entry["strategy_id"]: (
            entry["portfolio_asset_id"],
            entry["maker_venue"],
            entry["maker_symbol"],
            entry["market_type"],
            entry["maker_instrument_id"],
            entry["reference_instrument_id"],
        )
        for entry in rows
    }

    assert set(CORE_PROD_STRATEGY_IDS).issubset(contracts)
    assert set(DEPLOYED_MULTIVENUE_ROUTE_IDS).issubset(contracts)
    for entry in CORE_PROD_STRATEGIES:
        assert contracts[entry["strategy_id"]] == (
            entry["symbol"],
            "HYPERLIQUID",
            entry["symbol"],
            "perp",
            entry["hyperliquid_instrument_id"],
            entry["ibkr_instrument_id"],
        )
    for strategy_id, expected in DECLARED_MULTIVENUE_ROUTE_CONTRACTS.items():
        assert contracts[strategy_id] == expected


def test_equities_strategy_ibkr_gateway_client_ids_are_unique() -> None:
    repo_root = _repo_root()
    client_ids: list[int] = []

    for strategy_id in LIVE_ENROLLED_STRATEGY_IDS:
        active_config = _load_toml(repo_root / f"deploy/equities/strategies/{strategy_id}.toml")
        client_ids.append(active_config["node"]["venues"]["IBKR"]["ibg_client_id"])

    assert len(client_ids) == len(set(client_ids))


def test_equities_shared_ibkr_scope_client_ids_do_not_overlap_strategy_client_ids() -> None:
    repo_root = _repo_root()
    shared_config = _load_toml(repo_root / "deploy/equities/equities.live.toml")
    shared_client_ids = {
        row["ibg_client_id"]
        for row in shared_config["account_scopes"]
        if row["provider"] == "ibkr"
    }
    strategy_client_ids: set[int] = set()

    for strategy_id in LIVE_ENROLLED_STRATEGY_IDS:
        active_config = _load_toml(repo_root / f"deploy/equities/strategies/{strategy_id}.toml")
        strategy_client_ids.add(active_config["node"]["venues"]["IBKR"]["ibg_client_id"])

    assert shared_client_ids.isdisjoint(strategy_client_ids)


def test_equities_shared_gateway_owner_is_configured_once() -> None:
    config = _load_toml(_repo_root() / "deploy/equities/equities.live.toml")
    owners = [
        row["scope_id"]
        for row in config["account_scopes"]
        if row.get("provider") == "ibkr"
        and row.get("dockerized_gateway", {}).get("manage_container") is True
    ]

    assert owners == ["ibkr.reference.main"]


def test_equities_stack_env_example_defaults_to_safe_paper_without_execution() -> None:
    env_example = _read(_repo_root() / "deploy/equities/equities_stack.env.example")

    assert "EQUITIES_MODE=paper" in env_example
    assert "EQUITIES_CONFIRM_LIVE=0" in env_example
    assert "EQUITIES_ENABLE_EXECUTION=0" in env_example
    assert "EQUITIES_ALLOW_MISSING_KEYS=0" in env_example
    assert "deploy/equities/equities.live.toml" in env_example
    assert "TRADE_XYZ_AGENT_PK=" in env_example
    assert "TRADE_XYZ_ACCOUNT_ADDRESS=" in env_example
    assert "TRADE_XYZ_VAULT_ADDRESS=" in env_example
    assert "EQUITIES_BINANCE_API_KEY=" in env_example
    assert "EQUITIES_BINANCE_API_SECRET=" in env_example
    assert "TWS_USERNAME=" in env_example
    assert "TWS_PASSWORD=" in env_example


def test_equities_binance_discovery_and_env_contract_are_documented() -> None:
    repo_root = _repo_root()
    template = _read(repo_root / "deploy/equities/strategies/equities.strategy.template.toml")
    env_example = _read(repo_root / "deploy/equities/equities_stack.env.example")
    common_env = _read(repo_root / "deploy/equities/systemd/common.env.example")
    readme = _read(repo_root / "deploy/equities/README.md")

    assert "BINANCE_PERP" in template
    assert "EQUITIES_BINANCE_API_KEY=" in env_example
    assert "EQUITIES_BINANCE_API_SECRET=" in env_example
    assert "EQUITIES_BINANCE_API_KEY=" in common_env
    assert "EQUITIES_BINANCE_API_SECRET=" in common_env
    assert "binance_equities_universe.py" in readme
    assert "Checked-in `.toml` strategy files are the enrolled set." in readme


def test_equities_stack_honors_enable_execution_flag_for_nodes() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/equities_stack.sh")

    assert 'exec_flag=()' in script
    assert 'if [[ "${ENABLE_EXECUTION}" == "1" ]]; then' in script
    assert 'exec_flag+=(--enable-execution)' in script


def test_equities_systemd_installer_honors_enable_execution_flag_for_nodes() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/install_equities_systemd.sh")

    assert 'ENABLE_EXECUTION="${EQUITIES_ENABLE_EXECUTION:-0}"' in script
    assert 'if [[ "${ENABLE_EXECUTION}" == "1" ]]; then' in script
    assert "--enable-execution" in script


def test_equities_stack_script_is_scoped_to_equities_services_and_paths() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/equities_stack.sh")

    assert 'DEFAULT_ENV_PATH="${ROOT_DIR}/deploy/equities/equities_stack.env"' in script
    assert 'CONFIG_PATH="${EQUITIES_CONFIG_PATH:-${ROOT_DIR}/deploy/equities/equities.live.toml}"' in script
    assert 'STRATEGIES_DIR="${EQUITIES_STRATEGIES_DIR:-${ROOT_DIR}/deploy/equities/strategies}"' in script
    assert 'MODE="${EQUITIES_MODE:-paper}"' in script
    assert 'ENABLE_EXECUTION="${EQUITIES_ENABLE_EXECUTION:-0}"' in script
    assert 'nautilus_trader.flux.runners.equities.run_portfolio' in script
    assert 'nautilus_trader.flux.runners.equities.run_bridge' in script
    assert 'nautilus_trader.flux.runners.equities.run_api' in script
    assert 'nautilus_trader.flux.runners.equities.run_node' in script
    assert "--all-strategies" not in script
    assert '/equities' in script
    assert '/api/v1/params?profile=equities' in script
    assert '/api/v1/balances?profile=equities' in script
    assert '/api/v1/trades?profile=equities' in script
    assert 'logs <svc>' in script
    assert 'TOKENMM_' not in script


def test_equities_systemd_assets_use_core_prod_service_names_only() -> None:
    target = _read(_repo_root() / "deploy/equities/systemd/flux-equities.target")
    install_script = _read(_repo_root() / "ops/scripts/deploy/install_equities_systemd.sh")
    common_env = _read(_repo_root() / "deploy/equities/systemd/common.env.example")
    sudoers = _read(_repo_root() / "deploy/equities/systemd/flux-pulse.sudoers")

    assert "[Install]" in target
    assert "WantedBy=multi-user.target" in target
    assert 'Wants=flux@equities-api.service' in target
    assert 'Wants=flux@equities-portfolio.service' in target
    assert 'Wants=flux@equities-bridge.service' in target
    for strategy_id in CORE_PROD_STRATEGY_IDS:
        assert f'Wants=flux@equities-node-{strategy_id}.service' in target
    for strategy_id in DEPLOYED_MULTIVENUE_ROUTE_IDS:
        assert f'Wants=flux@equities-node-{strategy_id}.service' in target
    for strategy_id in REMOVED_MAKERV3_STRATEGY_IDS:
        assert f'Wants=flux@equities-node-{strategy_id}.service' not in target
    assert 'deploy/equities/equities.live.toml' in install_script
    assert 'flux-equities.target' in install_script
    assert 'deploy/equities/systemd/common.env.example' in install_script
    assert '/etc/sudoers.d/flux-pulse' in install_script
    assert 'rebuild_flux_pulse_sudoers.sh' in install_script
    assert 'strategy_stack_write_env' in install_script
    assert 'equities-api.env' in install_script
    assert 'equities-portfolio.env' in install_script
    assert 'equities-bridge.env' in install_script
    assert '"equities"' in install_script
    assert '"Equities"' in install_script
    assert '"20"' in install_script
    assert '--host 127.0.0.1 --port 5024 --serve-fluxboard' in install_script
    assert '"0"' in install_script
    assert 'strategy_stack_discover_strategy_ids' in install_script
    assert "--all-strategies" not in install_script
    assert 'EQUITIES_REDIS_HOST=' in common_env
    assert 'EQUITIES_REDIS_PORT=6379' in common_env
    assert 'EQUITIES_REDIS_USERNAME=default' in common_env
    assert 'EQUITIES_REDIS_PASSWORD=' in common_env
    assert 'EQUITIES_REDIS_SSL=1' in common_env
    assert 'EQUITIES_API_BACKEND_URL=http://127.0.0.1:5024' in common_env
    assert 'TRADE_XYZ_AGENT_PK=' in common_env
    assert 'TRADE_XYZ_ACCOUNT_ADDRESS=' in common_env
    assert 'TRADE_XYZ_VAULT_ADDRESS=' in common_env
    assert 'EQUITIES_BINANCE_API_KEY=' in common_env
    assert 'EQUITIES_BINANCE_API_SECRET=' in common_env
    assert "/usr/bin/systemctl start flux@equities-api.service" not in sudoers
    assert "/usr/bin/systemctl restart flux@equities-portfolio.service" in sudoers
    for strategy_id in CORE_PROD_STRATEGY_IDS:
        assert f"/usr/bin/systemctl restart flux@equities-node-{strategy_id}.service" in sudoers
    for strategy_id in DEPLOYED_MULTIVENUE_ROUTE_IDS:
        assert f"/usr/bin/systemctl restart flux@equities-node-{strategy_id}.service" in sudoers
    for strategy_id in REMOVED_MAKERV3_STRATEGY_IDS:
        assert f"/usr/bin/systemctl restart flux@equities-node-{strategy_id}.service" not in sudoers
    assert "flux@*" not in sudoers


def test_equities_installer_embeds_checkout_specific_runtime_paths() -> None:
    install_script = _read(_repo_root() / "ops/scripts/deploy/install_equities_systemd.sh")

    assert 'EQUITIES_PYTHON_BIN="${ROOT_DIR}/.venv/bin/python"' in install_script
    assert "require_project_python()" in install_script
    assert 'if [[ ! -x "${EQUITIES_PYTHON_BIN}" ]]; then' in install_script
    assert "uv sync --all-groups --all-extras" in install_script
    assert "--active --all-groups --all-extras" not in install_script
    assert re.search(
        r"main\(\)\s*\{\n(?:[ \t]*(?:#.*)?\n)*[ \t]*require_sudo\n(?:[ \t]*(?:#.*)?\n)*[ \t]*require_project_python\n",
        install_script,
    )
    assert "find \"${ENV_DIR}\" -maxdepth 1 -type f -name 'equities-node-*.env' -delete" in install_script
    assert "append_checkout_env_overrides()" in install_script
    assert "printf 'WORKDIR=%s\\nPYTHONPATH=%s\\n' \"${ROOT_DIR}\" \"${ROOT_DIR}\" >> \"${env_path}\"" in install_script
    assert '${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_api' in install_script
    assert '${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_portfolio' in install_script
    assert '${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_bridge' in install_script
    assert '${EQUITIES_PYTHON_BIN} -m nautilus_trader.flux.runners.equities.run_node' in install_script
    assert 'append_checkout_env_overrides "${ENV_DIR}/equities-api.env"' in install_script
    assert 'append_checkout_env_overrides "${ENV_DIR}/equities-portfolio.env"' in install_script
    assert 'append_checkout_env_overrides "${ENV_DIR}/equities-bridge.env"' in install_script
    assert 'append_checkout_env_overrides "${ENV_DIR}/${service_id}.env"' in install_script


def test_equities_installer_cleans_all_generated_envs_before_reinstall() -> None:
    install_script = _read(_repo_root() / "ops/scripts/deploy/install_equities_systemd.sh")

    assert 'rm -f "${ENV_DIR}/equities-api.env"' in install_script
    assert 'rm -f "${ENV_DIR}/equities-portfolio.env"' in install_script
    assert 'rm -f "${ENV_DIR}/equities-bridge.env"' in install_script
    assert "find \"${ENV_DIR}\" -maxdepth 1 -type f -name 'equities-node-*.env' -delete" in install_script


def test_equities_shared_fluxboard_contract_uses_neutral_static_prefix() -> None:
    repo_root = _repo_root()
    install_script = _read(repo_root / "ops/scripts/deploy/install_equities_systemd.sh")
    lp_contract = _read(repo_root / "fluxboard/docs/lp_contract.md")

    assert "FLUXBOARD_BASE_PATH=/tokenmm/" not in install_script
    assert "/static/fluxboard/*" in lp_contract
    assert "`/equities`, `/lp`, and `/tokenmm` stay SPA entry routes, not asset owners." in lp_contract


def test_equities_contract_doc_keeps_equities_routes_spa_only() -> None:
    contract = _read(_repo_root() / "fluxboard/docs/equities_contract.md")

    assert "/equities/assets/*" not in contract
    assert "/static/fluxboard/assets/*" in contract
    assert "`/equities` stays a SPA route, not the asset prefix." in contract


def test_equities_deploy_docs_keep_equities_routes_spa_only() -> None:
    repo_root = _repo_root()
    readme = _read(repo_root / "deploy/equities/README.md")
    strategies_readme = _read(repo_root / "deploy/equities/strategies/README.md")

    assert "currently exposes `/equities/assets/*` route capability" not in readme
    assert "currently exposes `/equities/assets/*` route capability" not in strategies_readme
    assert (
        "The standalone equities runner keeps `/equities` as the SPA route while shared Fluxboard assets load from `/static/fluxboard/*`."
        in readme
    )
    assert (
        "The standalone equities runner keeps `/equities` as the SPA route while shared Fluxboard assets load from `/static/fluxboard/*`."
        in strategies_readme
    )


def test_equities_removed_makerv3_strategy_ids_are_disjoint_from_live_enrollment() -> None:
    assert set(LIVE_ENROLLED_STRATEGY_IDS).isdisjoint(REMOVED_MAKERV3_STRATEGY_IDS)
    assert set(CORE_PROD_STRATEGY_IDS).issubset(LIVE_ENROLLED_STRATEGY_IDS)
    assert set(DEPLOYED_MULTIVENUE_ROUTE_IDS).issubset(LIVE_ENROLLED_STRATEGY_IDS)


def test_equities_deploy_readme_freezes_enrolled_makerv4_routes() -> None:
    readme = _read(_repo_root() / "deploy/equities/README.md")

    assert "The checked-in Pulse-managed equities universe is now the full `maker_v4` route set below." in readme
    assert _extract_markdown_code_bullets(readme, "Enrolled Hyperliquid Routes", level=3) == CORE_PROD_STRATEGY_IDS
    assert (
        _extract_markdown_code_bullets(readme, "Enrolled Binance Routes", level=3)
        == DEPLOYED_MULTIVENUE_ROUTE_IDS
    )
    assert (
        _extract_markdown_code_bullets(readme, "Removed MakerV3 Files", level=3)
        == REMOVED_MAKERV3_STRATEGY_IDS
    )


def test_equities_strategy_readme_freezes_enrolled_makerv4_routes() -> None:
    readme = _read(_repo_root() / "deploy/equities/strategies/README.md")

    assert _extract_markdown_code_bullets(readme, "Enrolled Hyperliquid Routes", level=3) == CORE_PROD_STRATEGY_IDS
    assert (
        _extract_markdown_code_bullets(readme, "Enrolled Binance Routes", level=3)
        == DEPLOYED_MULTIVENUE_ROUTE_IDS
    )
    assert (
        _extract_markdown_code_bullets(readme, "Removed MakerV3 Files", level=3)
        == REMOVED_MAKERV3_STRATEGY_IDS
    )


def test_equities_deploy_docs_require_post_install_env_verification() -> None:
    readme = _read(_repo_root() / "deploy/equities/README.md")
    common_env = _read(_repo_root() / "deploy/equities/systemd/common.env.example")

    assert "`sed -n '1,120p' /etc/flux/equities-api.env`" in readme
    assert "`sed -n '1,120p' /etc/flux/equities-portfolio.env`" in readme
    assert "`sed -n '1,120p' /etc/flux/equities-bridge.env`" in readme
    assert (
        f"`sed -n '1,120p' /etc/flux/equities-node-{ACTIVE_STRATEGY_IDS[0]}.env`" in readme
    )
    assert "`find /etc/flux -maxdepth 1 -type f -name 'equities-node-*.env' -print | sort`" in readme
    assert "`for env_path in /etc/flux/equities-node-*.env; do sed -n '1,120p' \"$env_path\"; done`" in readme
    assert "`uv sync --all-groups --all-extras`" in readme
    assert "the generated envs append `WORKDIR=` / `PYTHONPATH=` for the selected checkout" in readme
    assert "the generated env commands use the checkout-local `.venv/bin/python`" in readme
    assert "every generated `equities-node-*.env` is rewritten from the intended checkout" in readme
    assert "print and review every rendered `equities-node-*.env` contents" in readme
    assert "Do not restart services until those env files match the intended checkout and live flags." in readme
    assert "verify every matching `/etc/flux/equities-node-*.env`" in common_env
    assert "`uv sync --all-groups --all-extras` in the selected checkout" in common_env


def test_equities_and_tokenmm_installers_use_shared_strategy_stack_conventions() -> None:
    repo_root = _repo_root()
    shared = _read(repo_root / "ops/scripts/deploy/shared_strategy_stack.sh")
    tokenmm_install = _read(repo_root / "ops/scripts/deploy/install_tokenmm_systemd.sh")
    equities_install = _read(repo_root / "ops/scripts/deploy/install_equities_systemd.sh")
    tokenmm_stack = _read(repo_root / "ops/scripts/deploy/tokenmm_stack.sh")
    equities_stack = _read(repo_root / "ops/scripts/deploy/equities_stack.sh")

    assert (
        'source "${DEPLOY_ROOT}/ops/scripts/deploy/shared_strategy_stack.sh"'
        in tokenmm_install
    )
    assert 'source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"' in equities_install
    assert 'source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"' in tokenmm_stack
    assert 'source "${ROOT_DIR}/ops/scripts/deploy/shared_strategy_stack.sh"' in equities_stack
    assert '/../../.." && pwd)' in tokenmm_install
    assert '/../../.." && pwd)' in equities_install
    assert '/../../.." && pwd)' in tokenmm_stack
    assert '/../../.." && pwd)' in equities_stack

    assert "strategy_stack_render_target()" in shared
    assert "strategy_stack_render_merged_sudoers()" in shared
    assert "strategy_stack_write_env()" in shared
    assert "strategy_stack_render_sudoers()" in shared
    assert "strategy_stack_load_strategy_configs()" in shared
    assert "strategy_stack_print_install_hint()" in shared

    assert "strategy_stack_write_env" in tokenmm_install
    assert "strategy_stack_write_env" in equities_install
    assert '"tokenmm"' in tokenmm_install
    assert '"equities"' in equities_install
    assert 'ENV_PATH="${TOKENMM_ENV_PATH:-${DEFAULT_ENV_PATH}}"' in tokenmm_stack
    assert 'ENV_PATH="${EQUITIES_ENV_PATH:-${DEFAULT_ENV_PATH}}"' in equities_stack


def test_equities_stack_script_parses_env_safely_and_loads_trade_xyz_secrets() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/equities_stack.sh")

    assert 'source "${ENV_PATH}"' not in script
    assert "aws secretsmanager get-secret-value" in script
    assert "jq -r 'to_entries[] |" in script
    assert "TRADE_XYZ_AGENT_PK" in script
    assert "TRADE_XYZ_ACCOUNT_ADDRESS" in script
    assert "TRADE_XYZ_VAULT_ADDRESS" in script
    assert "warning: skipping unsupported secret key" in script


def test_equities_stack_script_validates_ibkr_docker_gateway_credentials_when_configured() -> None:
    script = _read(_repo_root() / "ops/scripts/deploy/equities_stack.sh")

    assert "TWS_USERNAME" in script
    assert "TWS_PASSWORD" in script
    assert "missing required IBKR dockerized gateway credentials" in script
    assert "dockerized_gateway" in script


def test_equities_docs_reference_profile_and_portfolio_contracts() -> None:
    repo_root = _repo_root()
    readme = _read(repo_root / "deploy/equities/README.md")
    strategies_readme = _read(repo_root / "deploy/equities/strategies/README.md")
    common_env = _read(repo_root / "deploy/equities/systemd/common.env.example")
    live_config = _read(repo_root / "deploy/equities/equities.live.toml")
    contract = _read(repo_root / "fluxboard/docs/equities_contract.md")

    assert "EQUITIES_API_BACKEND_URL=http://127.0.0.1:5024" in readme
    assert "TRADE_XYZ_AGENT_PK" in readme
    assert "TRADE_XYZ_ACCOUNT_ADDRESS" in readme
    assert "TRADE_XYZ_VAULT_ADDRESS" in readme
    assert "TWS_USERNAME" in readme
    assert "TWS_PASSWORD" in readme
    assert (
        "shared config merge only imports `redis`, `portfolio`, `[[strategy_contracts]]`, "
        "and `[[account_scopes]]`" in readme
    )
    assert "active node settings live in `deploy/equities/strategies/*.toml`" in readme
    assert "`[[account_scopes]]`" in readme
    assert "ibkr.reference.main" in readme
    assert "hyperliquid.xyz.main" in readme
    assert "maker_venue" in readme
    assert "portfolio_asset_id" in readme
    assert "binance.futures.main" in readme
    assert "older manifests must be upgraded" in readme
    assert "AAPL.NASDAQ" in readme
    assert "`/equities` API contract catalog is built from the shared `[[contracts]]` entries" in readme
    assert "Shared IBKR contract entry must mirror the active enrolled route set before restart" in readme
    assert "vault_address_env" in readme
    assert 'use_regular_trading_hours = false' in readme
    assert '`ibkr.reference.main` is the only equities IBKR gateway owner' in readme
    assert 'twofa_timeout_action = "exit"' in readme

    assert "<stock>_tradexyz_makerv4.toml" in strategies_readme
    assert "dead `maker_v3` files have been removed from discovery" in strategies_readme
    assert "AAPL.NASDAQ" in strategies_readme
    assert "use_regular_trading_hours = false" in strategies_readme
    assert "manage_container = false" in strategies_readme
    assert "TRADE_XYZ_VAULT_ADDRESS" in strategies_readme
    assert "multiple strategy routes can share one canonical stock bucket" in strategies_readme.lower()
    assert (
        "Keep the shared `[[contracts]]` IBKR entry aligned with the active enrolled reference instrument set before restart."
        in strategies_readme
    )
    assert "TWS_USERNAME" in strategies_readme
    assert (
        "node runners inherit the shared `[redis]`, `[portfolio]`, `[[strategy_contracts]]`, "
        "and `[[account_scopes]]` contract tables" in strategies_readme
    )
    assert "TWS_PASSWORD" in strategies_readme

    assert "EQUITIES_REDIS_HOST=" in common_env
    assert "EQUITIES_REDIS_PASSWORD=" in common_env
    assert "EQUITIES_API_BACKEND_URL=http://127.0.0.1:5024" in common_env
    assert "Shared `account_scopes` in `deploy/equities/equities.live.toml` own the profile-level providers." in common_env
    assert "ibkr.reference.main" in common_env
    assert "TRADE_XYZ_AGENT_PK=" in common_env
    assert "TRADE_XYZ_ACCOUNT_ADDRESS=" in common_env
    assert "TRADE_XYZ_VAULT_ADDRESS=" in common_env

    assert 'portfolio_id = "equities"' in live_config
    assert "[[account_scopes]]" in live_config
    assert 'scope_id = "ibkr.reference.main"' in live_config
    assert 'scope_id = "hyperliquid.xyz.main"' in live_config
    assert 'scope_id = "binance.futures.main"' in live_config
    assert 'strategy_id = "pltr_binance_perp_makerv4"' in live_config
    assert 'maker_venue = "BINANCE_PERP"' in live_config
    assert 'instrument_id = "PLTRUSDT-PERP.BINANCE_PERP"' in live_config
    assert "equities_strategy_ids" in live_config
    assert f'strategy_class = "{ACTIVE_STRATEGY_CLASS}"' in live_config
    for strategy_id in LIVE_ENROLLED_STRATEGY_IDS:
        assert strategy_id in live_config
    for strategy_id in REMOVED_MAKERV3_STRATEGY_IDS:
        assert strategy_id not in live_config

    assert "/equities" in contract
    assert "/api/v1/signals?profile=equities" in contract
    assert "/api/v1/params?profile=equities" in contract
    assert "trade[XYZ]" in contract
    assert "AAPL.NASDAQ" in contract
    assert "MakerV4" in contract
