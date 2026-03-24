import { FileText, Play, RotateCw, Square } from "lucide-react";

import type { Job } from "../api";
import { statusTone } from "../theme";
import { StatusPill } from "./StatusPill";

interface JobCardsProps {
  groupKey: string;
  groupLabel: string;
  jobs: Job[];
  busyJobIds: Set<string>;
  busy: boolean;
  onAction: (jobId: string, action: "start" | "stop" | "restart") => void;
  onGroupAction: (groupKey: string, action: "start" | "stop" | "restart") => void;
  onViewLogs: (job: Job) => void;
  onViewError: (job: Job) => void;
}

function formatTimestamp(timestamp: string | null | undefined): string {
  if (!timestamp) {
    return "";
  }

  const hasTimezone = /Z$|[+-]\d{2}:?\d{2}$/.test(timestamp);
  const parsed = new Date(hasTimezone ? timestamp : `${timestamp}Z`);
  if (Number.isNaN(parsed.getTime())) {
    return timestamp;
  }

  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(parsed);
}

function livenessStatus(job: Job): string {
  return job.systemd_status || job.status || job.state || "inactive";
}

export function JobCards({
  groupKey,
  groupLabel,
  jobs,
  busyJobIds,
  busy,
  onAction,
  onGroupAction,
  onViewLogs,
  onViewError,
}: JobCardsProps) {
  const activeCount = jobs.filter((job) => (job.status || job.state) === "active").length;
  const degradedCount = jobs.filter((job) => (job.status || job.state) === "degraded").length;
  const inactiveCount = jobs.filter((job) => (job.status || job.state) === "inactive").length;
  const failedCount = jobs.filter((job) => (job.status || job.state) === "failed").length;
  const hasRunning = jobs.some((job) => livenessStatus(job) === "active");
  const hasStartable = inactiveCount > 0 || failedCount > 0;

  return (
    <section className="job-cards-group">
      <div className="group-row__content job-cards-group__header">
        <div>
          <div className="group-row__label">{groupLabel}</div>
          <div className="group-row__summary">
            {jobs.length} jobs, {activeCount} active
            {degradedCount ? `, ${degradedCount} degraded` : ""}
            {failedCount ? `, ${failedCount} failed` : ""}
          </div>
        </div>

        <div className="group-row__actions">
          <button
            type="button"
            className="button"
            onClick={() => onGroupAction(groupKey, "start")}
            disabled={!hasStartable || busy}
            aria-label={`Start All ${groupLabel}`}
          >
            <Play size={14} />
            <span>Start All</span>
          </button>
          <button
            type="button"
            className="button"
            onClick={() => onGroupAction(groupKey, "stop")}
            disabled={!hasRunning || busy}
            aria-label={`Stop All ${groupLabel}`}
          >
            <Square size={14} />
            <span>Stop All</span>
          </button>
          <button
            type="button"
            className="button"
            onClick={() => onGroupAction(groupKey, "restart")}
            disabled={!hasRunning || busy}
            aria-label={`Restart All ${groupLabel}`}
          >
            <RotateCw size={14} />
            <span>Restart All</span>
          </button>
        </div>
      </div>

      <div className="job-cards" role="list" aria-label={`${groupLabel} jobs`}>
        {jobs.map((job) => {
          const jobId = job.id || job.name;
          const jobBusy = busy || busyJobIds.has(jobId);
          const status = job.status || job.state || "inactive";
          const systemdStatus = livenessStatus(job);
          const canStart = systemdStatus === "inactive" || systemdStatus === "failed";
          const canStop = systemdStatus === "active";
          const canRestart = systemdStatus === "active";

          return (
            <article key={jobId} className="job-card" role="listitem">
              <div className="job-card__header">
                <div className="job-row__primary mono" title={job.description ?? undefined}>
                  {job.name}
                </div>
                <StatusPill label={status.toUpperCase()} tone={statusTone(status)} />
              </div>

              <dl className="job-card__meta">
                <div>
                  <dt>PID</dt>
                  <dd className="mono">{job.pid ?? "—"}</dd>
                </div>
                <div>
                  <dt>Memory</dt>
                  <dd className="mono">{job.memory ?? "—"}</dd>
                </div>
                <div>
                  <dt>Uptime</dt>
                  <dd className="mono">{job.uptime ?? "—"}</dd>
                </div>
                <div>
                  <dt>Errors</dt>
                  <dd>
                    {job.errors ? (
                      <StatusPill
                        label={job.errors.count === 1 ? "1 error" : `${job.errors.count} errors`}
                        tone={job.errors.count === 0 ? "success" : "danger"}
                      />
                    ) : (
                      "—"
                    )}
                  </dd>
                </div>
              </dl>

              {job.errors?.preview ? (
                <button
                  type="button"
                  className="job-row__secondary job-row__secondary-button"
                  title={job.errors.preview}
                  onClick={() => onViewError(job)}
                  disabled={jobBusy}
                  aria-label={`View latest error ${job.name}`}
                >
                  {job.errors.preview}
                </button>
              ) : null}

              {job.errors?.last_seen ? (
                <div className="job-row__secondary">{formatTimestamp(job.errors.last_seen)}</div>
              ) : null}

              <div className="actions job-card__actions">
                <button
                  type="button"
                  className="icon-button"
                  onClick={() => onAction(jobId, "start")}
                  disabled={!canStart || jobBusy}
                  aria-label={`Start ${job.name}`}
                  title="Start"
                >
                  <Play size={14} />
                </button>
                <button
                  type="button"
                  className="icon-button"
                  onClick={() => onAction(jobId, "stop")}
                  disabled={!canStop || jobBusy}
                  aria-label={`Stop ${job.name}`}
                  title="Stop"
                >
                  <Square size={14} />
                </button>
                <button
                  type="button"
                  className="icon-button"
                  onClick={() => onAction(jobId, "restart")}
                  disabled={!canRestart || jobBusy}
                  aria-label={`Restart ${job.name}`}
                  title="Restart"
                >
                  <RotateCw size={14} />
                </button>
                <button
                  type="button"
                  className="icon-button"
                  onClick={() => onViewLogs(job)}
                  disabled={jobBusy}
                  aria-label={`View Logs ${job.name}`}
                  title="View Logs"
                >
                  <FileText size={14} />
                </button>
              </div>
            </article>
          );
        })}
      </div>
    </section>
  );
}
