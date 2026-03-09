import { AlertTriangle, Clock3 } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { calculateStats, getJobs, groupJobs, performGroupAction, performJobAction, type Job, type ShellLink } from "./api";
import { JobGroup } from "./components/JobGroup";
import { JobCards } from "./components/JobCards";
import { LogsModal } from "./components/LogsModal";
import { TopBar } from "./components/TopBar";
import type { LogFilter } from "./logs";
import { REFRESH_INTERVAL_MS } from "./theme";

interface OpenLogsState {
  jobId: string;
  jobName: string;
  jobCmd?: string | null;
  initialFilter?: LogFilter;
}

interface ActionResult {
  success: boolean;
  message?: string;
  pending?: boolean;
  deferred?: string[];
  errors?: string[];
}

function jobIdOf(job: Job): string {
  return job.id || job.name;
}

function compactText(value: string): string {
  return value.replace(/\s+/g, " ").trim();
}

function actionFailureMessage(result: ActionResult, fallback: string): string {
  const summary = compactText(result.message || fallback);
  const firstError = result.errors?.find(Boolean);
  if (!firstError) {
    return summary;
  }

  const detail = compactText(firstError);
  const remaining = (result.errors?.length || 0) - 1;
  return remaining > 0 ? `${summary}: ${detail} (+${remaining} more)` : `${summary}: ${detail}`;
}

function actionSuccessMessage(result: ActionResult, fallback: string): string {
  const summary = compactText(result.message || fallback);
  const details: string[] = [];

  if (result.pending) {
    details.push("pending");
  }

  const deferred = result.deferred?.map((item) => compactText(item)).filter(Boolean) ?? [];
  if (deferred.length > 0) {
    details.push(`deferred: ${deferred.join(", ")}`);
  }

  return details.length > 0 ? `${summary} (${details.join("; ")})` : summary;
}

export default function App() {
  const [jobs, setJobs] = useState<Job[]>([]);
  const [shellLinks, setShellLinks] = useState<ShellLink[]>([]);
  const [loading, setLoading] = useState(true);
  const [autoRefresh, setAutoRefresh] = useState(true);
  const [showOnlyErrors, setShowOnlyErrors] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [lastUpdated, setLastUpdated] = useState<number | null>(null);
  const [openLogs, setOpenLogs] = useState<OpenLogsState | null>(null);
  const [busyJobIds, setBusyJobIds] = useState<Set<string>>(new Set());
  const [busyGroupKeys, setBusyGroupKeys] = useState<Set<string>>(new Set());
  const busyGroupKeysRef = useRef<Set<string>>(new Set());
  const refreshTimer = useRef<number | null>(null);
  const [isCompactLayout, setIsCompactLayout] = useState(() =>
    typeof window !== "undefined" && typeof window.matchMedia === "function"
      ? window.matchMedia("(max-width: 760px)").matches
      : false,
  );

  async function loadJobs(options?: { silent?: boolean }) {
    if (!options?.silent) {
      setLoading(true);
    }

    setError(null);

    try {
      const payload = await getJobs();
      setJobs(payload.jobs);
      setShellLinks(payload.shell_links || []);
      setLastUpdated(Date.now());
    } catch (err) {
      setShellLinks([]);
      setError(err instanceof Error ? err.message : "Failed to fetch jobs");
    } finally {
      setLoading(false);
    }
  }

  async function handleJobAction(jobId: string, action: "start" | "stop" | "restart") {
    setBusyJobIds((current) => new Set(current).add(jobId));
    setError(null);
    setMessage(null);

    try {
      const response = await performJobAction(jobId, action);
      if (!response.success) {
        throw new Error(actionFailureMessage(response, `Failed to ${action} ${jobId}`));
      }
      setMessage(actionSuccessMessage(response, `Requested ${action} for ${jobId}`));
      await loadJobs({ silent: true });
    } catch (err) {
      setError(err instanceof Error ? err.message : `Failed to ${action} ${jobId}`);
    } finally {
      setBusyJobIds((current) => {
        const next = new Set(current);
        next.delete(jobId);
        return next;
      });
    }
  }

  async function handleGroupAction(groupKey: string, action: "start" | "stop" | "restart") {
    if (busyGroupKeysRef.current.has(groupKey)) {
      return;
    }

    const nextBusyGroups = new Set(busyGroupKeysRef.current).add(groupKey);
    busyGroupKeysRef.current = nextBusyGroups;
    setBusyGroupKeys(nextBusyGroups);
    setError(null);
    setMessage(null);

    try {
      const response = await performGroupAction(groupKey, action);
      if (!response.success) {
        throw new Error(actionFailureMessage(response, `Failed to ${action} ${groupKey}`));
      }
      setMessage(actionSuccessMessage(response, `Requested ${action} for ${groupKey}`));
      await loadJobs({ silent: true });
    } catch (err) {
      setError(err instanceof Error ? err.message : `Failed to ${action} ${groupKey}`);
    } finally {
      const next = new Set(busyGroupKeysRef.current);
      next.delete(groupKey);
      busyGroupKeysRef.current = next;
      setBusyGroupKeys(next);
    }
  }

  useEffect(() => {
    void loadJobs();
  }, []);

  useEffect(() => {
    if (!autoRefresh) {
      if (refreshTimer.current !== null) {
        window.clearInterval(refreshTimer.current);
        refreshTimer.current = null;
      }
      return;
    }

    refreshTimer.current = window.setInterval(() => {
      void loadJobs({ silent: true });
    }, REFRESH_INTERVAL_MS);

    return () => {
      if (refreshTimer.current !== null) {
        window.clearInterval(refreshTimer.current);
        refreshTimer.current = null;
      }
    };
  }, [autoRefresh]);

  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
      return;
    }

    const mediaQuery = window.matchMedia("(max-width: 760px)");
    setIsCompactLayout(mediaQuery.matches);

    const handleChange = (event: MediaQueryListEvent) => {
      setIsCompactLayout(event.matches);
    };

    if (typeof mediaQuery.addEventListener === "function") {
      mediaQuery.addEventListener("change", handleChange);
      return () => mediaQuery.removeEventListener("change", handleChange);
    }

    mediaQuery.addListener(handleChange);
    return () => mediaQuery.removeListener(handleChange);
  }, []);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement) {
        return;
      }

      // Keep browser/OS shortcuts like Ctrl+A and Cmd+A intact.
      if (event.ctrlKey || event.metaKey || event.altKey) {
        return;
      }

      if (event.key === "r" || event.key === "R") {
        event.preventDefault();
        void loadJobs();
      }

      if (event.key === "a" || event.key === "A") {
        event.preventDefault();
        setAutoRefresh((value) => !value);
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  const visibleJobs = showOnlyErrors ? jobs.filter((job) => (job.errors?.count || 0) > 0) : jobs;
  const groupedJobs = groupJobs(visibleJobs);
  const stats = calculateStats(jobs);

  return (
    <div className="app-shell">
      <a href="#main" className="skip-link">
        Skip to content
      </a>
      <TopBar
        stats={stats}
        shellLinks={shellLinks}
        autoRefresh={autoRefresh}
        isRefreshing={loading}
        onRefresh={() => void loadJobs()}
        onToggleAutoRefresh={() => setAutoRefresh((value) => !value)}
      />

      <main id="main" className="content">
        <section className="toolbar">
          <div className="toolbar__title-block">
            <h2 className="toolbar__title">Process Jobs</h2>
            <p className="toolbar__subtitle">Grouped by deployment metadata from `/api/pulse/jobs`.</p>
          </div>

          <div className="toolbar__actions">
            <label className="toggle">
              <input
                type="checkbox"
                checked={showOnlyErrors}
                onChange={(event) => setShowOnlyErrors(event.target.checked)}
              />
              <span>Show only jobs with errors</span>
            </label>

            <div className="shortcut-hint">
              <Clock3 size={14} />
              <span>`R` refresh, `A` auto-refresh, `Esc` close logs</span>
            </div>
          </div>
        </section>

        {message ? (
          <div className="info-banner" role="status">
            {message}
          </div>
        ) : null}

        {error ? (
          <div className="error-banner" role="alert">
            <AlertTriangle size={15} />
            <span>{error}</span>
          </div>
        ) : null}

        {lastUpdated ? (
          <p className="last-updated">Last updated {new Date(lastUpdated).toLocaleTimeString()}</p>
        ) : null}

        <section className="table-card">
          {isCompactLayout ? (
            <div className="job-cards-shell">
              {groupedJobs.map((group) => (
                <JobCards
                  key={group.key}
                  groupKey={group.key}
                  groupLabel={group.label}
                  jobs={group.jobs}
                  busyJobIds={busyJobIds}
                  busy={busyGroupKeys.has(group.key)}
                  onAction={handleJobAction}
                  onGroupAction={handleGroupAction}
                  onViewLogs={(job) =>
                    setOpenLogs({
                      jobId: jobIdOf(job),
                      jobName: job.name,
                      jobCmd: job.cmd,
                      initialFilter: "ALL",
                    })
                  }
                  onViewError={(job) =>
                    setOpenLogs({
                      jobId: jobIdOf(job),
                      jobName: job.name,
                      jobCmd: job.cmd,
                      initialFilter: "ERROR",
                    })
                  }
                />
              ))}
            </div>
          ) : (
            <table className="jobs-table">
              <thead>
                <tr>
                  <th>Job</th>
                  <th>Status</th>
                  <th>PID</th>
                  <th>Memory</th>
                  <th>Uptime</th>
                  <th>Errors</th>
                  <th>Actions</th>
                </tr>
              </thead>
              <tbody>
                {groupedJobs.map((group) => (
                  <JobGroup
                    key={group.key}
                    groupKey={group.key}
                    groupLabel={group.label}
                    jobs={group.jobs}
                    busyJobIds={busyJobIds}
                    busy={busyGroupKeys.has(group.key)}
                    onAction={handleJobAction}
                    onGroupAction={handleGroupAction}
                    onViewLogs={(job) =>
                      setOpenLogs({
                        jobId: jobIdOf(job),
                        jobName: job.name,
                        jobCmd: job.cmd,
                        initialFilter: "ALL",
                      })
                    }
                    onViewError={(job) =>
                      setOpenLogs({
                        jobId: jobIdOf(job),
                        jobName: job.name,
                        jobCmd: job.cmd,
                        initialFilter: "ERROR",
                      })
                    }
                  />
                ))}
              </tbody>
            </table>
          )}

          {!loading && groupedJobs.length === 0 ? <div className="empty-state empty-state--panel">No jobs found.</div> : null}
          {loading && jobs.length === 0 ? <div className="empty-state empty-state--panel">Loading process jobs...</div> : null}
        </section>
      </main>

      {openLogs ? (
        <LogsModal
          jobId={openLogs.jobId}
          jobName={openLogs.jobName}
          jobCmd={openLogs.jobCmd}
          initialFilter={openLogs.initialFilter}
          onClose={() => setOpenLogs(null)}
        />
      ) : null}
    </div>
  );
}
