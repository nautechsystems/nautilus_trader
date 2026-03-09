from __future__ import annotations

import configparser
import hashlib
import hmac
import json
import logging
import os
import time
from dataclasses import dataclass
from datetime import datetime
from datetime import timezone
from decimal import Decimal
from decimal import InvalidOperation
from pathlib import Path
from typing import Any
from typing import Mapping
from typing import Sequence
from urllib.parse import urlencode

import requests
from requests.adapters import HTTPAdapter
from urllib3.util.retry import Retry

try:
    from zoneinfo import ZoneInfo
except ImportError:  # pragma: no cover - py38 fallback
    ZoneInfo = None  # type: ignore[assignment]

try:
    from dateutil import tz as dateutil_tz  # type: ignore[import]
except ImportError:  # pragma: no cover - optional dep
    dateutil_tz = None  # type: ignore[assignment]


LOG = logging.getLogger(__name__)

_TRANSIENT_HTTP_CODES = frozenset({429, 500, 502, 503, 504})
_THREAD_ERR_TOKENS = (
    "message thread not found",
    "message_thread_id",
    "thread not found",
    "topic",
)
_CONFIG_SECTION_NAME = "lan_rogue_trader_alert"
_LEGACY_CONFIG_SECTION_NAME = "lan_usdt_watch"
_DEFAULT_STATE_PATH = Path("state/lan_rogue_trader_alert.json")


def _default_http_timeout_seconds() -> float:
    return float(os.getenv("HTTP_DEFAULT_TIMEOUT", "5"))


def build_http_session() -> requests.Session:
    """Build a local equivalent of the source retrying engine HTTP session."""
    session = requests.Session()
    retry = Retry(
        total=3,
        backoff_factor=0.5,
        status_forcelist=(500, 502, 503, 504),
        allowed_methods=("GET", "POST", "PUT", "DELETE", "PATCH"),
        raise_on_status=False,
    )
    adapter = HTTPAdapter(max_retries=retry, pool_connections=10, pool_maxsize=50)
    session.mount("http://", adapter)
    session.mount("https://", adapter)

    default_timeout = _default_http_timeout_seconds()
    original_request = session.request

    def _timeout_request(method: str, url: str, **kwargs: Any) -> requests.Response:
        if "timeout" not in kwargs:
            kwargs["timeout"] = default_timeout
        return original_request(method, url, **kwargs)

    session.request = _timeout_request  # type: ignore[assignment]
    return session


class MissingAssetError(RuntimeError):
    """Raised when requested asset row is missing from Binance PM balance payload."""


@dataclass(frozen=True)
class WatchConfig:
    poll_secs: int
    cooldown_secs: int
    binance_base_url: str
    asset: str
    binance_api_key: str
    binance_api_secret: str
    account_label: str
    telegram_bot_token: str
    telegram_chat_id: int
    telegram_thread_id: int | None
    strict_thread: bool
    state_path: Path
    emergency_bypass_usdt: Decimal
    timezone_name: str
    send_baseline: bool


@dataclass
class WatchState:
    last_balance: Decimal
    last_alert_at: int
    last_alert_balance: Decimal
    pending: bool
    pending_start_balance: Decimal
    pending_start_at: int
    pending_last_balance: Decimal
    pending_last_at: int
    pending_count: int

    @classmethod
    def initial(cls, balance: Decimal) -> WatchState:
        return cls(
            last_balance=balance,
            last_alert_at=0,
            last_alert_balance=balance,
            pending=False,
            pending_start_balance=balance,
            pending_start_at=0,
            pending_last_balance=balance,
            pending_last_at=0,
            pending_count=0,
        )

    @classmethod
    def from_dict(cls, payload: Mapping[str, Any]) -> WatchState:
        return cls(
            last_balance=_as_decimal(payload.get("last_balance"), "last_balance"),
            last_alert_at=int(payload.get("last_alert_at") or 0),
            last_alert_balance=_as_decimal(payload.get("last_alert_balance"), "last_alert_balance"),
            pending=bool(payload.get("pending", False)),
            pending_start_balance=_as_decimal(
                payload.get("pending_start_balance"), "pending_start_balance"
            ),
            pending_start_at=int(payload.get("pending_start_at") or 0),
            pending_last_balance=_as_decimal(payload.get("pending_last_balance"), "pending_last_balance"),
            pending_last_at=int(payload.get("pending_last_at") or 0),
            pending_count=int(payload.get("pending_count") or 0),
        )

    def to_dict(self) -> dict[str, Any]:
        return {
            "last_balance": str(self.last_balance),
            "last_alert_at": int(self.last_alert_at),
            "last_alert_balance": str(self.last_alert_balance),
            "pending": bool(self.pending),
            "pending_start_balance": str(self.pending_start_balance),
            "pending_start_at": int(self.pending_start_at),
            "pending_last_balance": str(self.pending_last_balance),
            "pending_last_at": int(self.pending_last_at),
            "pending_count": int(self.pending_count),
        }


class JsonStateStore:
    def __init__(self, path: Path) -> None:
        self.path = path

    def load(self) -> WatchState | None:
        if not self.path.exists():
            return None
        try:
            payload = json.loads(self.path.read_text(encoding="utf-8"))
            if not isinstance(payload, Mapping):
                raise RuntimeError("state payload must be a JSON object")
            return WatchState.from_dict(payload)
        except Exception as exc:
            LOG.error("Failed to load state file %s: %s", self.path, exc)
            return None

    def save(self, state: WatchState) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        tmp_path = self.path.with_suffix(self.path.suffix + ".tmp")
        data = json.dumps(state.to_dict(), separators=(",", ":"), sort_keys=True)
        with tmp_path.open("w", encoding="utf-8") as handle:
            handle.write(data)
            handle.write("\n")
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(tmp_path, self.path)


class BinancePmClient:
    def __init__(
        self,
        base_url: str,
        asset: str,
        api_key: str,
        api_secret: str,
        session: requests.Session,
        recv_window_ms: int = 5000,
        timeout_sec: float = 10.0,
    ) -> None:
        self.base_url = base_url.rstrip("/")
        self.asset = str(asset).upper()
        self.api_key = api_key
        self.api_secret = api_secret
        self.session = session
        self.recv_window_ms = int(recv_window_ms)
        self.timeout_sec = float(timeout_sec)

    @staticmethod
    def sign_params(params: Sequence[tuple[str, str]], secret: str) -> str:
        query = urlencode(list(params))
        return hmac.new(secret.encode("utf-8"), query.encode("utf-8"), hashlib.sha256).hexdigest()

    def fetch_balance(self) -> Decimal:
        now_ms = int(time.time() * 1000)
        params: list[tuple[str, str]] = [
            ("asset", self.asset),
            ("timestamp", str(now_ms)),
            ("recvWindow", str(self.recv_window_ms)),
        ]
        signature = self.sign_params(params, self.api_secret)
        signed_params = list(params)
        signed_params.append(("signature", signature))
        headers = {"X-MBX-APIKEY": self.api_key}

        response = self.session.get(
            f"{self.base_url}/papi/v1/balance",
            params=signed_params,
            headers=headers,
            timeout=self.timeout_sec,
        )
        if response.status_code >= 400:
            text = _response_text(response)
            raise RuntimeError(f"Binance PM error HTTP {response.status_code}: {text}")

        payload = response.json()
        rows: list[Mapping[str, Any]] = []
        if isinstance(payload, Mapping):
            if "code" in payload and "msg" in payload:
                raise RuntimeError(f"Binance PM error code {payload.get('code')}: {payload.get('msg')}")
            rows = [payload]
        elif isinstance(payload, list):
            rows = [row for row in payload if isinstance(row, Mapping)]
        else:
            raise RuntimeError("Binance PM balance payload is not a list/object")

        for row in rows:
            row_asset = str(row.get("asset") or "").upper()
            if row_asset != self.asset:
                continue
            raw_balance = row.get("totalWalletBalance")
            return _as_decimal(raw_balance, "totalWalletBalance")

        raise MissingAssetError(f"Asset {self.asset} missing in Binance PM balance payload")


class TelegramNotifier:
    def __init__(
        self,
        bot_token: str,
        chat_id: int,
        thread_id: int | None,
        strict_thread: bool,
        session: requests.Session,
        base_url: str = "https://api.telegram.org",
        max_retries: int = 3,
    ) -> None:
        self.bot_token = bot_token
        self.chat_id = int(chat_id)
        self.thread_id = thread_id
        self.strict_thread = strict_thread
        self.session = session
        self.base_url = base_url.rstrip("/")
        self.max_retries = max(1, int(max_retries))

    def send_message(self, text: str) -> bool:
        if self.thread_id is None:
            sent, _status, _desc = self._send(text=text, thread_id=None)
            return sent

        sent, status, desc = self._send(text=text, thread_id=self.thread_id)
        if sent:
            return True

        if _is_thread_error(status, desc):
            if self.strict_thread:
                LOG.error("Thread delivery failed and strict_thread=1; alert dropped: %s", desc)
                return False
            fallback_text = "⚠️ WARNING: thread_id failed, posting to group root\n\n" + text
            fallback_sent, _status, _desc = self._send(text=fallback_text, thread_id=None)
            return fallback_sent

        return False

    def _send(self, text: str, thread_id: int | None) -> tuple[bool, int | None, str]:
        url = f"{self.base_url}/bot{self.bot_token}/sendMessage"
        payload: dict[str, Any] = {
            "chat_id": self.chat_id,
            "text": text,
            "disable_web_page_preview": True,
        }
        if thread_id is not None:
            payload["message_thread_id"] = int(thread_id)

        for attempt in range(self.max_retries):
            try:
                response = self.session.post(url, json=payload, timeout=10.0)
                status = int(response.status_code)
                desc = _telegram_description(response)

                if 200 <= status < 300:
                    data = _safe_json(response)
                    if isinstance(data, Mapping) and data.get("ok") is False:
                        if _is_transient(status):
                            _sleep_retry(attempt)
                            continue
                        LOG.error("Telegram send failed: status=%s desc=%s", status, desc)
                        return False, status, desc
                    return True, status, ""

                if _is_transient(status) and attempt < self.max_retries - 1:
                    _sleep_retry(attempt)
                    continue

                LOG.error("Telegram send failed: status=%s desc=%s", status, desc)
                return False, status, desc
            except requests.RequestException as exc:
                if attempt < self.max_retries - 1:
                    _sleep_retry(attempt)
                    continue
                LOG.error("Telegram send transport failure: %s", exc)
                return False, None, str(exc)

        return False, None, "send failed"


class LanRogueTraderAlertService:
    def __init__(
        self,
        config: WatchConfig,
        binance_client: BinancePmClient,
        telegram: TelegramNotifier,
        store: JsonStateStore,
        sleep_fn=time.sleep,
        now_fn=time.time,
    ) -> None:
        self.config = config
        self.binance_client = binance_client
        self.telegram = telegram
        self.store = store
        self._sleep = sleep_fn
        self._now = now_fn
        self.state: WatchState | None = None
        self._missing_asset_episode_open = False

    def run_forever(self) -> None:
        self.state = self.store.load()
        while True:
            self.poll_once()
            self._sleep(self.config.poll_secs)

    def poll_once(self) -> None:
        now = int(self._now())
        try:
            balance = self.binance_client.fetch_balance()
            self._missing_asset_episode_open = False
        except MissingAssetError as exc:
            self._handle_missing_asset(now=now, error_text=str(exc))
            return
        except Exception:
            LOG.exception("Failed to fetch Binance PM USDT balance")
            return

        if self.state is None:
            self.state = WatchState.initial(balance)
            self.store.save(self.state)
            if self.config.send_baseline:
                baseline_text = render_baseline(config=self.config, balance=balance, now=now)
                if self.telegram.send_message(baseline_text):
                    LOG.info("Sent baseline message to Telegram")
                else:
                    LOG.warning("Failed to send baseline message to Telegram")
            return

        self._apply_balance(balance=balance, now=now)

    def _handle_missing_asset(self, now: int, error_text: str) -> None:
        if self._missing_asset_episode_open:
            LOG.warning("Binance PM USDT row still missing: %s", error_text)
            return
        message = (
            "⚠️ Binance PM watch error\n"
            f"Account: {self.config.account_label}\n"
            f"Issue: {error_text}\n"
            f"When: {format_local_utc(now, self.config.timezone_name)}"
        )
        if self.telegram.send_message(message):
            LOG.info("Sent missing-asset alert to Telegram")
        else:
            LOG.warning("Failed to send missing-asset alert to Telegram")
        self._missing_asset_episode_open = True

    def _apply_balance(self, balance: Decimal, now: int) -> None:
        assert self.state is not None
        state = self.state

        if balance == state.last_balance:
            if (
                state.pending
                and state.last_alert_at > 0
                and now >= state.last_alert_at + self.config.cooldown_secs
            ):
                summary = render_deferred_summary(config=self.config, state=state, now=now)
                if self.telegram.send_message(summary):
                    LOG.info("Sent deferred summary to Telegram")
                    self._mark_alert_sent(balance=balance, now=now)
                    self.store.save(state)
            return

        prev_balance = state.last_balance
        state.last_balance = balance

        if not state.pending:
            state.pending = True
            state.pending_start_balance = prev_balance
            state.pending_start_at = now
            state.pending_count = 1
        else:
            state.pending_count += 1

        state.pending_last_balance = balance
        state.pending_last_at = now

        should_alert_now = state.last_alert_at == 0 or now >= state.last_alert_at + self.config.cooldown_secs
        if not should_alert_now and self.config.emergency_bypass_usdt > Decimal("0"):
            since_last_alert = abs(balance - state.last_alert_balance)
            if since_last_alert >= self.config.emergency_bypass_usdt:
                should_alert_now = True

        if should_alert_now:
            immediate = render_immediate_alert(
                config=self.config,
                prev_balance=prev_balance,
                balance=balance,
                now=now,
                cooldown_secs=self.config.cooldown_secs,
            )
            if self.telegram.send_message(immediate):
                LOG.info("Sent immediate alert to Telegram")
                self._mark_alert_sent(balance=balance, now=now)

        self.store.save(state)

    def _mark_alert_sent(self, balance: Decimal, now: int) -> None:
        assert self.state is not None
        self.state.last_alert_at = int(now)
        self.state.last_alert_balance = balance
        self.state.pending = False
        self.state.pending_start_balance = balance
        self.state.pending_start_at = 0
        self.state.pending_last_balance = balance
        self.state.pending_last_at = 0
        self.state.pending_count = 0


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def default_config_path() -> Path:
    return _repo_root() / "deploy" / "tg_bots" / "lan_rogue_trader_alert.ini"


def load_config(config_path: str | Path | None = None) -> WatchConfig:
    resolved_path = Path(config_path) if config_path is not None else default_config_path()
    parser = configparser.ConfigParser()
    read_files = parser.read(resolved_path)
    if not read_files:
        raise RuntimeError(f"Config file not found: {resolved_path}")
    if _CONFIG_SECTION_NAME in parser:
        cfg = parser[_CONFIG_SECTION_NAME]
    elif _LEGACY_CONFIG_SECTION_NAME in parser:
        cfg = parser[_LEGACY_CONFIG_SECTION_NAME]
    else:
        raise RuntimeError(
            f"Missing [{_CONFIG_SECTION_NAME}] or [{_LEGACY_CONFIG_SECTION_NAME}] section in {resolved_path}"
        )

    poll_secs = _get_int(cfg, "poll_secs", 60)
    cooldown_secs = _get_int(cfg, "cooldown_secs", 10800)

    binance_base_url = _get_required(cfg, "binance_base_url")
    asset = (cfg.get("asset", "USDT") or "USDT").strip().upper()
    binance_key_env = _get_required(cfg, "api_key_env")
    binance_secret_env = _get_required(cfg, "api_secret_env")
    account_label = _get_required(cfg, "account_label")

    tg_token_env = _get_required(cfg, "telegram_bot_token_env")
    chat_id = _get_int(cfg, "telegram_chat_id", 0)
    if chat_id == 0:
        raise RuntimeError("telegram_chat_id must be a non-zero integer")

    thread_raw = (cfg.get("telegram_thread_id") or "").strip()
    thread_id = int(thread_raw) if thread_raw else None

    strict_thread = _get_bool(cfg, "strict_thread", False)
    state_path = Path((cfg.get("state_path") or str(_DEFAULT_STATE_PATH)).strip())
    emergency_bypass = _get_decimal(cfg, "emergency_bypass_usdt", Decimal("0"))
    timezone_name = (cfg.get("timezone") or "Asia/Bangkok").strip() or "Asia/Bangkok"
    send_baseline = _get_bool(cfg, "send_baseline", True)

    binance_api_key = _load_secret(binance_key_env)
    binance_api_secret = _load_secret(binance_secret_env)
    telegram_bot_token = _load_secret(tg_token_env)

    return WatchConfig(
        poll_secs=poll_secs,
        cooldown_secs=cooldown_secs,
        binance_base_url=binance_base_url,
        asset=asset,
        binance_api_key=binance_api_key,
        binance_api_secret=binance_api_secret,
        account_label=account_label,
        telegram_bot_token=telegram_bot_token,
        telegram_chat_id=chat_id,
        telegram_thread_id=thread_id,
        strict_thread=strict_thread,
        state_path=state_path,
        emergency_bypass_usdt=emergency_bypass,
        timezone_name=timezone_name,
        send_baseline=send_baseline,
    )


def render_baseline(config: WatchConfig, balance: Decimal, now: int) -> str:
    return (
        "✅ Lan USDT Watch baseline\n"
        f"Account: {config.account_label}\n"
        f"Balance: {fmt_amount(balance)} USDT\n"
        "Cooldown: not started\n"
        f"When: {format_local_utc(now, config.timezone_name)}"
    )


def render_immediate_alert(
    config: WatchConfig,
    prev_balance: Decimal,
    balance: Decimal,
    now: int,
    cooldown_secs: int,
) -> str:
    delta = balance - prev_balance
    next_allowed = now + int(cooldown_secs)
    return (
        "🚨 USDT balance changed (Binance PM)\n"
        f"Account: {config.account_label}\n"
        f"Prev: {fmt_amount(prev_balance)}\n"
        f"Now: {fmt_amount(balance)}\n"
        f"Delta: {fmt_amount(delta)} USDT\n"
        f"When: {format_local_utc(now, config.timezone_name)}\n"
        f"Cooldown: next alert allowed after {format_local_utc(next_allowed, config.timezone_name)}"
    )


def render_deferred_summary(config: WatchConfig, state: WatchState, now: int) -> str:
    net = state.pending_last_balance - state.pending_start_balance
    return (
        "🕒 USDT balance changes during cooldown (summary)\n"
        f"Account: {config.account_label}\n"
        f"From: {fmt_amount(state.pending_start_balance)} ({format_local_utc(state.pending_start_at, config.timezone_name)})\n"
        f"To: {fmt_amount(state.pending_last_balance)} ({format_local_utc(state.pending_last_at, config.timezone_name)})\n"
        f"Net: {fmt_amount(net)} USDT\n"
        f"Changes seen: {state.pending_count}\n"
        f"Summary sent at: {format_local_utc(now, config.timezone_name)}"
    )


def fmt_amount(amount: Decimal) -> str:
    return f"{amount:,.2f}"


def format_local_utc(ts: int, timezone_name: str) -> str:
    utc_dt = datetime.fromtimestamp(int(ts), tz=timezone.utc)
    local_dt = _to_local_tz(utc_dt, timezone_name)
    local_str = local_dt.strftime("%Y-%m-%d %H:%M:%S")
    utc_str = utc_dt.strftime("%Y-%m-%d %H:%M:%S")
    return f"{local_str} {timezone_name} ({utc_str} UTC)"


def _to_local_tz(dt: datetime, timezone_name: str) -> datetime:
    if ZoneInfo is not None:
        try:
            return dt.astimezone(ZoneInfo(timezone_name))
        except Exception:
            pass

    if dateutil_tz is not None:
        try:
            tzinfo = dateutil_tz.gettz(timezone_name)
            if tzinfo is not None:
                return dt.astimezone(tzinfo)
        except Exception:
            pass

    LOG.warning("Invalid or unsupported timezone %s, falling back to UTC", timezone_name)
    return dt


def _is_transient(status_code: int | None) -> bool:
    return status_code in _TRANSIENT_HTTP_CODES


def _is_thread_error(status_code: int | None, description: str) -> bool:
    if status_code != 400:
        return False
    desc = description.lower()
    return any(token in desc for token in _THREAD_ERR_TOKENS)


def _sleep_retry(attempt: int) -> None:
    backoff = min(5.0, 0.5 * (2**attempt))
    time.sleep(backoff)


def _response_text(response: requests.Response) -> str:
    text = (response.text or "").strip()
    return text[:500] if text else "<empty>"


def _safe_json(response: requests.Response) -> Any:
    try:
        return response.json()
    except Exception:
        return None


def _telegram_description(response: requests.Response) -> str:
    data = _safe_json(response)
    if isinstance(data, Mapping):
        desc = data.get("description")
        if isinstance(desc, str) and desc.strip():
            return desc.strip()
    return _response_text(response)


def _as_decimal(raw: Any, field_name: str) -> Decimal:
    try:
        return Decimal(str(raw))
    except (InvalidOperation, TypeError, ValueError) as exc:
        raise RuntimeError(f"Invalid decimal for {field_name}: {raw!r}") from exc


def _get_required(cfg: configparser.SectionProxy, key: str) -> str:
    value = (cfg.get(key) or "").strip()
    if not value:
        raise RuntimeError(f"Missing required config key: {key}")
    return value


def _get_int(cfg: configparser.SectionProxy, key: str, default: int) -> int:
    raw = (cfg.get(key) or "").strip()
    if not raw:
        return int(default)
    try:
        return int(raw)
    except ValueError as exc:
        raise RuntimeError(f"Invalid integer for {key}: {raw!r}") from exc


def _get_bool(cfg: configparser.SectionProxy, key: str, default: bool) -> bool:
    raw = (cfg.get(key) or "").strip()
    if not raw:
        return bool(default)
    lowered = raw.lower()
    if lowered in {"1", "true", "yes", "on"}:
        return True
    if lowered in {"0", "false", "no", "off"}:
        return False
    raise RuntimeError(f"Invalid boolean for {key}: {raw!r}")


def _get_decimal(cfg: configparser.SectionProxy, key: str, default: Decimal) -> Decimal:
    raw = (cfg.get(key) or "").strip()
    if not raw:
        return default
    return _as_decimal(raw, key)


def _load_secret(env_key: str) -> str:
    value = (os.getenv(env_key) or "").strip()
    if not value:
        raise RuntimeError(f"Missing secret env var: {env_key}")
    return value


__all__ = [
    "BinancePmClient",
    "build_http_session",
    "JsonStateStore",
    "LanRogueTraderAlertService",
    "MissingAssetError",
    "TelegramNotifier",
    "WatchConfig",
    "WatchState",
    "default_config_path",
    "fmt_amount",
    "format_local_utc",
    "load_config",
    "render_baseline",
    "render_deferred_summary",
    "render_immediate_alert",
]
