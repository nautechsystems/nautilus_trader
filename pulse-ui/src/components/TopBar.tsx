import { Activity, AlertTriangle, RefreshCw, Server } from "lucide-react";

import type { JobStats } from "../api";
import { REFRESH_INTERVAL_MS } from "../theme";
import { StatusPill } from "./StatusPill";

interface TopBarProps {
  stats: JobStats;
  autoRefresh: boolean;
  isRefreshing: boolean;
  onRefresh: () => void;
  onToggleAutoRefresh: () => void;
}

export function TopBar({
  stats,
  autoRefresh,
  isRefreshing,
  onRefresh,
  onToggleAutoRefresh,
}: TopBarProps) {
  return (
    <header className="topbar">
      <div className="topbar__title-row">
        <div>
          <h1 className="topbar__title">Pulse</h1>
          <p className="topbar__subtitle">Flux deployment control for process jobs</p>
        </div>

        <div className="topbar__controls">
          <label className="toggle">
            <input type="checkbox" checked={autoRefresh} onChange={onToggleAutoRefresh} />
            <span>Auto-refresh {REFRESH_INTERVAL_MS / 1000}s</span>
          </label>

          <button type="button" className="button button--primary" onClick={onRefresh} disabled={isRefreshing}>
            <RefreshCw className={isRefreshing ? "spin" : ""} size={15} />
            <span>Refresh</span>
          </button>
        </div>
      </div>

      <div className="stats-grid">
        <div className="stat-card">
          <Server size={14} />
          <span className="stat-card__label">Total</span>
          <strong>{stats.total}</strong>
        </div>
        <div className="stat-card">
          <Activity size={14} />
          <span className="stat-card__label">Active</span>
          <strong className="text-success">{stats.active}</strong>
        </div>
        <div className="stat-card">
          <AlertTriangle size={14} />
          <span className="stat-card__label">Failed</span>
          <strong className="text-danger">{stats.failed}</strong>
        </div>
        <div className="stat-card stat-card--stacked">
          <span className="stat-card__label">Errors</span>
          <StatusPill
            label={`${stats.totalErrors}`}
            tone={stats.totalErrors === 0 ? "success" : stats.totalErrors > 5 ? "danger" : "warning"}
          />
        </div>
      </div>
    </header>
  );
}
