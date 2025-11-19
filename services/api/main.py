from __future__ import annotations

import asyncio
import json
import logging
import os
import shutil
import sys
import threading
import time
import uuid
from datetime import datetime
from enum import Enum
from functools import lru_cache
from pathlib import Path
from typing import Any, cast

import msgspec
from fastapi import FastAPI
from fastapi import HTTPException
from fastapi import Request
from fastapi import Response
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import FileResponse
from pydantic import BaseModel


# Agents team service (optional integration)
try:
    from services.api.agents.service import TEAM  # type: ignore
except Exception:  # pragma: no cover
    TEAM = None  # type: ignore

# Lazy-import Nautilus components so the API can start without full native build.
# Endpoints that require them will import on demand and return 503 if unavailable.
try:
    from nautilus_trader.config import BacktestRunConfig  # type: ignore
    from nautilus_trader.config import TradingNodeConfig  # type: ignore
    from nautilus_trader.config import msgspec_decoding_hook  # type: ignore
except Exception:  # pragma: no cover
    BacktestRunConfig = None  # type: ignore
    TradingNodeConfig = None  # type: ignore
    def msgspec_decoding_hook(x):  # type: ignore
        return x

def _get_BacktestNode():
    try:
        from nautilus_trader.backtest.node import BacktestNode as _BN  # type: ignore
        return _BN
    except Exception as e:
        raise HTTPException(status_code=503, detail=f"Backtest engine unavailable: {e}")

def _get_TradingNode():
    try:
        from nautilus_trader.live.node import TradingNode as _TN  # type: ignore
        return _TN
    except Exception as e:
        raise HTTPException(status_code=503, detail=f"Live trading engine unavailable: {e}")


def _register_client_factories(node: Any, live_cfg: Any) -> None:
    """
    Register adapter client factories on the TradingNode based on provided venue keys.

    This enables the node to instantiate data/exec clients from config.
    """
    try:
        venues: set[str] = set()
        try:
            for k in (getattr(live_cfg, "data_clients", {}) or {}).keys():
                venues.add(str(k))
        except Exception:
            pass
        try:
            for k in (getattr(live_cfg, "exec_clients", {}) or {}).keys():
                venues.add(str(k))
        except Exception:
            pass

        # Binance
        if "BINANCE" in venues:
            try:
                from nautilus_trader.adapters.binance import BINANCE as BINANCE_CODE  # type: ignore
                from nautilus_trader.adapters.binance import BinanceLiveDataClientFactory  # type: ignore
                from nautilus_trader.adapters.binance import BinanceLiveExecClientFactory  # type: ignore
                node.add_data_client_factory(BINANCE_CODE, BinanceLiveDataClientFactory)
                node.add_exec_client_factory(BINANCE_CODE, BinanceLiveExecClientFactory)
            except Exception as e:
                logger.warning("live.start: failed to register Binance factories: %s", e)
        # Coinbase International
        if "COINBASE_INTX" in venues:
            try:
                from nautilus_trader.adapters.coinbase_intx import COINBASE_INTX as CB_CODE  # type: ignore
                from nautilus_trader.adapters.coinbase_intx import CoinbaseIntxLiveDataClientFactory  # type: ignore
                from nautilus_trader.adapters.coinbase_intx import CoinbaseIntxLiveExecClientFactory  # type: ignore
                node.add_data_client_factory(CB_CODE, CoinbaseIntxLiveDataClientFactory)
                node.add_exec_client_factory(CB_CODE, CoinbaseIntxLiveExecClientFactory)
            except Exception as e:
                logger.warning("live.start: failed to register Coinbase Intx factories: %s", e)
    except Exception as e:
        logger.debug("live.start: factory registration error: %s", e)


logger = logging.getLogger(__name__)

app = FastAPI(title="NautilusTrader API", version="0.1.0")


@app.on_event("startup")
def _startup_load_state() -> None:
    # Load jobs state
    try:
        if _JOBS_STATE_PATH.exists():
            obj = json.loads(_JOBS_STATE_PATH.read_text(encoding="utf-8"))
            items = obj.get("jobs") or []
            recovered = 0
            for rec in items:
                try:
                    job = Job.model_validate(rec)  # pydantic v2
                except Exception:
                    try:
                        job = Job(**rec)
                    except Exception:
                        continue
                # Downgrade running jobs to stopped since process restarted
                if job.status in {JobStatus.running, JobStatus.starting}:
                    job.status = JobStatus.stopped
                    if job.finished_at is None:
                        job.finished_at = time.time()
                    if not job.error:
                        job.error = "Recovered after API restart"
                    recovered += 1
                _JOBS[job.id] = job
            if _JOBS:
                global _JOBS_VER
                _JOBS_VER = max(_JOBS_VER, int(obj.get("ver") or 0) + 1)
            logger.info("jobs.startup: loaded %d jobs (recovered %d)", len(_JOBS), recovered)
    except Exception as e:
        logger.warning("jobs.startup: failed to load state: %s", e)

    # Load dataset jobs
    try:
        if _DATA_JOBS_STATE_PATH.exists():
            obj = json.loads(_DATA_JOBS_STATE_PATH.read_text(encoding="utf-8"))
            items = obj.get("jobs") or []
            for rec in items:
                _DATA_JOBS[str(rec.get("id") or uuid.uuid4())] = rec
            global _DATA_JOBS_VER
            _DATA_JOBS_VER = max(_DATA_JOBS_VER, int(obj.get("ver") or 0) + 1)
    except Exception as e:
        logger.debug("data_jobs.startup: failed to load state: %s", e)

# CORS (allow Next.js dev and configurable origins)
ALLOWED_ORIGINS = os.getenv("ALLOWED_ORIGINS", "*")
_origins = [o.strip() for o in ALLOWED_ORIGINS.split(",") if o.strip()] or ["*"]
app.add_middleware(
    CORSMiddleware,
    allow_origins=_origins,
    allow_credentials=True,
    allow_methods=["*"],
    allow_headers=["*"],
)

# Artifacts directory
ARTIFACTS_ROOT = Path(os.getenv("NAUTILUS_ARTIFACTS_DIR", Path(__file__).parent / "data" / "artifacts"))
ARTIFACTS_ROOT.mkdir(parents=True, exist_ok=True)

# Logs persistence directory (optional)
LOGS_ROOT = Path(os.getenv("NAUTILUS_LOGS_DIR", ARTIFACTS_ROOT / "logs"))
LOGS_ROOT.mkdir(parents=True, exist_ok=True)

# Datasets root (for catalog/listing and imports)
DATASETS_ROOT = Path(os.getenv("NAUTILUS_DATASETS_DIR", ARTIFACTS_ROOT / "datasets"))
DATASETS_ROOT.mkdir(parents=True, exist_ok=True)

# Optional providers
OPENAI_API_KEY = os.getenv("OPENAI_API_KEY")
ANTHROPIC_API_KEY = os.getenv("ANTHROPIC_API_KEY")
GEMINI_API_KEY = os.getenv("GEMINI_API_KEY") or os.getenv("GOOGLE_API_KEY")
OLLAMA_API_URL = os.getenv("OLLAMA_API_URL", "http://localhost:11434")

class JobKind(str, Enum):
    backtest = "backtest"
    live = "live"


class JobStatus(str, Enum):
    starting = "starting"
    running = "running"
    completed = "completed"
    failed = "failed"
    stopped = "stopped"


class Job(BaseModel):
    id: str
    kind: JobKind
    status: JobStatus
    started_at: float
    finished_at: float | None = None
    error: str | None = None
    result: dict[str, Any] | None = None

_JOBS: dict[str, Job] = {}
_LIVE: dict[str, dict[str, Any]] = {}  # job_id -> { node, thread }
_BACKTESTS: dict[str, dict[str, Any]] = {}  # job_id -> { node, engines: {run_id: engine} }
_PAPER_ORDERS: dict[str, list[dict[str, Any]]] = {}  # job_id -> paper orders
_JOBS_VER: int = 0
# Compliance mode: 'soft' (default) or 'hard'
_COMPLIANCE_MODE: str = (os.getenv("NAUTILUS_COMPLIANCE_MODE", "soft") or "soft").strip().lower()
# Max age for compliance approvals in seconds (hard mode)
_COMPLIANCE_MAX_AGE_SEC: int = int(os.getenv("NAUTILUS_COMPLIANCE_MAX_AGE_SEC", "300"))

# Persistence paths
_JOBS_STATE_PATH = ARTIFACTS_ROOT / "jobs_state.json"
_DATA_JOBS_STATE_PATH = ARTIFACTS_ROOT / "data_jobs_state.json"

# In-memory job logs (best-effort)
_LOGS: dict[str, list[dict[str, Any]]] = {}
_LOGS_CAP: int = int(os.getenv("NAUTILUS_LOGS_CAP", "2000"))

# Optional retention for on-disk logs (files will be rotated/pruned by size/count best-effort)
_LOG_FILE_MAX_BYTES = int(os.getenv("NAUTILUS_LOG_FILE_MAX_BYTES", str(5 * 1024 * 1024)))  # 5MB
_LOG_FILE_MAX_ROTATIONS = int(os.getenv("NAUTILUS_LOG_FILE_ROTATIONS", "3"))


def _persist_jobs_state() -> None:
    try:
        payload = {
            "ver": _JOBS_VER,
            "updated_at": time.time(),
            "jobs": [j.model_dump() for j in _JOBS.values()],
        }
        _JOBS_STATE_PATH.parent.mkdir(parents=True, exist_ok=True)
        _JOBS_STATE_PATH.write_text(json.dumps(payload), encoding="utf-8")
    except Exception as e:
        logger.debug("persist.jobs: failed: %s", e)


def _persist_data_jobs_state() -> None:
    try:
        payload = {
            "ver": _DATA_JOBS_VER,
            "updated_at": time.time(),
            "jobs": list(_DATA_JOBS.values()),
        }
        _DATA_JOBS_STATE_PATH.parent.mkdir(parents=True, exist_ok=True)
        _DATA_JOBS_STATE_PATH.write_text(json.dumps(payload), encoding="utf-8")
    except Exception as e:
        logger.debug("persist.data_jobs: failed: %s", e)


def _bump_jobs_ver() -> None:
    global _JOBS_VER
    _JOBS_VER += 1
    _persist_jobs_state()


def _log_paths(job_id: str) -> tuple[Path, Path]:
    d = (LOGS_ROOT / job_id)
    d.mkdir(parents=True, exist_ok=True)
    return d / "logs.ndjson", d / "logs.txt"


def _maybe_rotate(path: Path) -> None:
    try:
        if path.exists() and path.stat().st_size > _LOG_FILE_MAX_BYTES:
            # rotate: path -> path.1, shift older
            for i in range(_LOG_FILE_MAX_ROTATIONS, 0, -1):
                p_old = path.with_suffix(path.suffix + f".{i}")
                p_new = path.with_suffix(path.suffix + f".{i+1}")
                if p_new.exists():
                    try:
                        p_new.unlink()
                    except Exception as e:
                        logger.debug("rotate: unlink failed for %s: %s", p_new, e)
                if p_old.exists():
                    try:
                        p_old.rename(p_new)
                    except Exception as e:
                        logger.debug("rotate: rename failed %s -> %s: %s", p_old, p_new, e)
            path.rename(path.with_suffix(path.suffix + ".1"))
    except Exception as e:
        logger.warning("rotate: rotation failed for %s: %s", path, e)


def _persist_log(job_id: str, entry: dict[str, Any]) -> None:
    ndjson, txt = _log_paths(job_id)
    try:
        _maybe_rotate(ndjson)
        _maybe_rotate(txt)
        with ndjson.open("a", encoding="utf-8") as f:
            f.write(json.dumps(entry, ensure_ascii=False) + "\n")
        tstr = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(entry.get("ts", time.time())))
        level = str(entry.get("level", "INFO")).upper()
        msg = str(entry.get("message", ""))
        with txt.open("a", encoding="utf-8") as f2:
            f2.write(f"{tstr} [{level}] {msg}\n")
    except Exception as e:
        logger.warning("persist_log failed for job %s: %s", job_id, e)


def _log(job_id: str, message: str, level: str = "info") -> None:
    entry = {"ts": time.time(), "level": level, "message": str(message)}
    buf = _LOGS.setdefault(job_id, [])
    buf.append(entry)
    # Cap buffer
    if len(buf) > _LOGS_CAP:
        del buf[: len(buf) - _LOGS_CAP]
    # Persist best-effort
    _persist_log(job_id, entry)


@app.get("/health")
def health() -> dict[str, str]:
    return {"status": "ok"}


@app.post("/backtests/run")
def run_backtests(configs: list[dict]) -> dict[str, str]:
    job_id = str(uuid.uuid4())
    job = Job(id=job_id, kind=JobKind.backtest, status=JobStatus.running, started_at=time.time())
    _JOBS[job_id] = job
    _LOGS[job_id] = []
    _log(job_id, "Backtest job created", "info")
    _bump_jobs_ver()

    def _worker():
        try:
            _log(job_id, "Decoding run configs", "debug")
            # Decode list[BacktestRunConfig] via msgspec to ensure types
            cfg_bytes = msgspec.json.encode(configs)
            run_cfgs = msgspec.json.decode(cfg_bytes, type=list[BacktestRunConfig], dec_hook=msgspec_decoding_hook)

            BacktestNode = _get_BacktestNode()
            node = BacktestNode(configs=run_cfgs)
            _log(job_id, f"Building BacktestNode with {len(run_cfgs)} configs", "info")
            node.build()

            payload = []
            engines_map: dict[str, Any] = {}

            # Iterate configs and run one-shot, keep engines for reports
            for cfg in node.configs:
                run_id = getattr(cfg, "id", None) or str(uuid.uuid4())
                _log(job_id, f"Starting run {run_id}", "info")
                engine = node.get_engine(run_id)
                if engine is None:
                    # If engine not found by given id, try to pick the only engine
                    engines = node.get_engines()
                    engine = engines[0] if engines else None
                    if engine is None:
                        raise RuntimeError("Engine not built")

                # Run one-shot (loads data and runs), then collect result, then clear data but keep state
                node._run_oneshot(
                    run_config_id=run_id,
                    engine=engine,
                    data_configs=cfg.data,
                    start=cfg.start,
                    end=cfg.end,
                )

                result = engine.get_result()
                _log(job_id, f"Run {run_id} finished: orders={result.total_orders} positions={result.total_positions}", "info")
                engine.clear_data()

                payload.append({
                    "trader_id": result.trader_id,
                    "machine_id": result.machine_id,
                    "run_config_id": result.run_config_id,
                    "instance_id": result.instance_id,
                    "run_id": result.run_id,
                    "run_started": result.run_started,
                    "run_finished": result.run_finished,
                    "backtest_start": result.backtest_start,
                    "backtest_end": result.backtest_end,
                    "elapsed_time": result.elapsed_time,
                    "iterations": result.iterations,
                    "total_events": result.total_events,
                    "total_orders": result.total_orders,
                    "total_positions": result.total_positions,
                    "stats_pnls": result.stats_pnls,
                    "stats_returns": result.stats_returns,
                })

                engines_map[run_id] = engine

            _BACKTESTS[job_id] = {"node": node, "engines": engines_map}

            job.status = JobStatus.completed
            job.finished_at = time.time()
            job.result = {"results": payload}
            _log(job_id, "Backtest job completed", "info")
            _bump_jobs_ver()

            # Persist artifacts to disk for quick download later
            try:
                saved = _persist_backtest_artifacts(job_id)
                _log(job_id, f"Persisted {saved} artifact files", "debug")
            except Exception as perr:
                _log(job_id, f"Persist error: {perr}", "error")
        except Exception as e:
            job.status = JobStatus.failed
            job.finished_at = time.time()
            job.error = str(e)
            _log(job_id, f"Backtest job failed: {e}", "error")
            _bump_jobs_ver()

    threading.Thread(target=_worker, daemon=True).start()
    return {"job_id": job_id}




@app.get("/jobs/history")
def list_jobs_history(
    limit: int = 50,
    page: int | None = None,
    page_size: int | None = None,
    status: str | None = None,
    kind: str | None = None,
) -> dict[str, Any]:
    """
    Return recent jobs from in-memory store (best-effort history).

    Supports both legacy `limit` param and paginated `page` + `page_size`.
    Optionally filter by `status` and/or `kind`.
    """
    items = list(_JOBS.values())
    # Optional filters
    if status:
        try:
            s_val = JobStatus(status)
            items = [j for j in items if j.status == s_val]
        except Exception:
            items = [j for j in items if str(j.status) == status]
    if kind:
        try:
            k_val = JobKind(kind)
            items = [j for j in items if j.kind == k_val]
        except Exception:
            items = [j for j in items if str(j.kind) == kind]

    items.sort(key=lambda j: j.started_at, reverse=True)

    if page is None and page_size is None:
        # Legacy behavior using `limit`
        lim = max(1, min(int(limit), 500))
        out = [j.model_dump() for j in items[:lim]]
        return {"jobs": out, "total": len(items)}

    # Paginated behavior
    p = max(1, int(page or 1))
    ps = max(1, min(int(page_size or limit or 50), 500))
    start = (p - 1) * ps
    end = start + ps
    page_items = items[start:end]
    return {
        "jobs": [j.model_dump() for j in page_items],
        "total": len(items),
        "page": p,
        "page_size": ps,
        "has_next": end < len(items),
    }


@app.get("/jobs/stream")
async def jobs_stream(request: Request) -> Response:
    from fastapi.responses import StreamingResponse
    async def eventgen():
        last = -1
        while True:
            if await request.is_disconnected():
                break
            if last != _JOBS_VER:
                last = _JOBS_VER
                by_status: dict[str, int] = {}
                by_kind: dict[str, int] = {}
                for j in _JOBS.values():
                    by_status[str(j.status)] = by_status.get(str(j.status), 0) + 1
                    by_kind[str(j.kind)] = by_kind.get(str(j.kind), 0) + 1
                data = {
                    "ver": _JOBS_VER,
                    "ts": time.time(),
                    "count": len(_JOBS),
                    "stats": {"by_status": by_status, "by_kind": by_kind},
                    "jobs": [j.model_dump() for j in _JOBS.values()],
                }
                yield f"data: {json.dumps(data)}\n\n"
            await asyncio.sleep(1.0)
    return StreamingResponse(eventgen(), media_type="text/event-stream")


@app.get("/portfolio/stream")
async def portfolio_stream(request: Request) -> Response:
    from fastapi.responses import StreamingResponse
    async def eventgen():
        # Optional job_id filter
        job_id = request.query_params.get("job_id") if hasattr(request, "query_params") else None
        while True:
            if await request.is_disconnected():
                break
            try:
                data = portfolio_summary(job_id)
                yield f"data: {json.dumps(data)}\n\n"
            except Exception as e:
                yield "data: " + json.dumps({"status": "error", "error": str(e), "updated_at": time.time()}) + "\n\n"
            await asyncio.sleep(1.5)
    return StreamingResponse(eventgen(), media_type="text/event-stream")


def _compute_jobs_to_remove(items: list[Job], keep: int | None, cutoff_ts: float | None) -> list[str]:
    to_remove: list[str] = []
    for idx, j in enumerate(items):
        if j.id in _LIVE:
            continue
        if keep is not None and idx >= max(0, int(keep)):
            to_remove.append(j.id)
            continue
        if cutoff_ts is not None and j.started_at < float(cutoff_ts):
            to_remove.append(j.id)
    return to_remove


def _prune_job_logs(jid: str) -> None:
    try:
        d = (LOGS_ROOT / jid)
        if d.exists():
            for p in d.glob("*"):
                try:
                    p.unlink()
                except Exception as e:
                    logger.debug("jobs_prune: failed to unlink %s: %s", p, e)
            try:
                d.rmdir()
            except Exception as e:
                logger.debug("jobs_prune: failed to rmdir %s: %s", d, e)
    except Exception as e:
        logger.debug("jobs_prune: failed to prune logs for %s: %s", jid, e)


@app.post("/jobs/prune")
def jobs_prune(keep: int | None = 1000, older_than_sec: int | None = None, delete_logs: bool = False) -> dict[str, Any]:
    """
    Prune in-memory jobs: keep most recent `keep` and/or drop jobs older than `older_than_sec`.

    Live jobs are preserved. Backtest engine retention is also pruned accordingly.
    """
    global _JOBS
    if not _JOBS:
        return {"removed": 0, "remaining": 0}
    items = sorted(_JOBS.values(), key=lambda j: j.started_at, reverse=True)
    cutoff_ts = (time.time() - older_than_sec) if older_than_sec else None
    to_remove = _compute_jobs_to_remove(items, keep, cutoff_ts)
    for jid in to_remove:
        _JOBS.pop(jid, None)
        _BACKTESTS.pop(jid, None)
        if delete_logs:
            _prune_job_logs(jid)
    if to_remove:
        _bump_jobs_ver()
        _persist_jobs_state()
    return {"removed": len(to_remove), "remaining": len(_JOBS)}


@app.get("/jobs/counts")
def jobs_counts() -> dict[str, Any]:
    by_status: dict[str, int] = {}
    by_kind: dict[str, int] = {}
    for j in _JOBS.values():
        by_status[str(j.status)] = by_status.get(str(j.status), 0) + 1
        by_kind[str(j.kind)] = by_kind.get(str(j.kind), 0) + 1
    return {
        "ver": _JOBS_VER,
        "ts": time.time(),
        "count": len(_JOBS),
        "stats": {"by_status": by_status, "by_kind": by_kind},
    }


@app.get("/jobs/{job_id}")
def get_job(job_id: uuid.UUID) -> dict[str, Any]:
    job = _JOBS.get(str(job_id))
    if not job:
        raise HTTPException(status_code=404, detail="Job not found")
    return job.model_dump()


def _collect_reports(engine: Any) -> dict[str, Any]:
    trader = engine.trader
    return {
        "orders.csv": trader.generate_orders_report(),
        "order_fills.csv": trader.generate_order_fills_report(),
        "fills.csv": trader.generate_fills_report(),
        "positions.csv": trader.generate_positions_report(),
    }


def _collect_performance(engine: Any) -> Any | None:
    analyzer = engine._kernel.portfolio.analyzer
    returns_fn = getattr(analyzer, "returns", None)
    return returns_fn() if returns_fn else None


def _persist_for_run(job_id: str, run_id: str, engine: Any) -> int:
    written = 0
    run_dir = (ARTIFACTS_ROOT / job_id / run_id)
    run_dir.mkdir(parents=True, exist_ok=True)

    def write_text(path: Path, content: str) -> None:
        nonlocal written
        path.write_text(content, encoding="utf-8")
        written += 1

    for name, df in _collect_reports(engine).items():
        try:
            if df is not None and not df.empty:
                write_text(run_dir / name, df.to_csv(index=True))
        except Exception as e:
            print(f"[persist] job={job_id} run={run_id} write {name} failed: {e}")

    try:
        import pandas as pd
        returns_series = _collect_performance(engine)
        if returns_series is not None and not returns_series.empty:
            perf_df = pd.DataFrame({"ts": returns_series.index, "return": returns_series.to_numpy()})
            perf_df["cum_return"] = perf_df["return"].cumsum()
            write_text(run_dir / "performance.csv", perf_df.to_csv(index=False))
    except Exception as e:
        print(f"[persist] job={job_id} run={run_id} performance failed: {e}")

    try:
        import pandas as pd
        eq = get_backtest_equity(job_id, run_id)
        for ccy, points in eq.get("equity", {}).items():
            if points:
                eq_df = pd.DataFrame(points)
                write_text(run_dir / f"equity_{ccy}.csv", eq_df.to_csv(index=False))
    except Exception as e:
        print(f"[persist] job={job_id} run={run_id} equity failed: {e}")

    return written


def _persist_backtest_artifacts(job_id: str, run_config_id: str | None = None) -> int:
    entry = _BACKTESTS.get(job_id)
    if not entry:
        raise HTTPException(status_code=404, detail="Backtest job not found or no engines retained")

    engines: dict[str, Any] = entry.get("engines", {})
    if not engines:
        raise HTTPException(status_code=404, detail="No engines to persist for job")

    job_dir = ARTIFACTS_ROOT / job_id
    job_dir.mkdir(parents=True, exist_ok=True)

    items: list[tuple[str, Any]]
    if run_config_id and run_config_id in engines:
        items = [(run_config_id, engines[run_config_id])]
    else:
        items = list(engines.items())

    total = 0
    for run_id, engine in items:
        total += _persist_for_run(job_id, run_id, engine)
    return total


@app.get("/backtests/{job_id}/reports")
def get_backtest_report(job_id: str, run_config_id: str | None = None, report: str = "orders") -> dict[str, Any]:
    entry = _BACKTESTS.get(job_id)
    if not entry:
        raise HTTPException(status_code=404, detail="Backtest job not found or no engines retained")

    engines = entry.get("engines", {})
    engine = None
    if run_config_id and run_config_id in engines:
        engine = engines[run_config_id]
    elif engines:
        # Fallback to first engine
        engine = next(iter(engines.values()))

    if engine is None:
        raise HTTPException(status_code=404, detail="Engine not found for job")

    trader = engine.trader
    import pandas as pd  # Local import to avoid global pandas dependency here

    if report == "orders":
        df = trader.generate_orders_report()
    elif report == "order_fills":
        df = trader.generate_order_fills_report()
    elif report == "fills":
        df = trader.generate_fills_report()
    elif report == "positions":
        df = trader.generate_positions_report()
    else:
        raise HTTPException(status_code=400, detail="Unknown report type")

    if df is None or (isinstance(df, pd.DataFrame) and df.empty):
        return {"report": report, "records": []}

    data = df.reset_index().to_dict(orient="records")
    return {"report": report, "records": data}


@app.get("/backtests/{job_id}/performance")
def get_backtest_performance(job_id: str, run_config_id: str | None = None) -> dict[str, Any]:
    entry = _BACKTESTS.get(job_id)
    if not entry:
        raise HTTPException(status_code=404, detail="Backtest job not found or no engines retained")

    engines = entry.get("engines", {})
    engine = None
    if run_config_id and run_config_id in engines:
        engine = engines[run_config_id]
    elif engines:
        engine = next(iter(engines.values()))

    if engine is None:
        raise HTTPException(status_code=404, detail="Engine not found for job")

    # Portfolio analyzer time series + stats
    analyzer = engine._kernel.portfolio.analyzer
    returns_series = getattr(analyzer, "returns")() if hasattr(analyzer, "returns") else None

    returns: list[dict[str, Any]] = []
    if returns_series is not None and not returns_series.empty:
        # returns_series index is datetime, values are floats
        for ts, val in returns_series.items():
            try:
                iso = ts.isoformat()
            except Exception:
                iso = str(ts)
            returns.append({"ts": iso, "value": float(val)})

    stats_pnls: dict[str, dict[str, float]] = {}
    # analyzer.currencies -> list[Currency]
    try:
        for ccy in analyzer.currencies:
            stats_pnls[getattr(ccy, "code", str(ccy))] = analyzer.get_performance_stats_pnls(ccy)
    except Exception:
        stats_pnls = {}

    stats_returns = analyzer.get_performance_stats_returns()

    return {
        "returns": returns,
        "stats_pnls": stats_pnls,
        "stats_returns": stats_returns,
    }


@app.get("/backtests/{job_id}/equity")
def get_backtest_equity(job_id: str, run_config_id: str | None = None) -> dict[str, Any]:
    entry = _BACKTESTS.get(job_id)
    if not entry:
        raise HTTPException(status_code=404, detail="Backtest job not found or no engines retained")

    engines = entry.get("engines", {})
    engine = None
    if run_config_id and run_config_id in engines:
        engine = engines[run_config_id]
    elif engines:
        engine = next(iter(engines.values()))

    if engine is None:
        raise HTTPException(status_code=404, detail="Engine not found for job")

    trader = engine.trader

    # Aggregate account totals per currency across all venues, over time
    series: dict[str, list[dict[str, Any]]] = {}
    try:
        for venue in engine._venues.keys():
            df = trader.generate_account_report(venue)
            if df is None or df.empty:
                continue
            # df has columns incl. 'total' and 'currency' per event timestamp (index)
            df2 = df.reset_index()[["ts_event", "total", "currency"]]
            # Group by timestamp and currency to handle multiple accounts per currency
            grouped = (
                df2.groupby(["ts_event", "currency"], as_index=False)["total"].sum().sort_values("ts_event")
            )
            for _, row in grouped.iterrows():
                ts_iso = row["ts_event"].isoformat() if hasattr(row["ts_event"], "isoformat") else str(row["ts_event"])
                ccy = str(row["currency"])
                series.setdefault(ccy, []).append({"ts": ts_iso, "value": float(row["total"])})
    except Exception as e:
        # Fallback: return empty if aggregation fails
        print(f"[equity] aggregation error: {e}")
        series = {}

    return {"equity": series}


@app.get("/backtests/{job_id}/reports.zip")
def download_backtest_reports_zip(job_id: str, run_config_id: str | None = None) -> Response:
    entry = _BACKTESTS.get(job_id)
    if not entry:
        raise HTTPException(status_code=404, detail="Backtest job not found or no engines retained")

    engines = entry.get("engines", {})
    engine = None
    if run_config_id and run_config_id in engines:
        engine = engines[run_config_id]
    elif engines:
        engine = next(iter(engines.values()))

    if engine is None:
        raise HTTPException(status_code=404, detail="Engine not found for job")

    trader = engine.trader
    import io
    import zipfile

    import pandas as pd

    mem = io.BytesIO()
    with zipfile.ZipFile(mem, mode="w", compression=zipfile.ZIP_DEFLATED) as zf:
        # Tabular reports
        reports = {
            "orders.csv": trader.generate_orders_report(),
            "order_fills.csv": trader.generate_order_fills_report(),
            "fills.csv": trader.generate_fills_report(),
            "positions.csv": trader.generate_positions_report(),
        }
        for name, df in reports.items():
            try:
                if df is not None and not df.empty:
                    zf.writestr(name, df.to_csv(index=True))
            except Exception as write_err:
                print(f"[zip] failed to write {name}: {write_err}")

        # Performance
        analyzer = engine._kernel.portfolio.analyzer
        returns_series = getattr(analyzer, "returns")() if hasattr(analyzer, "returns") else None
        if returns_series is not None and not returns_series.empty:
            perf_df = pd.DataFrame({
                "ts": returns_series.index,
                "return": returns_series.to_numpy(),
            })
            perf_df["cum_return"] = perf_df["return"].cumsum()
            zf.writestr("performance.csv", perf_df.to_csv(index=False))

        # Equity per currency
        eq = get_backtest_equity(job_id, run_config_id)
        for ccy, points in eq.get("equity", {}).items():
            eq_df = pd.DataFrame(points)
            zf.writestr(f"equity_{ccy}.csv", eq_df.to_csv(index=False))

    mem.seek(0)
    return Response(
        content=mem.read(),
        media_type="application/zip",
        headers={"Content-Disposition": f"attachment; filename=backtest_reports_{job_id}.zip"},
    )


@app.get("/jobs/{job_id}/logs")
def get_job_logs(job_id: str, limit: int = 1000, level: str | None = None, q: str | None = None, since_ts: float | None = None) -> dict[str, Any]:
    logs = list(_LOGS.get(job_id, []))
    # Apply server-side filters (best-effort)
    if since_ts is not None:
        try:
            t0 = float(since_ts)
            logs = [entry for entry in logs if float(entry.get("ts", 0)) >= t0]
        except Exception as exc:
            logger.debug("logs.filter: invalid since_ts=%s error=%s", since_ts, exc)
    if level:
        lv = level.lower()
        logs = [e for e in logs if str(e.get("level", "")).lower() == lv]
    if q:
        qq = q.lower()
        logs = [e for e in logs if qq in str(e.get("message", "")).lower()]
    if limit and limit > 0:
        logs = logs[-int(limit):]
    return {"job_id": job_id, "count": len(_LOGS.get(job_id, [])), "logs": logs}


@app.get("/jobs/{job_id}/logs.txt")
def get_job_logs_text(job_id: str, limit: int = 2000, level: str | None = None, q: str | None = None, since_ts: float | None = None) -> Response:
    logs = list(_LOGS.get(job_id, []))
    if since_ts is not None:
        try:
            t0 = float(since_ts)
            logs = [entry for entry in logs if float(entry.get("ts", 0)) >= t0]
        except Exception as exc:
            logger.debug("logs.filter: invalid since_ts=%s error=%s", since_ts, exc)
    if level:
        lv = level.lower()
        logs = [e for e in logs if str(e.get("level", "")).lower() == lv]
    if q:
        qq = q.lower()
        logs = [e for e in logs if qq in str(e.get("message", "")).lower()]
    if limit and limit > 0:
        logs = logs[-int(limit):]
    lines: list[str] = []
    for entry in logs:
        ts = entry.get("ts", time.time())
        level = str(entry.get("level", "info")).upper()
        msg = str(entry.get("message", ""))
        tstr = time.strftime("%Y-%m-%d %H:%M:%S", time.localtime(ts))
        lines.append(f"{tstr} [{level}] {msg}")
    text = "\n".join(lines)
    return Response(content=text, media_type="text/plain")

@app.post("/jobs/{job_id}/logs/clear")
def clear_job_logs(job_id: str) -> dict[str, Any]:
    cnt = len(_LOGS.get(job_id, []))
    _LOGS[job_id] = []
    return {"job_id": job_id, "cleared": cnt}


@app.get("/jobs/{job_id}/logs/stream")
async def job_logs_stream(job_id: str, request: Request, level: str | None = None, q: str | None = None) -> Response:
    from fastapi.responses import StreamingResponse
    async def eventgen():
        last_len = -1
        while True:
            if await request.is_disconnected():
                break
            buf = _LOGS.get(job_id, [])
            if len(buf) != last_len:
                start = max(0, last_len)
                lines = buf[start:]
                if level:
                    lv = level.lower()
                    lines = [e for e in lines if str(e.get("level", "")).lower() == lv]
                if q:
                    qq = q.lower()
                    lines = [e for e in lines if qq in str(e.get("message", "")).lower()]
                data = {"job_id": job_id, "index": len(buf), "lines": lines, "ts": time.time()}
                yield f"data: {json.dumps(data)}\n\n"
                last_len = len(buf)
            await asyncio.sleep(1.0)
    return StreamingResponse(eventgen(), media_type="text/event-stream")


@app.get("/jobs/{job_id}/logs.ndjson")
def download_job_logs_ndjson(job_id: str) -> FileResponse:
    ndjson_path, _ = _log_paths(job_id)
    if not ndjson_path.exists():
        # fall back to build from memory buffer into temp file
        ndjson_path.parent.mkdir(parents=True, exist_ok=True)
        try:
            with ndjson_path.open("w", encoding="utf-8") as f:
                for e in _LOGS.get(job_id, []):
                    f.write(json.dumps(e, ensure_ascii=False) + "\n")
        except Exception:
            raise HTTPException(status_code=404, detail="No logs for job")
    return FileResponse(path=str(ndjson_path), filename=f"logs_{job_id}.ndjson")


@app.get("/jobs/{job_id}/logs/download")
def download_job_logs_zip(job_id: str) -> Response:
    import io
    import zipfile
    ndjson_path, txt_path = _log_paths(job_id)
    mem = io.BytesIO()
    with zipfile.ZipFile(mem, mode="w", compression=zipfile.ZIP_DEFLATED) as zf:
        if txt_path.exists():
            zf.write(txt_path, arcname="logs.txt")
        if ndjson_path.exists():
            zf.write(ndjson_path, arcname="logs.ndjson")
        # include rotated parts if any
        for p in ndjson_path.parent.glob("logs.ndjson.*"):
            zf.write(p, arcname=p.name)
        for p in txt_path.parent.glob("logs.txt.*"):
            zf.write(p, arcname=p.name)
    mem.seek(0)
    return Response(content=mem.read(), media_type="application/zip", headers={"Content-Disposition": f"attachment; filename=logs_{job_id}.zip"})


@app.get("/live/{job_id}/snapshot.zip")
def live_snapshot_zip(
    job_id: str,
    include_status: bool = True,
    include_logs: bool = True,
    include_orders: bool = True,
    include_fills: bool = True,
    include_positions: bool = True,
) -> Response:
    """Download a server-side snapshot ZIP for a live job (status JSON, logs, and current CSV reports)."""
    entry = _LIVE.get(job_id)
    if not entry:
        raise HTTPException(status_code=404, detail="Live job not found")
    node = cast(Any, entry.get("node"))
    if node is None:
        raise HTTPException(status_code=503, detail="Live node unavailable")
    trader = node.trader

    import io
    import zipfile
    import pandas as pd

    mem = io.BytesIO()
    with zipfile.ZipFile(mem, mode="w", compression=zipfile.ZIP_DEFLATED) as zf:
        if include_status:
            try:
                st = portfolio_summary(job_id)
                zf.writestr("status.json", json.dumps(st, indent=2))
            except Exception as e:
                zf.writestr("status_error.txt", str(e))
        if include_logs:
            ndjson_path, txt_path = _log_paths(job_id)
            if txt_path.exists():
                zf.write(txt_path, arcname="logs/logs.txt")
            if ndjson_path.exists():
                zf.write(ndjson_path, arcname="logs/logs.ndjson")
        # Tabular reports (best-effort)
        try:
            if include_orders:
                df = trader.generate_orders_report()
                if df is not None and not df.empty:
                    zf.writestr("reports/orders.csv", df.to_csv(index=True))
            if include_fills:
                df2 = trader.generate_fills_report()
                if df2 is not None and not df2.empty:
                    zf.writestr("reports/fills.csv", df2.to_csv(index=True))
            if include_positions:
                df3 = trader.generate_positions_report()
                if df3 is not None and not df3.empty:
                    zf.writestr("reports/positions.csv", df3.to_csv(index=True))
        except Exception as e:
            zf.writestr("reports_error.txt", str(e))

    mem.seek(0)
    return Response(
        content=mem.read(),
        media_type="application/zip",
        headers={"Content-Disposition": f"attachment; filename=live_snapshot_{job_id}.zip"},
    )


@app.get("/jobs/{job_id}/artifacts")
def list_artifacts(
    job_id: str,
    page: int = 1,
    page_size: int = 100,
    prefix: str | None = None,
    sort: str = "path",  # path|size|mtime
    order: str = "asc",  # asc|desc
) -> dict[str, Any]:
    job_dir = ARTIFACTS_ROOT / job_id
    if not job_dir.exists():
        return {"files": [], "total": 0, "page": page, "page_size": page_size, "has_next": False}
    files: list[dict[str, Any]] = []
    for root, _dirs, filenames in os.walk(job_dir):
        for fn in filenames:
            p = Path(root) / fn
            rel = p.relative_to(job_dir)
            sp = rel.as_posix()
            if prefix and prefix not in sp:
                continue
            stat = p.stat()
            files.append({
                "path": sp,
                "size": stat.st_size,
                "mtime": stat.st_mtime,
            })
    key = (sort if sort in {"path","size","mtime"} else "path")
    files.sort(key=lambda x: x[key] if key != "path" else cast(str, x["path"]))
    if order == "desc":
        files.reverse()
    total = len(files)
    page_i = max(1, int(page))
    page_size_i = max(1, min(int(page_size), 1000))
    start = (page_i - 1) * page_size_i
    end = start + page_size_i
    page_items = files[start:end]
    return {"files": page_items, "total": total, "page": page_i, "page_size": page_size_i, "has_next": end < total}


@app.get("/jobs/{job_id}/artifacts/download")
def download_artifact(job_id: str, path: str) -> FileResponse:
    job_dir = (ARTIFACTS_ROOT / job_id).resolve()
    target = (job_dir / Path(path)).resolve()
    if not str(target).startswith(str(job_dir)) or not target.exists() or not target.is_file():
        raise HTTPException(status_code=404, detail="Artifact not found")
    return FileResponse(path=str(target), filename=target.name)


@app.get("/jobs/{job_id}/artifacts.zip")
def download_artifacts_zip(job_id: str, run_config_id: str | None = None, prefix: str | None = None) -> Response:
    job_dir = (ARTIFACTS_ROOT / job_id).resolve()
    if not job_dir.exists():
        raise HTTPException(status_code=404, detail="No artifacts for job")

    import io
    import zipfile

    mem = io.BytesIO()
    with zipfile.ZipFile(mem, mode="w", compression=zipfile.ZIP_DEFLATED) as zf:
        def add_dir(src: Path) -> None:
            for root, _dirs, files in os.walk(src):
                for fn in files:
                    p = Path(root) / fn
                    rel = p.relative_to(job_dir)
                    sp = rel.as_posix()
                    if prefix and prefix not in sp:
                        continue
                    zf.write(p, arcname=sp)
        if run_config_id:
            src = (job_dir / run_config_id).resolve()
            if not str(src).startswith(str(job_dir)) or not src.exists() or not src.is_dir():
                raise HTTPException(status_code=404, detail="Artifacts for run not found")
            add_dir(src)
        else:
            add_dir(job_dir)

    mem.seek(0)
    return Response(
        content=mem.read(),
        media_type="application/zip",
        headers={"Content-Disposition": f"attachment; filename=artifacts_{job_id}.zip"},
    )


def _write_reports_to_zip(zf: Any, engine: Any, job_id: str, run_config_id: str | None) -> None:
    trader = engine.trader
    reports = {
        "reports/orders.csv": trader.generate_orders_report(),
        "reports/order_fills.csv": trader.generate_order_fills_report(),
        "reports/fills.csv": trader.generate_fills_report(),
        "reports/positions.csv": trader.generate_positions_report(),
    }
    for name, df in reports.items():
        try:
            if df is not None and not df.empty:
                zf.writestr(name, df.to_csv(index=True))
        except Exception as write_err:
            print(f"[analysis.zip] failed to write {name}: {write_err}")

    try:
        import pandas as pd
        analyzer = engine._kernel.portfolio.analyzer
        returns_series = getattr(analyzer, "returns")() if hasattr(analyzer, "returns") else None
        if returns_series is not None and not returns_series.empty:
            perf_df = pd.DataFrame({"ts": returns_series.index, "return": returns_series.to_numpy()})
            perf_df["cum_return"] = perf_df["return"].cumsum()
            zf.writestr("reports/performance.csv", perf_df.to_csv(index=False))
    except Exception as e:
        print(f"[analysis.zip] performance error: {e}")

    try:
        import pandas as pd
        eq = get_backtest_equity(job_id, run_config_id)
        for ccy, points in eq.get("equity", {}).items():
            if points:
                eq_df = pd.DataFrame(points)
                zf.writestr(f"reports/equity_{ccy}.csv", eq_df.to_csv(index=False))
    except Exception as e:
        print(f"[analysis.zip] equity error: {e}")


def _write_artifacts_to_zip(zf: Any, job_id: str, include_artifacts: bool) -> None:
    import json
    job_dir = (ARTIFACTS_ROOT / job_id).resolve()
    manifest = []
    if job_dir.exists():
        for root, _dirs, files in os.walk(job_dir):
            for fn in files:
                p = Path(root) / fn
                rel = p.relative_to(job_dir)
                info = {"path": rel.as_posix(), "size": p.stat().st_size, "mtime": p.stat().st_mtime}
                manifest.append(info)
                if include_artifacts:
                    zf.write(p, arcname=f"artifacts/{rel.as_posix()}")
    zf.writestr("artifacts_manifest.json", json.dumps({"files": manifest}, indent=2))


@app.get("/backtests/{job_id}/analysis.zip")
def download_analysis_zip(job_id: str, run_config_id: str | None = None, include_artifacts: bool = False) -> Response:
    import io
    import json
    import zipfile

    entry = _BACKTESTS.get(job_id)
    if not entry:
        raise HTTPException(status_code=404, detail="Backtest job not found or no engines retained")

    engines = entry.get("engines", {})
    engine = None
    if run_config_id and run_config_id in engines:
        engine = engines[run_config_id]
    elif engines:
        engine = next(iter(engines.values()))
    if engine is None:
        raise HTTPException(status_code=404, detail="Engine not found for job")

    mem = io.BytesIO()
    with zipfile.ZipFile(mem, mode="w", compression=zipfile.ZIP_DEFLATED) as zf:
        _write_reports_to_zip(zf, engine, job_id, run_config_id)
        _write_artifacts_to_zip(zf, job_id, include_artifacts)
        job_obj = _JOBS.get(job_id)
        if job_obj:
            zf.writestr("metadata.json", json.dumps(job_obj.model_dump(), indent=2))

    mem.seek(0)
    return Response(content=mem.read(),
        media_type="application/zip",
        headers={"Content-Disposition": f"attachment; filename=analysis_{job_id}.zip"},
    )


@app.get("/portfolio/summary")

def portfolio_summary(job_id: str | None = None) -> dict[str, Any]:
    """
    Return a snapshot of live portfolio (positions and accounts) for a running live job.

    If job_id is provided, use that live node; otherwise default to the first running job.
    If no live job is running, returns an empty snapshot with status information.
    """
    if not _LIVE:
        return {"status": "no_live", "positions": [], "accounts": [], "job_id": None, "updated_at": time.time()}

    # Pick requested or first live entry
    node_entry: dict[str, Any] | None = None
    use_jid: str | None = None
    if job_id and job_id in _LIVE:
        node_entry = _LIVE.get(job_id)
        use_jid = job_id
    else:
        use_jid, node_entry = next(iter(_LIVE.items()))
    node = cast(Any, (node_entry or {}).get("node"))
    if node is None:
        return {"status": "no_live", "positions": [], "accounts": [], "job_id": None, "updated_at": time.time()}

    positions: list[dict[str, Any]] = []
    accounts: list[dict[str, Any]] = []

    try:
        # Positions report
        pos_df = node.trader.generate_positions_report()
        if pos_df is not None and not pos_df.empty:
            positions = cast(list[dict[str, Any]], pos_df.reset_index().to_dict(orient="records"))
    except Exception as e:
        print(f"[portfolio] positions error: {e}")
        positions = []

    try:
        # Accounts per venue aggregated by currency totals
        for venue in getattr(node.kernel, "_venues", {}).keys():
            df = node.trader.generate_account_report(venue)
            if df is None or df.empty:
                continue
            df2 = df.reset_index()
            # expected columns: currency, total, available (best-effort)
            for _, row in df2.iterrows():
                rec = {k: (row[k] if k in row else None) for k in ["currency", "total", "available", "buying_power", "equity"]}
                rec["venue"] = str(venue)
                # Coerce datetimes if present (best-effort)
                coerced: dict[str, Any] = {}
                for k, v in rec.items():
                    val: Any = v
                    coerced[k] = val.isoformat() if hasattr(val, "isoformat") else val
                accounts.append(coerced)
    except Exception as e:
        print(f"[portfolio] accounts error: {e}")
        accounts = []

    return {
        "status": "ok",
        "job_id": use_jid,
        "trader_id": str(node.trader.trader_id) if hasattr(node.trader, "trader_id") else None,
        "positions": positions,
        "accounts": accounts,
        "desk_strategy_id": (str((node_entry or {}).get("desk").id) if (node_entry or {}).get("desk") is not None else None),
        "updated_at": time.time(),
    }


@app.get("/portfolio/reports")
def portfolio_reports(report: str = "positions", venue: str | None = None, open_only: bool = False, strategy_id: str | None = None) -> dict[str, Any]:
    """Return current live report as JSON records."""
    if not _LIVE:
        return {"report": report, "records": []}
    job_id, entry = next(iter(_LIVE.items()))
    node = cast(Any, entry.get("node"))
    if node is None:
        return {"report": report, "records": []}
    trader = node.trader
    import pandas as pd
    if report == "orders":
        df = trader.generate_orders_report()
    elif report in {"order_fills", "fills"}:
        # prefer specific call if available
        if report == "order_fills":
            df = trader.generate_order_fills_report()
        else:
            df = trader.generate_fills_report()
    else:
        # default positions
        df = trader.generate_positions_report()
    if df is None or (isinstance(df, pd.DataFrame) and df.empty):
        return {"report": report, "records": []}
    data = df.reset_index().to_dict(orient="records")

    # Optional server-side filters
    if venue:
        v = venue.upper()
        def _venue_from_instr(iid: str) -> str:
            s = str(iid or "")
            dot = s.rfind(".")
            return s[dot + 1 :].upper() if dot > 0 else ""
        data = [r for r in data if _venue_from_instr(r.get("instrument_id")) == v]
    if open_only:
        def _is_open(r: dict) -> bool:
            v = str(r.get("status") or r.get("state") or r.get("order_status") or "").upper()
            if any(k in v for k in ("FILLED","CANCEL","REJECT","CLOSE")):
                return False
            return ("OPEN" in v) or ("WORK" in v) or ("NEW" in v) or (v == "")
        data = [r for r in data if _is_open(r)]
    if strategy_id:
        sid = str(strategy_id)
        data = [r for r in data if str(r.get("strategy_id") or "") == sid]

    # Tag desk orders/fills with strategy_id if missing and COID belongs to desk
    try:
        desk = entry.get("desk")
        if desk is not None and hasattr(desk, "is_desk_client_order_id"):
            dsid = str(desk.id)
            for rec in data:
                if rec.get("strategy_id") in (None, ""):
                    coid = rec.get("client_order_id")
                    if coid and desk.is_desk_client_order_id(str(coid)):
                        rec["strategy_id"] = dsid
    except Exception:
        pass

    # Coerce datetimes
    for rec in data:
        for k, v in list(rec.items()):
            if hasattr(v, "isoformat"):
                rec[k] = v.isoformat()
    return {"report": report, "records": data}


@app.get("/portfolio/reports/{report_type}.csv")
def download_portfolio_report_csv(
    report_type: str,
    venue: str | None = None,
    open_only: bool = False,
    strategy_id: str | None = None,
) -> Response:
    if not _LIVE:
        return Response(content="", media_type="text/csv", headers={"Content-Disposition": f"attachment; filename={report_type}_live.csv"})
    _job_id, entry = next(iter(_LIVE.items()))
    node = cast(Any, entry.get("node"))
    if node is None:
        return Response(content="", media_type="text/csv", headers={"Content-Disposition": f"attachment; filename={report_type}_live.csv"})
    trader = node.trader
    import pandas as pd
    if report_type == "orders":
        df = trader.generate_orders_report()
    elif report_type == "order_fills":
        df = trader.generate_order_fills_report()
    elif report_type == "fills":
        df = trader.generate_fills_report()
    elif report_type == "positions":
        df = trader.generate_positions_report()
    else:
        raise HTTPException(status_code=400, detail="Unknown report type")
    if df is None or (isinstance(df, pd.DataFrame) and df.empty):
        return Response(content="", media_type="text/csv", headers={"Content-Disposition": f"attachment; filename={report_type}_live.csv"})

    # Add strategy_id for desk-originated rows if possible
    try:
        desk = entry.get("desk")
        if desk is not None and hasattr(desk, "is_desk_client_order_id") and hasattr(df, "copy"):
            df = df.copy()
            if "strategy_id" not in df.columns:
                df["strategy_id"] = None
            if "client_order_id" in df.columns:
                mask = df["client_order_id"].astype(str).map(lambda x: bool(desk.is_desk_client_order_id(str(x))))
                df.loc[mask, "strategy_id"] = str(desk.id)
    except Exception:
        pass

    # Server-side filters
    try:
        if venue and "instrument_id" in df.columns:
            v = venue.upper()
            def _venue_from_instr(iid: str) -> str:
                s = str(iid or "")
                dot = s.rfind(".")
                return s[dot + 1 :].upper() if dot > 0 else ""
            df = df[df["instrument_id"].astype(str).map(_venue_from_instr) == v]
        if open_only and report_type == "orders":
            # best-effort; normalize a status-like column
            col = "status" if "status" in df.columns else ("order_status" if "order_status" in df.columns else ("state" if "state" in df.columns else None))
            if col:
                def _is_open(v: str) -> bool:
                    u = str(v or "").upper()
                    if any(k in u for k in ("FILLED","CANCEL","REJECT","CLOSE")):
                        return False
                    return ("OPEN" in u) or ("WORK" in u) or ("NEW" in u) or (u == "")
                df = df[df[col].map(_is_open)]
        if strategy_id and "strategy_id" in df.columns:
            df = df[df["strategy_id"].astype(str) == str(strategy_id)]
    except Exception:
        pass

    csv_data = df.to_csv(index=True)
    return Response(content=csv_data, media_type="text/csv", headers={"Content-Disposition": f"attachment; filename={report_type}_live.csv"})


# ----------------------
# Agents endpoints
# ----------------------

@app.get("/agents/providers")
def agents_providers() -> dict[str, Any]:
    if TEAM is None:
        return {"providers": {"agent_framework": False}}
    return {"providers": TEAM.providers()}


@app.get("/agents/status")
def agents_status() -> dict[str, Any]:
    if TEAM is None:
        raise HTTPException(status_code=503, detail="Agent framework unavailable")
    return TEAM.status()


class ActivatePayload(BaseModel):
    strategy: str
    mode: str  # paper|live
    competency_target: dict[str, float] | None = None


@app.post("/agents/activate")
def agents_activate(payload: ActivatePayload) -> dict[str, Any]:
    if TEAM is None:
        raise HTTPException(status_code=503, detail="Agent framework unavailable")
    try:
        return TEAM.activate(payload.strategy, payload.mode, payload.competency_target)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))


@app.post("/agents/deactivate")
def agents_deactivate() -> dict[str, Any]:
    if TEAM is None:
        raise HTTPException(status_code=503, detail="Agent framework unavailable")
    return TEAM.deactivate()


@app.post("/agents/models")
def agents_assign_models(mapping: dict[str, dict[str, str]]) -> dict[str, Any]:
    if TEAM is None:
        raise HTTPException(status_code=503, detail="Agent framework unavailable")
    return TEAM.assign_models(mapping)


@app.post("/agents/run_cycle")
def agents_run_cycle() -> dict[str, Any]:
    if TEAM is None:
        raise HTTPException(status_code=503, detail="Agent framework unavailable")
    return TEAM.run_cycle()


class AgentsBacktestPayload(BaseModel):
    dataset: str
    instrument_id: str
    bar_spec: str | None = None
    start_time: str | None = None
    end_time: str | None = None
    strategy_name: str | None = None


@app.post("/agents/backtest")
def agents_backtest(payload: AgentsBacktestPayload) -> dict[str, Any]:
    # Build a run_cfg using built-in Blank strategy unless specified
    name = payload.strategy_name or "AgentsBacktest"
    instrument_id = payload.instrument_id
    dataset = payload.dataset
    bar_spec = payload.bar_spec
    start_time = payload.start_time
    end_time = payload.end_time

    # dataset catalog path
    catalog_path = str((DATASETS_ROOT / dataset).resolve())
    if not (DATASETS_ROOT / dataset).exists():
        raise HTTPException(status_code=404, detail="dataset not found")

    run_cfg = {
        "id": name,
        "engine": {
            "strategies": [{
                "strategy_path": "nautilus_trader.examples.strategies.blank:MyStrategy",
                "config_path": "nautilus_trader.examples.strategies.blank:MyStrategyConfig",
                "config": {"instrument_id": instrument_id},
            }]
        },
        "venues": [{"name": "SIM", "oms_type": "NETTING", "account_type": "CASH", "starting_balances": ["10000 USD"], "book_type": "L1_MBP"}],
        "data": [{
            "catalog_path": catalog_path,
            "data_cls": "nautilus_trader.model.data.Bar",
            "instrument_id": instrument_id,
            "bar_spec": bar_spec,
            "start_time": start_time,
            "end_time": end_time,
        }],
        "dispose_on_completion": False,
        "raise_exception": True,
    }

    try:
        r = run_backtests([run_cfg])
        return r
    except HTTPException:
        raise
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


class SchedulePayload(BaseModel):
    interval_sec: int | None = None
    enabled: bool = False


@app.post("/agents/schedule")
def agents_schedule(payload: SchedulePayload) -> dict[str, Any]:
    if TEAM is None:
        raise HTTPException(status_code=503, detail="Agent framework unavailable")
    return TEAM.set_schedule(payload.interval_sec, payload.enabled)


class CompliancePayload(BaseModel):
    recommendation_id: str
    approved: bool
    reason: str | None = None
    approver_role: str | None = None
    # Optional constraints
    instrument_id: str | None = None
    side: str | None = None  # buy|sell
    max_qty: float | None = None
    tif: str | None = None  # GTC|IOC|FOK
    expires_at: float | None = None  # unix seconds
    expires_in_sec: int | None = None


@app.post("/agents/compliance/record")
def agents_compliance_record(payload: CompliancePayload) -> dict[str, Any]:
    if TEAM is None:
        raise HTTPException(status_code=503, detail="Agent framework unavailable")
    # Pass through optional constraint fields
    return TEAM.record_compliance(
        payload.recommendation_id,
        payload.approved,
        payload.reason,
        approver_role=payload.approver_role,
        instrument_id=payload.instrument_id,
        side=(payload.side.lower() if payload.side else None),
        max_qty=payload.max_qty,
        tif=(payload.tif.upper() if payload.tif else None),
        expires_at=payload.expires_at,
        expires_in_sec=payload.expires_in_sec,
    )


@app.get("/agents/competency")
def agents_competency(horizon_jobs: int = 10) -> dict[str, Any]:
    if TEAM is None:
        raise HTTPException(status_code=503, detail="Agent framework unavailable")
    return TEAM.compute_competency(horizon_jobs)


@app.get("/agents/compliance/log")
def agents_compliance_log(limit: int = 50) -> dict[str, Any]:
    if TEAM is None:
        raise HTTPException(status_code=503, detail="Agent framework unavailable")
    return TEAM.get_compliance_log(limit)


@app.post("/agents/manager/evaluate")
def agents_manager_evaluate(payload: dict[str, Any] | None = None) -> dict[str, Any]:
    if TEAM is None:
        raise HTTPException(status_code=503, detail="Agent framework unavailable")
    roles = None
    if payload and isinstance(payload.get("roles"), list):
        roles = [str(r) for r in payload.get("roles")]
    return TEAM.evaluate_models(roles)


@app.get("/agents/models/stats")
def agents_models_stats() -> dict[str, Any]:
    if TEAM is None:
        raise HTTPException(status_code=503, detail="Agent framework unavailable")
    return TEAM.model_stats()


@app.get("/defi/quote")
def defi_quote(
    sellToken: str,
    buyToken: str,
    sellAmount: str | None = None,
    buyAmount: str | None = None,
    chainId: int = 1,
    takerAddress: str | None = None,
) -> dict[str, Any]:
    """
    Proxy to 0x Swap API for quotes to avoid CORS in the browser.

    Docs: https://0x.org/docs/introduction/0x-swap-api
    """
    import httpx

    base = {
        1: "https://api.0x.org/swap/v1/quote",
        137: "https://polygon.api.0x.org/swap/v1/quote",
        42161: "https://arbitrum.api.0x.org/swap/v1/quote",
    }.get(chainId, "https://api.0x.org/swap/v1/quote")

    params: dict[str, Any] = {
        "sellToken": sellToken,
        "buyToken": buyToken,
    }
    if sellAmount:
        params["sellAmount"] = sellAmount
    if buyAmount:
        params["buyAmount"] = buyAmount
    if takerAddress:
        params["takerAddress"] = takerAddress

    with httpx.Client(timeout=15) as client:
        r = client.get(base, params=params)
        r.raise_for_status()
        return r.json()


@app.get("/studio/providers")
def studio_providers() -> dict[str, Any]:
    import httpx
    providers: dict[str, Any] = {
        "openai": {"available": bool(OPENAI_API_KEY), "default_model": "gpt-4o-mini"},
        "anthropic": {"available": bool(ANTHROPIC_API_KEY), "default_model": "claude-3-5-sonnet-latest"},
        "gemini": {"available": bool(GEMINI_API_KEY), "default_model": "gemini-1.5-pro"},
        "ollama": {"available": False, "default_model": "llama3.1"},
    }
    try:
        with httpx.Client(timeout=2) as client:
            r = client.get(f"{OLLAMA_API_URL}/api/tags")
            providers["ollama"]["available"] = r.status_code == 200
    except Exception:
        providers["ollama"]["available"] = False
    return {"providers": providers}


@app.post("/studio/generate")
def studio_generate(payload: dict[str, Any]) -> dict[str, Any]:
    """
    Generate a Python strategy skeleton from a natural language prompt.

    If OPENAI_API_KEY is set and provider=openai, calls OpenAI; if provider=ollama, calls local Ollama.
    Otherwise, returns a templated strategy based on the prompt.
    """
    prompt: str = str(payload.get("prompt", "")).strip()
    name: str = payload.get("name") or "GeneratedStrategy"
    provider: str = (payload.get("provider") or ("openai" if OPENAI_API_KEY else "ollama")).lower()

    base_template = f"""
from nautilus_trader.trading.strategy import Strategy
from nautilus_trader.trading.strategy import StrategyConfig
from nautilus_trader.model.identifiers import InstrumentId

class {name}Config(StrategyConfig):
    instrument_id: str

class {name}(Strategy):
    def __init__(self, config: {name}Config) -> None:
        super().__init__(config)
        self.instrument_id = InstrumentId(config.instrument_id)

    def on_start(self):
        # TODO: subscribe to data and initialize indicators
        pass

    def on_bar(self, bar):
        # TODO: strategy logic here
        pass
""".strip()

    if not prompt:
        return {"name": name, "code": base_template}

    try:
        if provider == "openai" and OPENAI_API_KEY:
            import httpx
            headers = {"Authorization": f"Bearer {OPENAI_API_KEY}"}
            body = {
                "model": payload.get("model", "gpt-4o-mini"),
                "messages": [
                    {
                        "role": "system",
                        "content": (
                            "You generate NautilusTrader Strategy Python code only. "
                            "Output valid Python code blocks without commentary."
                        ),
                    },
                    {"role": "user", "content": f"Create a NautilusTrader strategy named {name}: {prompt}"},
                ],
                "temperature": 0.2,
            }
            with httpx.Client(timeout=60) as client:
                resp = client.post("https://api.openai.com/v1/chat/completions", headers=headers, json=body)
                resp.raise_for_status()
                data = resp.json()
                text = data.get("choices", [{}])[0].get("message", {}).get("content", "")
                code = text or base_template
        elif provider == "anthropic" and ANTHROPIC_API_KEY:
            import httpx
            headers = {
                "x-api-key": ANTHROPIC_API_KEY,
                "anthropic-version": "2023-06-01",
            }
            body = {
                "model": payload.get("model", "claude-3-5-sonnet-latest"),
                "max_tokens": 2000,
                "temperature": 0.2,
                "messages": [
                    {"role": "user", "content": f"Create a NautilusTrader strategy named {name}: {prompt}. Output only Python code."}
                ],
            }
            with httpx.Client(timeout=60) as client:
                resp = client.post("https://api.anthropic.com/v1/messages", headers=headers, json=body)
                resp.raise_for_status()
                data = resp.json()
                # messages API returns content list
                parts = (data.get("content") or [])
                text = "\n".join([p.get("text", "") for p in parts if isinstance(p, dict)])
                code = text or base_template
        elif provider == "gemini" and GEMINI_API_KEY:
            import httpx
            model = payload.get("model", "gemini-1.5-pro")
            url = f"https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent?key={GEMINI_API_KEY}"
            body = {
                "contents": [
                    {
                        "parts": [
                            {"text": f"Create a NautilusTrader strategy named {name}: {prompt}. Output only Python code."}
                        ]
                    }
                ],
                "generationConfig": {"temperature": 0.2},
            }
            with httpx.Client(timeout=60) as client:
                resp = client.post(url, json=body)
                resp.raise_for_status()
                data = resp.json()
                # Extract text from candidates
                candidates = data.get("candidates") or []
                text = ""
                if candidates:
                    content = (candidates[0].get("content") or {})
                    parts = content.get("parts") or []
                    if parts and isinstance(parts[0], dict):
                        text = parts[0].get("text", "")
                code = text or base_template
        elif provider == "ollama":
            import httpx
            with httpx.Client(timeout=60) as client:
                resp = client.post(
                    f"{OLLAMA_API_URL}/api/generate",
                    json={"model": payload.get("model", "llama3.1"), "prompt": f"Create NautilusTrader strategy named {name}: {prompt}"},
                )
                resp.raise_for_status()
                # Ollama streams by default; we assume simple response for brevity
                code = resp.json().get("response", base_template)
        else:
            code = base_template
    except Exception as e:
        print(f"[studio] generation error: {e}")
        code = base_template

    return {"name": name, "code": code}


async def _gen_openai_stream(model: str | None, name: str, prompt: str) -> Any:
    import httpx

    headers = {"Authorization": f"Bearer {OPENAI_API_KEY}"}
    body = {
        "model": model or "gpt-4o-mini",
        "stream": True,
        "messages": [
            {"role": "system", "content": "You generate NautilusTrader Strategy Python code only. Output raw Python code."},
            {"role": "user", "content": f"Create a NautilusTrader strategy named {name}: {prompt}"},
        ],
        "temperature": 0.2,
    }
    async with httpx.AsyncClient(timeout=60) as client:
        async with client.stream("POST", "https://api.openai.com/v1/chat/completions", headers=headers, json=body) as resp:
            resp.raise_for_status()
            async for line in resp.aiter_lines():
                if not line or not line.startswith("data: "):
                    continue
                data = line[6:].strip()
                if data == "[DONE]":
                    break
                try:
                    obj = msgspec.json.decode(data.encode("utf-8"))
                    delta = (((obj.get("choices") or [{}])[0]).get("delta") or {}).get("content")
                    if delta:
                        yield delta
                except Exception:
                    # Best-effort fallback
                    yield ""


async def _gen_ollama_stream(model: str | None, name: str, prompt: str) -> Any:
    import httpx

    body = {"model": model or "llama3.1", "prompt": f"Create NautilusTrader strategy named {name}: {prompt}", "stream": True}
    async with httpx.AsyncClient(timeout=60) as client:
        async with client.stream("POST", f"{OLLAMA_API_URL}/api/generate", json=body) as resp:
            resp.raise_for_status()
            async for line in resp.aiter_lines():
                if not line:
                    continue
                try:
                    obj = msgspec.json.decode(line.encode("utf-8"))
                    piece = obj.get("response")
                    if piece:
                        yield piece
                    if obj.get("done"):
                        break
                except Exception as e:
                    logger.debug("studio.generate.stream: decode error: %s", e)
                    continue


@app.post("/studio/generate/stream")
async def studio_generate_stream(payload: dict[str, Any]) -> Response:
    """Stream strategy code generation as plain text chunks."""
    from fastapi.responses import StreamingResponse

    prompt: str = str(payload.get("prompt", "")).strip()
    name: str = payload.get("name") or "GeneratedStrategy"
    provider: str = (payload.get("provider") or ("openai" if OPENAI_API_KEY else "ollama")).lower()
    model: str | None = payload.get("model")

    if provider == "openai" and OPENAI_API_KEY:
        return StreamingResponse(_gen_openai_stream(model, name, prompt), media_type="text/plain")
    if provider == "ollama":
        return StreamingResponse(_gen_ollama_stream(model, name, prompt), media_type="text/plain")
    raise HTTPException(status_code=400, detail="Streaming not supported for provider")


def _parse_strategy_ast(code: str) -> tuple[Any | None, list[str]]:
    import ast

    try:
        return ast.parse(code), []
    except SyntaxError as e:
        return None, [f"SyntaxError: {e.msg} (line {e.lineno}, col {e.offset})"]


def _check_strategy_bases(tree: Any) -> tuple[bool, bool]:
    import ast

    has_strategy = False
    has_config = False
    for node in ast.walk(tree):
        if isinstance(node, ast.ClassDef):
            name_bases = {str(getattr(b, "id", "")) for b in node.bases if isinstance(b, ast.Name)}
            attr_bases = {str(getattr(b, "attr", "")) for b in node.bases if isinstance(b, ast.Attribute)}
            bases_str = name_bases | attr_bases
            if "Strategy" in bases_str:
                has_strategy = True
            if "StrategyConfig" in bases_str:
                has_config = True
    return has_strategy, has_config


def _scan_guardrails(tree: Any) -> tuple[list[str], list[str]]:
    import ast

    errors: list[str] = []
    warnings: list[str] = []
    dangerous_imports = {"os", "subprocess", "socket", "shutil", "pathlib", "requests"}
    dangerous_calls = {"eval", "exec", "compile", "open", "__import__"}

    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for alias in node.names:
                if alias.name.split(".")[0] in dangerous_imports:
                    warnings.append(f"Import of '{alias.name}' may be unsafe in strategy code")
        elif isinstance(node, ast.ImportFrom):
            mod = node.module or ""
            if mod.split(".")[0] in dangerous_imports:
                warnings.append(f"Import from '{mod}' may be unsafe in strategy code")
        elif isinstance(node, ast.Call):
            fn = node.func
            if isinstance(fn, ast.Name) and fn.id in dangerous_calls:
                errors.append(f"Call to '{fn.id}' is not allowed")
            if isinstance(fn, ast.Attribute) and isinstance(fn.value, ast.Name) and fn.value.id == "subprocess":
                errors.append("Use of subprocess.* is not allowed")
    return errors, warnings


@app.post("/studio/validate")
def studio_validate(payload: dict[str, Any]) -> dict[str, Any]:
    """
    Validate a generated strategy code snippet.

    - AST parse and basic checks (subclasses Strategy, config present)
    - Guardrail scan for dangerous imports and calls
    """
    code: str = str(payload.get("code") or "")
    res: dict[str, Any] = {"ok": False, "errors": [], "warnings": []}
    if not code:
        res["errors"].append("code is required")
        return res

    tree, parse_errs = _parse_strategy_ast(code)
    if parse_errs:
        res["errors"].extend(parse_errs)
        return res
    assert tree is not None

    has_strategy, has_config = _check_strategy_bases(tree)
    if not has_strategy:
        res["errors"].append("No class inherits from Strategy found")
    if not has_config:
        res["warnings"].append("No class inherits from StrategyConfig found")

    errs, warns = _scan_guardrails(tree)
    res["errors"].extend(errs)
    res["warnings"].extend(warns)

    res["ok"] = len(res["errors"]) == 0
    return res


@app.post("/studio/backtest")
def studio_backtest(payload: dict[str, Any]) -> dict[str, Any]:
    """Write a strategy to disk and run a one-shot backtest (keeps engine for reports)."""
    # Extract inputs
    name: str = (payload.get("name") or "GeneratedStrategy").strip()
    code: str = str(payload.get("code") or "").strip()
    instrument_id: str = str(payload.get("instrument_id") or "").strip()
    catalog_path: str = str(payload.get("catalog_path") or "").strip()
    venue: str | None = payload.get("venue")
    bar_spec: str | None = payload.get("bar_spec")
    start_time = payload.get("start_time")
    end_time = payload.get("end_time")

    if not (name and code and instrument_id and catalog_path):
        raise HTTPException(status_code=400, detail="name, code, instrument_id, catalog_path are required")

    # Persist code
    repo_root = Path.cwd()
    pkg_dir = repo_root / "user_strategies"
    pkg_dir.mkdir(parents=True, exist_ok=True)
    (pkg_dir / "__init__.py").touch()
    mod = "".join([c if c.isalnum() or c == "_" else "_" for c in name]).strip("_") or "strategy"
    (pkg_dir / f"{mod}.py").write_text(code, encoding="utf-8")
    if str(repo_root) not in sys.path:
        sys.path.insert(0, str(repo_root))

    # Guess venue if absent
    venue = venue or (
        instrument_id.split(":")[0] if ":" in instrument_id else (instrument_id.split(".")[-1] if "." in instrument_id else "SIM")
    )

    # Build run config
    strategy_import = f"user_strategies.{mod}:{name}"
    config_import = f"user_strategies.{mod}:{name}Config"
    run_cfg = {
        "id": name,
        "engine": {"strategies": [{"strategy_path": strategy_import, "config_path": config_import, "config": {"instrument_id": instrument_id}}]},
        "venues": [{"name": venue, "oms_type": "NETTING", "account_type": "CASH", "starting_balances": ["10000 USD"], "book_type": "L1_MBP"}],
        "data": [{
            "catalog_path": catalog_path,
            "data_cls": "nautilus_trader.model.data.Bar",
            "instrument_id": instrument_id,
            "bar_spec": bar_spec,
            "start_time": start_time,
            "end_time": end_time,
        }],
        "dispose_on_completion": False,
        "raise_exception": True,
    }

    # Run in background and retain engine
    job_id = str(uuid.uuid4())
    job = Job(id=job_id, kind=JobKind.backtest, status=JobStatus.running, started_at=time.time())
    _JOBS[job_id] = job
    _LOGS[job_id] = []
    _log(job_id, f"Studio backtest created for strategy {name}", "info")
    _bump_jobs_ver()

    def _worker():
        try:
            cfgs = msgspec.json.decode(msgspec.json.encode([run_cfg]), type=list[BacktestRunConfig], dec_hook=msgspec_decoding_hook)
            BacktestNode = _get_BacktestNode()
            node = BacktestNode(configs=cfgs)
            _log(job_id, "Building backtest node", "info")
            node.build()
            engines_map: dict[str, Any] = {}
            payload_out: list[dict[str, Any]] = []
            for cfg in node.configs:
                run_id = getattr(cfg, "id", None) or name
                engine = node.get_engine(run_id) or (node.get_engines()[0] if node.get_engines() else None)
                if engine is None:
                    raise RuntimeError("Engine not built")
                node._run_oneshot(run_config_id=run_id, engine=engine, data_configs=cfg.data, start=cfg.start, end=cfg.end)
                result = engine.get_result()
                _log(job_id, f"Run {run_id} finished: orders={result.total_orders} positions={result.total_positions}", "info")
                engine.clear_data()
                payload_out.append({"run_id": result.run_id, "stats_returns": result.stats_returns, "stats_pnls": result.stats_pnls})
                engines_map[run_id] = engine
            _BACKTESTS[job_id] = {"node": node, "engines": engines_map}
            job.status = JobStatus.completed
            job.finished_at = time.time()
            job.result = {"results": payload_out}
            _log(job_id, "Studio backtest completed", "info")
            _bump_jobs_ver()
            try:
                saved = _persist_backtest_artifacts(job_id)
                _log(job_id, f"Persisted {saved} artifact files", "debug")
            except Exception as perr:
                _log(job_id, f"Persist error: {perr}", "error")
        except Exception as e:
            job.status = JobStatus.failed
            job.finished_at = time.time()
            job.error = str(e)
            _log(job_id, f"Studio backtest failed: {e}", "error")
            _bump_jobs_ver()

    threading.Thread(target=_worker, daemon=True).start()
    return {"job_id": job_id, "run_config_id": name}


@app.post("/analytics/sentiment")
def analyze_sentiment(payload: dict[str, Any]) -> dict[str, Any]:
    """Analyze sentiment for provided text using vaderSentiment if available, else a naive heuristic."""
    text = str(payload.get("text", ""))
    if not text:
        return {"score": 0.0, "positive": 0.0, "negative": 0.0, "neutral": 1.0}

    try:
        from vaderSentiment.vaderSentiment import SentimentIntensityAnalyzer

        analyzer = SentimentIntensityAnalyzer()
        scores = analyzer.polarity_scores(text)
        return {
            "score": scores.get("compound", 0.0),
            "positive": scores.get("pos", 0.0),
            "negative": scores.get("neg", 0.0),
            "neutral": scores.get("neu", 0.0),
        }
    except Exception:
        # Naive fallback
        pos = sum(1 for w in text.lower().split() if w in {"good", "great", "bullish", "up", "positive", "win"})
        neg = sum(1 for w in text.lower().split() if w in {"bad", "bearish", "down", "negative", "loss"})
        total = max(len(text.split()), 1)
        score = (pos - neg) / total
        return {"score": score, "positive": pos / total, "negative": neg / total, "neutral": max(0.0, 1.0 - (pos + neg) / total)}


@app.get("/backtests/{job_id}/metrics")
def backtest_metrics(job_id: str, run_config_id: str | None = None) -> dict[str, Any]:
    """Return additional derived metrics for dashboards."""
    entry = _BACKTESTS.get(job_id)
    if not entry:
        raise HTTPException(status_code=404, detail="Backtest job not found or no engines retained")

    engines = entry.get("engines", {})
    engine = None
    if run_config_id and run_config_id in engines:
        engine = engines[run_config_id]
    elif engines:
        engine = next(iter(engines.values()))
    if engine is None:
        raise HTTPException(status_code=404, detail="Engine not found for job")

    trader = engine.trader
    import pandas as pd

    orders = trader.generate_orders_report()
    positions = trader.generate_positions_report()

    metrics: dict[str, Any] = {}

    # Win rate
    if positions is not None and not positions.empty and "realized_pnl" in positions.columns:
        wins = (pd.to_numeric(positions["realized_pnl"], errors="coerce").fillna(0) > 0).sum()
        total = len(positions)
        metrics["win_rate"] = float(wins) / float(total)

    # Average slippage (if present)
    if orders is not None and not orders.empty and "slippage" in orders.columns:
        metrics["avg_slippage"] = float(pd.to_numeric(orders["slippage"], errors="coerce").fillna(0).mean())

    # Average holding time
    if positions is not None and not positions.empty and "duration_ns" in positions.columns:
        dur = pd.to_numeric(positions["duration_ns"], errors="coerce").fillna(0)
        metrics["avg_holding_seconds"] = float(dur.mean()) / 1e9

    return metrics

@app.post("/live/validate")
def live_validate(config: dict) -> dict[str, Any]:
    """Validate TradingNodeConfig without starting the node; return a summary if OK."""
    errors: list[str] = []
    try:
        cfg_bytes = msgspec.json.encode(config)
        _ = msgspec.json.decode(cfg_bytes, type=TradingNodeConfig, dec_hook=msgspec_decoding_hook)
    except Exception as e:
        errors.append(str(e))
    if errors:
        return {"ok": False, "errors": errors}
    # Build a lightweight summary from the provided config dict
    try:
        venues = config.get("venues") or []
        strategies = config.get("strategies") or []
        actors = config.get("actors") or []
        res = {
            "ok": True,
            "summary": {
                "trader_id": config.get("trader_id"),
                "venues": len(venues) if isinstance(venues, list) else 0,
                "strategies": len(strategies) if isinstance(strategies, list) else 0,
                "actors": len(actors) if isinstance(actors, list) else 0,
                "log_level": config.get("log_level"),
            }
        }
        return res
    except Exception:
        return {"ok": True}


# Presets for quick-start live nodes

def _build_live_preset_config(
    preset: str,
    trader_id: str | None = None,
    load_all: bool = True,
    testnet: bool | None = None,
    account_type: str | None = None,
    portfolio_id: str | None = None,
) -> dict[str, Any]:
    preset_key = (preset or "").lower()
    trader = trader_id or "TRADER-001"
    if preset_key in {"binance_us_spot", "binance_us"}:
        # Defaults for Binance.US Spot
        acct = (account_type or "spot").lower()
        cfg = {
            "trader_id": trader,
            "data_clients": {
                "BINANCE": {
                    "api_key": None,  # use env: BINANCE_API_KEY
                    "api_secret": None,  # use env: BINANCE_API_SECRET
                    "account_type": acct,
                    "us": True,
                    "testnet": bool(testnet) if testnet is not None else False,
                    "instrument_provider": {"load_all": bool(load_all)},
                }
            },
            "exec_clients": {
                "BINANCE": {
                    "api_key": None,
                    "api_secret": None,
                    "account_type": acct,
                    "us": True,
                    "testnet": bool(testnet) if testnet is not None else False,
                    "instrument_provider": {"load_all": bool(load_all)},
                }
            },
        }
        return cfg
    if preset_key in {"coinbase", "coinbase_intx", "coinbase_perp"}:
        cfg = {
            "trader_id": trader,
            "data_clients": {
                "COINBASE_INTX": {
                    "api_key": None,  # env: COINBASE_INTX_API_KEY
                    "api_secret": None,  # env: COINBASE_INTX_API_SECRET
                    "api_passphrase": None,  # env: COINBASE_INTX_API_PASSPHRASE
                    "base_url_http": None,
                    "base_url_ws": None,
                    "http_timeout_secs": 60,
                    "instrument_provider": {"load_all": bool(load_all)},
                }
            },
            "exec_clients": {
                "COINBASE_INTX": {
                    "api_key": None,
                    "api_secret": None,
                    "api_passphrase": None,
                    "portfolio_id": portfolio_id,  # or env: COINBASE_INTX_PORTFOLIO_ID
                    "base_url_http": None,
                    "base_url_ws": None,
                    "http_timeout_secs": 60,
                    "instrument_provider": {"load_all": bool(load_all)},
                }
            },
        }
        return cfg
    raise HTTPException(status_code=400, detail="unknown preset")


@app.get("/live/presets")
def live_presets() -> dict[str, Any]:
    return {
        "presets": [
            {
                "name": "binance_us_spot",
                "requires_env": ["BINANCE_API_KEY", "BINANCE_API_SECRET"],
                "notes": "Uses account_type=spot; set BINANCE_* env vars in the API container; us=True",
            },
            {
                "name": "coinbase_intx",
                "requires_env": [
                    "COINBASE_INTX_API_KEY",
                    "COINBASE_INTX_API_SECRET",
                    "COINBASE_INTX_API_PASSPHRASE",
                    "COINBASE_INTX_PORTFOLIO_ID",
                ],
                "notes": "Coinbase International derivatives; instrument ids like BTC-PERP.COINBASE_INTX",
            },
        ]
    }


class LivePresetPayload(BaseModel):
    preset: str
    trader_id: str | None = None
    load_all: bool = True
    testnet: bool | None = None
    account_type: str | None = None
    portfolio_id: str | None = None


@app.post("/live/preset/start")
def live_preset_start(payload: LivePresetPayload) -> dict[str, Any]:
    cfg = _build_live_preset_config(
        preset=payload.preset,
        trader_id=payload.trader_id,
        load_all=payload.load_all,
        testnet=payload.testnet,
        account_type=payload.account_type,
        portfolio_id=payload.portfolio_id,
    )
    # Reuse /live/start path
    return start_live(cfg)


@app.post("/live/start")
def start_live(config: dict) -> dict[str, str]:
    job_id = str(uuid.uuid4())
    job = Job(id=job_id, kind=JobKind.live, status=JobStatus.starting, started_at=time.time())
    _JOBS[job_id] = job
    _LOGS[job_id] = []
    _log(job_id, "Starting live node", "info")

    def _worker():
        try:
            cfg_bytes = msgspec.json.encode(config)
            live_cfg = msgspec.json.decode(cfg_bytes, type=TradingNodeConfig, dec_hook=msgspec_decoding_hook)
            TradingNode = _get_TradingNode()
            node = TradingNode(config=live_cfg)
            try:
                _register_client_factories(node, live_cfg)
            except Exception as reg_err:
                _log(job_id, f"Factory registration warning: {reg_err}", "warn")
            # Register desk strategy to route API orders
            try:
                from services.api.trading.desk_strategy import DeskStrategy  # type: ignore
                from services.api.trading.desk_strategy import DeskStrategyConfig  # type: ignore
                desk = DeskStrategy(config=DeskStrategyConfig())
                node.trader.add_strategy(desk)
                _LIVE[job_id] = {"node": node, "thread": threading.current_thread(), "desk": desk}
                _log(job_id, "Desk strategy registered", "info")
            except Exception as e_desk:
                _log(job_id, f"Desk strategy registration failed: {e_desk}", "warn")
                _LIVE[job_id] = {"node": node, "thread": threading.current_thread()}
            _log(job_id, "Building TradingNode", "info")
            node.build()
            job.status = JobStatus.running
            _log(job_id, "Live node running", "info")
            try:
                _entry = _LIVE.get(job_id, {})
                _entry["node"] = node
                _entry["thread"] = threading.current_thread()
                _LIVE[job_id] = _entry
            except Exception:
                _LIVE[job_id] = {"node": node, "thread": threading.current_thread()}
            _bump_jobs_ver()
            node.run()
            job.status = JobStatus.stopped
            job.finished_at = time.time()
            _log(job_id, "Live node stopped", "info")
            _bump_jobs_ver()
        except Exception as e:
            job.status = JobStatus.failed
            job.finished_at = time.time()
            job.error = str(e)
            _log(job_id, f"Live job failed: {e}", "error")
            _bump_jobs_ver()
        finally:
            # Ensure resources are freed
            try:
                _entry = _LIVE.get(job_id)
                if _entry and _entry.get("node"):
                    _entry["node"].dispose()
            except Exception as cleanup_err:
                # Log cleanup error and continue
                print(f"[live/cleanup] job={job_id} error={cleanup_err}")
            _LIVE.pop(job_id, None)

    threading.Thread(target=_worker, daemon=True).start()
    return {"job_id": job_id}


@app.post("/live/{job_id}/orders")
def live_paper_order(job_id: str, payload: dict[str, Any]) -> dict[str, Any]:
    # Only allow in paper mode
    if TEAM is not None:
        try:
            st = TEAM.status()
            if st.get("mode") != "paper":
                raise HTTPException(status_code=400, detail="Orders allowed only in paper mode via API")
        except Exception:
            pass
    if job_id not in _LIVE:
        raise HTTPException(status_code=404, detail="Live job not found")
    # Accept a minimal order schema
    side = (payload.get("side") or "").lower()  # buy|sell
    qty = float(payload.get("qty") or 0)
    instrument_id = payload.get("instrument_id") or ""
    order_type = (payload.get("type") or "market").lower()  # market|limit
    limit_price = float(payload.get("limit_price") or 0)
    ref_price = payload.get("price")
    price = float(ref_price) if ref_price is not None else (limit_price if order_type == "limit" else 0.0)
    if side not in {"buy", "sell"} or qty <= 0 or not instrument_id:
        raise HTTPException(status_code=400, detail="invalid order payload")
    fee_bps = float(os.getenv("NAUTILUS_FEE_BPS", "10"))  # default 10 bps = 0.1%
    notional = abs(qty * price) if price else 0.0
    est_fee = notional * (fee_bps / 10000.0)
    oid = str(uuid.uuid4())
    rec = {
        "id": oid,
        "ts": time.time(),
        "instrument_id": instrument_id,
        "side": side,
        "qty": qty,
        "type": order_type,
        "limit_price": limit_price if order_type == "limit" else None,
        "price": price if price else None,
        "est_fee": est_fee if price else None,
        "status": "placed",
    }
    arr = _PAPER_ORDERS.setdefault(job_id, [])
    arr.append(rec)
    _log(job_id, f"paper order {side} {qty} {instrument_id} type={order_type} id={oid} est_fee={est_fee:.6f}")
    return rec


@app.get("/live/{job_id}/orders")
def live_list_orders(job_id: str) -> dict[str, Any]:
    if job_id not in _LIVE:
        raise HTTPException(status_code=404, detail="Live job not found")
    return {"orders": _PAPER_ORDERS.get(job_id, [])}


# Optional API key protection for live execution endpoints
_LIVE_TRADING_API_KEY = os.getenv("LIVE_TRADING_API_KEY")

def _check_live_auth(request: Request) -> None:
    if not _LIVE_TRADING_API_KEY:
        return
    auth = request.headers.get("authorization") or request.headers.get("Authorization") or ""
    if not auth.lower().startswith("bearer "):
        raise HTTPException(status_code=401, detail="missing bearer token")
    token = auth.split(" ", 1)[1].strip()
    if token != _LIVE_TRADING_API_KEY:
        raise HTTPException(status_code=401, detail="invalid token")


@app.post("/live/{job_id}/orders/execute")
def live_execute_order(job_id: str, payload: LiveOrderPayload, request: Request) -> dict[str, Any]:
    _check_live_auth(request)
    # Require live mode and competency (unless overridden); enforce compliance mode
    if TEAM is not None:
        st = TEAM.status()
        if st.get("mode") != "live":
            raise HTTPException(status_code=400, detail="Agents not in live mode")
        comp = TEAM.compute_competency()
        if not comp.get("meets_target", False) and not payload.override_competency:
            raise HTTPException(status_code=400, detail="Competency target not met; set override_competency to true to proceed")
        # Hard compliance requires an approval_id previously recorded by /agents/compliance/record
        if _COMPLIANCE_MODE == "hard":
            if not payload.approval_id:
                raise HTTPException(status_code=400, detail="approval_id required in hard compliance mode")
            try:
                log = TEAM.get_compliance_log(limit=200)
                ok = False
                now = time.time()
                for rec in (log.get("records") or []):
                    if str(rec.get("id")) == str(payload.approval_id) and bool(rec.get("approved", False)):
                        ts = float(rec.get("ts") or 0)
                        exp = rec.get("expires_at")
                        valid_age = True
                        if exp is not None:
                            try:
                                valid_age = now <= float(exp)
                            except Exception:
                                valid_age = False
                        else:
                            valid_age = (_COMPLIANCE_MAX_AGE_SEC <= 0) or ((now - ts) <= _COMPLIANCE_MAX_AGE_SEC)
                        if not valid_age:
                            continue
                        # Enforce optional constraints if present
                        instr_ok = (not rec.get("instrument_id")) or (str(rec.get("instrument_id")) == str(payload.instrument_id))
                        side_ok = (not rec.get("side")) or (str(rec.get("side")).lower() == str(payload.side).lower())
                        qty_ok = (not isinstance(rec.get("max_qty"), int | float)) or (float(payload.qty) <= float(rec.get("max_qty")))
                        tif_ok = (not rec.get("tif")) or (str(rec.get("tif")).upper() == str(payload.tif or "GTC").upper())
                        if instr_ok and side_ok and qty_ok and tif_ok:
                            ok = True
                            # Append event trail on approval usage
                            try:
                                TEAM.append_compliance_event(payload.approval_id, {  # type: ignore[attr-defined]
                                    "type": "used",
                                    "job_id": job_id,
                                    "instrument_id": payload.instrument_id,
                                    "side": payload.side,
                                    "qty": payload.qty,
                                    "tif": payload.tif or "GTC",
                                })
                            except Exception:
                                pass
                            break
                if not ok:
                    raise HTTPException(status_code=403, detail="No valid compliance approval for approval_id (expired, not found, or constraints not met)")
            except HTTPException:
                raise
            except Exception as e:
                raise HTTPException(status_code=500, detail=f"Compliance check failed: {e}")
    entry = _LIVE.get(job_id)
    if not entry:
        raise HTTPException(status_code=404, detail="Live job not found")
    desk = entry.get("desk")
    if desk is None:
        raise HTTPException(status_code=503, detail="Desk strategy not available")

    # Map tif if provided
    tif_val = None
    if payload.tif:
        try:
            from nautilus_trader.model.enums import TimeInForce
            tif_val = TimeInForce[payload.tif.upper()]
        except Exception:
            raise HTTPException(status_code=400, detail="Invalid tif value")

    # Route order via desk strategy
    try:
        if payload.type.lower() == "market":
            order = desk.submit_market(
                instrument_id=payload.instrument_id,
                side=payload.side,
                qty=payload.qty,
                tif=tif_val,
            )
        elif payload.type.lower() == "limit":
            if payload.price is None or payload.price <= 0:
                raise HTTPException(status_code=400, detail="price required for limit orders")
            order = desk.submit_limit(
                instrument_id=payload.instrument_id,
                side=payload.side,
                qty=payload.qty,
                price=payload.price,
                tif=tif_val,
                post_only=payload.post_only,
            )
        else:
            raise HTTPException(status_code=400, detail="Unsupported order type")
    except HTTPException:
        raise
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))

    # Respond with client_order_id and basic info
    return {
        "client_order_id": order.client_order_id.to_str() if hasattr(order, "client_order_id") else None,
        "instrument_id": payload.instrument_id,
        "side": payload.side,
        "type": payload.type,
        "qty": payload.qty,
        "price": payload.price,
        "tif": payload.tif or "GTC",
        "placed_at": time.time(),
    }


@app.post("/live/{job_id}/orders/execute/cancel")
def live_execute_cancel(job_id: str, payload: LiveCancelPayload, request: Request) -> dict[str, Any]:
    _check_live_auth(request)
    entry = _LIVE.get(job_id)
    if not entry:
        raise HTTPException(status_code=404, detail="Live job not found")
    desk = entry.get("desk")
    if desk is None:
        raise HTTPException(status_code=503, detail="Desk strategy not available")
    try:
        ok = desk.cancel_by_client_order_id(payload.instrument_id, payload.client_order_id)
        if not ok:
            raise HTTPException(status_code=404, detail="Client order not found for desk strategy")
        return {"ok": True}
    except HTTPException:
        raise
    except Exception as e:
        raise HTTPException(status_code=500, detail=str(e))


class CancelPayload(BaseModel):
    order_id: str


class LiveOrderPayload(BaseModel):
    instrument_id: str
    side: str  # buy|sell
    qty: float
    type: str = "market"  # market|limit
    price: float | None = None
    tif: str | None = None  # GTC|IOC|FOK|GTD (string)
    post_only: bool = False
    override_competency: bool = False
    approval_id: str | None = None  # required in hard compliance mode


class LiveCancelPayload(BaseModel):
    instrument_id: str
    client_order_id: str


class LiveComplianceModePayload(BaseModel):
    mode: str  # 'soft' | 'hard'


@app.get("/live/compliance/mode")
def live_get_compliance_mode() -> dict[str, Any]:
    return {"mode": _COMPLIANCE_MODE, "max_age_sec": _COMPLIANCE_MAX_AGE_SEC}


@app.post("/live/compliance/mode")
def live_set_compliance_mode(payload: LiveComplianceModePayload) -> dict[str, Any]:
    global _COMPLIANCE_MODE
    m = (payload.mode or "").strip().lower()
    if m not in {"soft", "hard"}:
        raise HTTPException(status_code=400, detail="mode must be 'soft' or 'hard'")
    _COMPLIANCE_MODE = m
    return {"ok": True, "mode": _COMPLIANCE_MODE}


@app.post("/live/{job_id}/cancel")
def live_cancel_order(job_id: str, payload: CancelPayload) -> dict[str, Any]:
    if job_id not in _LIVE:
        raise HTTPException(status_code=404, detail="Live job not found")
    orders = _PAPER_ORDERS.get(job_id, [])
    for o in orders:
        if o.get("id") == payload.order_id:
            o["status"] = "canceled"
            _log(job_id, f"paper order canceled id={payload.order_id}")
            return {"ok": True, "order": o}
    raise HTTPException(status_code=404, detail="Order not found")


@app.post("/live/stop/{job_id}")
def stop_live(job_id: str) -> dict[str, str]:
    entry = _LIVE.get(job_id)
    if not entry:
        raise HTTPException(status_code=404, detail="Live job not found")
    node = cast(Any, entry["node"])  # type: ignore[assignment]
    try:
        node.dispose()
        _log(job_id, "Live node disposed via stop", "info")
        job = _JOBS.get(job_id)
        if job:
            job.status = JobStatus.stopped
            job.finished_at = time.time()
            _bump_jobs_ver()
    finally:
        _LIVE.pop(job_id, None)
    return {"status": "stopped", "job_id": job_id}


@app.get("/backtests/{job_id}/reports/{report_type}.csv")
def download_backtest_report_csv(
    job_id: str,
    report_type: str,
    run_config_id: str | None = None,
) -> Response:
    """Download a single CSV report (orders, order_fills, fills, positions)."""
    entry = _BACKTESTS.get(job_id)
    if not entry:
        raise HTTPException(status_code=404, detail="Backtest job not found or no engines retained")

    engines = entry.get("engines", {})
    engine = None
    if run_config_id and run_config_id in engines:
        engine = engines[run_config_id]
    elif engines:
        engine = next(iter(engines.values()))

    if engine is None:
        raise HTTPException(status_code=404, detail="Engine not found for job")

    trader = engine.trader
    import pandas as pd

    if report_type == "orders":
        df = trader.generate_orders_report()
    elif report_type == "order_fills":
        df = trader.generate_order_fills_report()
    elif report_type == "fills":
        df = trader.generate_fills_report()
    elif report_type == "positions":
        df = trader.generate_positions_report()
    else:
        raise HTTPException(status_code=400, detail="Unknown report type")

    if df is None or (isinstance(df, pd.DataFrame) and df.empty):
        return Response(
            content="",
            media_type="text/csv",
            headers={"Content-Disposition": f"attachment; filename={report_type}_{job_id}.csv"},
        )

    csv_data = df.to_csv(index=True)
    return Response(
        content=csv_data,
        media_type="text/csv",
        headers={"Content-Disposition": f"attachment; filename={report_type}_{job_id}.csv"},
    )


@app.get("/backtests/{job_id}/performance.csv")
def download_backtest_performance_csv(
    job_id: str,
    run_config_id: str | None = None,
) -> Response:
    """Download performance time series as CSV."""
    entry = _BACKTESTS.get(job_id)
    if not entry:
        raise HTTPException(status_code=404, detail="Backtest job not found or no engines retained")

    engines = entry.get("engines", {})
    engine = None
    if run_config_id and run_config_id in engines:
        engine = engines[run_config_id]
    elif engines:
        engine = next(iter(engines.values()))

    if engine is None:
        raise HTTPException(status_code=404, detail="Engine not found for job")

    analyzer = engine._kernel.portfolio.analyzer
    returns_series = getattr(analyzer, "returns")() if hasattr(analyzer, "returns") else None

    import pandas as pd

    if returns_series is None or returns_series.empty:
        return Response(
            content="",
            media_type="text/csv",
            headers={"Content-Disposition": f"attachment; filename=performance_{job_id}.csv"},
        )

    perf_df = pd.DataFrame({
        "ts": returns_series.index,
        "return": returns_series.to_numpy(),
    })
    perf_df["cum_return"] = perf_df["return"].cumsum()

    csv_data = perf_df.to_csv(index=False)
    return Response(
        content=csv_data,
        media_type="text/csv",
        headers={"Content-Disposition": f"attachment; filename=performance_{job_id}.csv"},
    )


@app.post("/backtests/{job_id}/reports/save")
def save_backtest_reports(job_id: str, run_config_id: str | None = None) -> dict[str, Any]:
    count = _persist_backtest_artifacts(job_id, run_config_id)
    return {"job_id": job_id, "run_config_id": run_config_id, "saved": count, "path": str((ARTIFACTS_ROOT / job_id).resolve())}


@app.get("/backtests/{job_id}/equity.csv")
def download_backtest_equity_csv(
    job_id: str,
    run_config_id: str | None = None,
) -> Response:
    """Download equity time series as CSV (all currencies)."""
    entry = _BACKTESTS.get(job_id)
    if not entry:
        raise HTTPException(status_code=404, detail="Backtest job not found or no engines retained")

    engines = entry.get("engines", {})
    engine = None
    if run_config_id and run_config_id in engines:
        engine = engines[run_config_id]
    elif engines:
        engine = next(iter(engines.values()))

    if engine is None:
        raise HTTPException(status_code=404, detail="Engine not found for job")

    import pandas as pd

    eq = get_backtest_equity(job_id, run_config_id)
    all_rows = []
    for ccy, points in eq.get("equity", {}).items():
        for pt in points:
            all_rows.append({"currency": ccy, "ts": pt["ts"], "value": pt["value"]})

    if not all_rows:
        return Response(
            content="",
            media_type="text/csv",
            headers={"Content-Disposition": f"attachment; filename=equity_{job_id}.csv"},
        )

    df = pd.DataFrame(all_rows)
    csv_data = df.to_csv(index=False)
    return Response(
        content=csv_data,
        media_type="text/csv",
        headers={"Content-Disposition": f"attachment; filename=equity_{job_id}.csv"},
    )


# Strategy management endpoints
@app.get("/studio/strategies/list")
def strategies_list() -> dict[str, Any]:
    repo_root = Path.cwd()
    pkg_dir = repo_root / "user_strategies"
    items: list[dict[str, Any]] = []
    if pkg_dir.exists() and pkg_dir.is_dir():
        for p in sorted(pkg_dir.glob("*.py")):
            items.append({"name": p.stem, "path": str(p.resolve())})
    return {"strategies": items}


@app.get("/studio/strategies/get")
def strategies_get(name: str) -> dict[str, Any]:
    repo_root = Path.cwd()
    pkg_dir = (repo_root / "user_strategies").resolve()
    mod = "".join([c if c.isalnum() or c == "_" else "_" for c in name]).strip("_") or "strategy"
    target = (pkg_dir / f"{mod}.py").resolve()
    if not str(target).startswith(str(pkg_dir)) or not target.exists():
        raise HTTPException(status_code=404, detail="Strategy not found")
    return {"name": mod, "code": target.read_text(encoding="utf-8")}


@app.post("/studio/strategies/save")
def strategies_save(payload: dict[str, Any]) -> dict[str, Any]:
    name: str = (payload.get("name") or "").strip()
    code: str = str(payload.get("code") or "")
    if not name:
        raise HTTPException(status_code=400, detail="name is required")
    repo_root = Path.cwd()
    pkg_dir = (repo_root / "user_strategies").resolve()
    pkg_dir.mkdir(parents=True, exist_ok=True)
    (pkg_dir / "__init__.py").touch()
    mod = "".join([c if c.isalnum() or c == "_" else "_" for c in name]).strip("_") or "strategy"
    target = (pkg_dir / f"{mod}.py").resolve()
    if not str(target).startswith(str(pkg_dir)):
        raise HTTPException(status_code=400, detail="Invalid name")
    target.write_text(code, encoding="utf-8")
    return {"ok": True, "name": mod, "path": str(target)}


@app.post("/studio/strategies/delete")
def strategies_delete(payload: dict[str, Any]) -> dict[str, Any]:
    name: str = (payload.get("name") or "").strip()
    if not name:
        raise HTTPException(status_code=400, detail="name is required")
    repo_root = Path.cwd()
    pkg_dir = (repo_root / "user_strategies").resolve()
    mod = "".join([c if c.isalnum() or c == "_" else "_" for c in name]).strip("_") or "strategy"
    target = (pkg_dir / f"{mod}.py").resolve()
    if not str(target).startswith(str(pkg_dir)) or not target.exists():
        raise HTTPException(status_code=404, detail="Strategy not found")
    target.unlink()
    return {"ok": True}


# ----------------------
# Datasets (catalog + jobs)
# ----------------------
_DATA_JOBS: dict[str, dict[str, Any]] = {}
_DATA_JOBS_VER = 0


def _bump_data_jobs_ver() -> None:
    global _DATA_JOBS_VER
    _DATA_JOBS_VER += 1
    _persist_data_jobs_state()


@app.get("/datasets")
def list_datasets() -> dict[str, Any]:
    items: list[dict[str, Any]] = []
    for p in sorted(DATASETS_ROOT.glob("*")):
        if not p.is_dir():
            continue
        stat = p.stat()
        summary_path = p / "summary.json"
        summary: dict[str, Any] | None = None
        try:
            if summary_path.exists():
                summary = json.loads(summary_path.read_text(encoding="utf-8"))
        except Exception:
            summary = None
        items.append({
            "name": p.name,
            "path": str(p.resolve()),
            "mtime": stat.st_mtime,
            "size": sum((f.stat().st_size for f in p.rglob("*") if f.is_file()), 0),
            **({"summary": summary} if summary else {}),
        })
    return {"datasets": items}


class DatasetImportPayload(BaseModel):
    name: str
    source: str | None = None  # path to an existing Nautilus catalog (directory containing 'data/')


@app.post("/datasets/import")
def datasets_import(payload: DatasetImportPayload) -> dict[str, Any]:
    job_id = str(uuid.uuid4())
    logs: list[dict[str, Any]] = []
    job: dict[str, Any] = {
        "id": job_id,
        "name": payload.name,
        "status": "queued",
        "progress": 0,
        "started_at": time.time(),
        "logs": logs,
    }
    _DATA_JOBS[job_id] = job
    _bump_data_jobs_ver()

    def wlog(msg: str) -> None:
        t = time.time()
        logs.append({"ts": t, "message": msg})

    def _worker():
        job["status"] = "running"
        _bump_data_jobs_ver()
        try:
            name = (payload.name or "").strip()
            if not name:
                raise ValueError("name is required")

            src = Path(payload.source.strip()) if payload.source else None
            target = (DATASETS_ROOT / name).resolve()
            wlog(f"Target: {target}")
            target.mkdir(parents=True, exist_ok=True)

            # Two modes:
            #  - If `source` provided: validate and copy known subdirs.
            #  - If `source` omitted: scaffold a minimal, empty catalog (to support tests and quick demos).
            if src is None:
                wlog("No source provided; creating empty catalog scaffold")
                # Create minimal structure
                (target / "data").mkdir(parents=True, exist_ok=True)
                # Write empty summary
                summary = _build_dataset_summary(target)
                (target / "summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
                job["progress"] = 100
                job["status"] = "completed"
                job["finished_at"] = time.time()
                _bump_data_jobs_ver()
                wlog("Completed")
                return

            # Validate and normalize source path
            wlog(f"Validating source: {src}")
            if not src.exists() or not src.is_dir():
                raise FileNotFoundError(f"source not found: {src}")
            if (src / "catalog").exists() and (src / "catalog").is_dir():
                src = src / "catalog"
            cat_data = src / "data"
            if not cat_data.exists() or not cat_data.is_dir():
                raise ValueError("source must be a Nautilus catalog root (missing 'data' dir)")

            # Copy catalog: prefer rsync-like merge to preserve existing files
            subdirs = ["data", "backtest", "live"]
            total_steps = 3 + sum(1 for d in subdirs if (src / d).exists())
            step_i = 0

            def step(msg: str):
                nonlocal step_i
                step_i += 1
                wlog(msg)
                job["progress"] = int(min(99, (step_i / max(total_steps, 1)) * 100))
                _bump_data_jobs_ver()

            step("Copying catalog subdirectories")
            for d in subdirs:
                s = src / d
                if not s.exists():
                    continue
                dst = target / d
                dst.mkdir(parents=True, exist_ok=True)
                for root, _dirs, files in os.walk(s):
                    rel = Path(root).relative_to(s)
                    (dst / rel).mkdir(parents=True, exist_ok=True)
                    for fn in files:
                        sp = Path(root) / fn
                        dp = dst / rel / fn
                        try:
                            shutil.copy2(sp, dp)
                        except Exception as e:
                            wlog(f"copy failed: {sp} -> {dp}: {e}")

            step("Writing summary")
            summary = _build_dataset_summary(target)
            (target / "summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")

            step("Finalizing")
            job["status"] = "completed"
            job["finished_at"] = time.time()
            job["progress"] = 100
            _bump_data_jobs_ver()
            wlog("Completed")
        except Exception as e:
            job["status"] = "failed"
            job["error"] = str(e)
            job["finished_at"] = time.time()
            _bump_data_jobs_ver()
            wlog(f"Error: {e}")

    threading.Thread(target=_worker, daemon=True).start()
    return {"job_id": job_id}


@app.get("/datasets/jobs")
def datasets_jobs() -> dict[str, Any]:
    jobs = sorted(_DATA_JOBS.values(), key=lambda j: j.get("started_at", 0), reverse=True)
    return {"jobs": jobs, "ver": _DATA_JOBS_VER}


@app.get("/datasets/jobs/stream")
async def datasets_jobs_stream(request: Request) -> Response:
    from fastapi.responses import StreamingResponse
    async def eventgen():
        last = -1
        while True:
            if await request.is_disconnected():
                break
            if last != _DATA_JOBS_VER:
                last = _DATA_JOBS_VER
                data = {"ver": _DATA_JOBS_VER, "jobs": sorted(_DATA_JOBS.values(), key=lambda j: j.get("started_at", 0), reverse=True)}
                yield f"data: {json.dumps(data)}\n\n"
            await asyncio.sleep(1.0)
    return StreamingResponse(eventgen(), media_type="text/event-stream")


@app.post("/datasets/cleanup")
def datasets_cleanup(name: str) -> dict[str, Any]:
    target = (DATASETS_ROOT / name)
    if not target.exists():
        raise HTTPException(status_code=404, detail="dataset not found")
    # Count files for reporting, then remove the entire dataset directory tree
    removed = sum(1 for p in target.rglob("*") if p.is_file())
    try:
        shutil.rmtree(target)
    except Exception as e:
        logger.debug("datasets.cleanup: rmtree failed for %s: %s", target, e)
        raise HTTPException(status_code=500, detail=f"failed to remove dataset: {e}")
    return {"ok": True, "removed": removed}


# -------- Dataset describe utilities --------
try:
    from nautilus_trader.persistence.catalog.parquet import ParquetDataCatalog
    from nautilus_trader.persistence.funcs import filename_to_class
    _CAT_AVAILABLE = True
except Exception:
    _CAT_AVAILABLE = False


def _build_dataset_summary(root: Path) -> dict[str, Any]:
    out: dict[str, Any] = {"generated_at": time.time(), "types": [], "ranges": {}, "counts": {}}
    data_dir = root / "data"
    if not data_dir.exists():
        return out
    types: list[str] = []
    for p in sorted(data_dir.glob("*")):
        if p.is_dir():
            types.append(p.name)
    out["types"] = types
    if not _CAT_AVAILABLE:
        return out
    try:
        cat = ParquetDataCatalog(str(root))
        for t in types:
            try:
                cls = filename_to_class(t)
            except Exception:
                continue
            # Count instruments and files
            inst_dirs = [d for d in (data_dir / t).glob("*") if d.is_dir()]
            file_count = len(list((data_dir / t).rglob("*.parquet")))
            out["counts"][t] = {"instruments": len(inst_dirs), "files": file_count}
            # Aggregate intervals across all identifiers by union of instrument folders
            start_ns = None
            end_ns = None
            # If instrument subfolders exist, aggregate per subfolder; else aggregate directly
            if inst_dirs:
                for d in inst_dirs:
                    # derive identifier from folder name (already urisafe)
                    intervals = cat.get_intervals(cls, d.name)
                    if intervals:
                        s = intervals[0][0]
                        e = intervals[-1][1]
                        start_ns = s if start_ns is None or s < start_ns else start_ns
                        end_ns = e if end_ns is None or e > end_ns else end_ns
            else:
                intervals = cat.get_intervals(cls, None)
                if intervals:
                    start_ns = intervals[0][0]
                    end_ns = intervals[-1][1]
            if start_ns is not None and end_ns is not None:
                out["ranges"][t] = {
                    "start_ns": int(start_ns),
                    "end_ns": int(end_ns),
                    "start_iso": datetime.utcfromtimestamp(start_ns/1e9).isoformat()+"Z",
                    "end_iso": datetime.utcfromtimestamp(end_ns/1e9).isoformat()+"Z",
                }
    except Exception as e:
        print(f"[datasets] summary error: {e}")
    return out


@app.get("/datasets/describe")
def datasets_describe(name: str) -> dict[str, Any]:
    target = (DATASETS_ROOT / name)
    if not target.exists() or not target.is_dir():
        raise HTTPException(status_code=404, detail="dataset not found")
    # Try to return cached summary first
    summary_path = target / "summary.json"
    if summary_path.exists():
        try:
            return json.loads(summary_path.read_text(encoding="utf-8"))
        except Exception:
            pass
    # Rebuild summary
    summary = _build_dataset_summary(target)
    try:
        (target / "summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
    except Exception:
        pass
    return summary


@app.get("/venues/presets")
def venues_presets() -> dict[str, Any]:
    presets = [
        {
            "name": "BINANCE",
            "oms_type": "NETTING",
            "account_type": "CASH",
            "book_type": "L1_MBP",
            "starting_balances": ["10000 USD"],
        },
        {
            "name": "BYBIT",
            "oms_type": "NETTING",
            "account_type": "CASH",
            "book_type": "L1_MBP",
            "starting_balances": ["10000 USD"],
        },
        {
            "name": "OKX",
            "oms_type": "NETTING",
            "account_type": "CASH",
            "book_type": "L1_MBP",
            "starting_balances": ["10000 USD"],
        },
    ]
    return {"presets": presets}


@app.get("/engine/presets")
def engine_presets() -> dict[str, Any]:
    presets = [
        {
            "name": "default",
            "risk_engine": {
                "qsize": 100000,
                "graceful_shutdown_on_exception": False
            },
            "exec_engine": {
                "reconciliation": True,
                "inflight_check_interval_ms": 2000,
                "inflight_check_threshold_ms": 5000,
                "inflight_check_retries": 5,
                "open_check_interval_secs": 10,
                "open_check_open_only": True,
                "open_check_lookback_mins": 60,
                "qsize": 100000,
                "reconciliation_startup_delay_secs": 10.0
            }
        },
        {
            "name": "hft_low_latency",
            "risk_engine": {
                "qsize": 200000,
                "graceful_shutdown_on_exception": False
            },
            "exec_engine": {
                "reconciliation": True,
                "inflight_check_interval_ms": 1000,
                "inflight_check_threshold_ms": 3000,
                "inflight_check_retries": 5,
                "open_check_interval_secs": 5,
                "open_check_open_only": True,
                "open_check_lookback_mins": 30,
                "qsize": 200000,
                "reconciliation_startup_delay_secs": 5.0
            }
        }
    ]
    return {"presets": presets}


def _extract_instrument_id(dtype: str, folder_name: str) -> str:
    # For bars, folder name often starts with instrument_id-...; otherwise use as-is
    if dtype.lower().startswith("bar") and "-" in folder_name:
        return folder_name.split("-")[0]
    return folder_name


@lru_cache(maxsize=128)
def _list_instruments_cached(dataset: str, dtype: str | None) -> list[str]:
    root = (DATASETS_ROOT / dataset / "data")
    if not root.exists():
        return []
    types = [dtype] if dtype else [p.name for p in root.iterdir() if p.is_dir()]
    seen: set[str] = set()
    results: list[str] = []
    for t in types:
        tdir = root / t
        if not tdir.exists() or not tdir.is_dir():
            continue
        # instrument subfolders
        for sub in tdir.iterdir():
            if sub.is_dir():
                iid = _extract_instrument_id(t, sub.name)
                if iid not in seen:
                    seen.add(iid)
                    results.append(iid)
    results.sort()
    return results


@app.get("/catalog/instruments")
def catalog_instruments(dataset: str, q: str | None = None, dtype: str | None = None, limit: int = 50) -> dict[str, Any]:
    if not dataset:
        raise HTTPException(status_code=400, detail="dataset is required")
    all_ids = _list_instruments_cached(dataset, dtype)
    if q:
        ql = q.lower()
        all_ids = [i for i in all_ids if ql in i.lower()]
    limit = max(1, min(int(limit or 50), 500))
    return {"instruments": all_ids[:limit], "total": len(all_ids)}
