import { Activity, AlertTriangle, RefreshCw, Server } from "lucide-react";

import type { JobStats, ShellLink } from "../api";
import { buildPulseHref, buildShellHref } from "../basePath";
import { REFRESH_INTERVAL_MS } from "../theme";
import { StatusPill } from "./StatusPill";

interface TopBarProps {
  stats: JobStats;
  shellLinks: ShellLink[];
  autoRefresh: boolean;
  isRefreshing: boolean;
  onRefresh: () => void;
  onToggleAutoRefresh: () => void;
}

export function TopBar({
  stats,
  shellLinks,
  autoRefresh,
  isRefreshing,
  onRefresh,
  onToggleAutoRefresh,
}: TopBarProps) {
  const pulseHref = buildPulseHref();

  return (
    <header className="topbar">
      <div className="suite-nav" aria-label="Primary">
        <div className="suite-nav__brand" aria-label="flux">
          flux
        </div>

        <div className="suite-nav__links">
          {shellLinks.map((link) => (
            <a key={link.path} href={buildShellHref(link.path)} className="nav-link nav-link--primary">
              {link.label}
            </a>
          ))}
          <a href={pulseHref} className="nav-link nav-link--primary nav-link--active" aria-current="page">
            Pulse
          </a>
        </div>
      </div>

      <div className="topbar__title-row">
        <div>
          <h1 className="topbar__title">Pulse</h1>
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
