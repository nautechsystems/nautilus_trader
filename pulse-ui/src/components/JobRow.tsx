import { FileText, Play, RotateCw, Square } from "lucide-react";

import type { Job } from "../api";
import { statusTone } from "../theme";
import { StatusPill } from "./StatusPill";

interface JobRowProps {
  job: Job;
  busy: boolean;
  onAction: (jobId: string, action: "start" | "stop" | "restart") => void;
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

export function JobRow({ job, busy, onAction, onViewLogs, onViewError }: JobRowProps) {
  const jobId = job.id || job.name;
  const status = job.status || job.state || "inactive";
  const systemdStatus = livenessStatus(job);
  const canStart = systemdStatus === "inactive" || systemdStatus === "failed";
  const canStop = systemdStatus === "active";
  const canRestart = systemdStatus === "active";

  return (
    <tr className="job-row">
      <td>
        <div className="job-row__primary mono" title={job.description ?? undefined}>
          {job.name}
        </div>
      </td>
      <td>
        <StatusPill label={status.toUpperCase()} tone={statusTone(status)} />
      </td>
      <td className="mono">{job.pid ?? "—"}</td>
      <td className="mono">{job.memory ?? "—"}</td>
      <td className="mono">{job.uptime ?? "—"}</td>
      <td>
        {job.errors ? (
          <div className="error-stack">
            <StatusPill
              label={job.errors.count === 1 ? "1 error" : `${job.errors.count} errors`}
              tone={job.errors.count === 0 ? "success" : "danger"}
            />
            {job.errors.preview ? (
              <button
                type="button"
                className="job-row__secondary job-row__secondary-button"
                title={job.errors.preview}
                onClick={() => onViewError(job)}
                disabled={busy}
                aria-label={`View latest error ${job.name}`}
              >
                {job.errors.preview}
              </button>
            ) : null}
            {job.errors.last_seen ? (
              <span className="job-row__secondary">{formatTimestamp(job.errors.last_seen)}</span>
            ) : null}
          </div>
        ) : (
          <span className="job-row__secondary">—</span>
        )}
      </td>
      <td>
        <div className="actions">
          <button
            type="button"
            className="icon-button"
            onClick={() => onAction(jobId, "start")}
            disabled={!canStart || busy}
            aria-label={`Start ${job.name}`}
            title="Start"
          >
            <Play size={14} />
          </button>
          <button
            type="button"
            className="icon-button"
            onClick={() => onAction(jobId, "stop")}
            disabled={!canStop || busy}
            aria-label={`Stop ${job.name}`}
            title="Stop"
          >
            <Square size={14} />
          </button>
          <button
            type="button"
            className="icon-button"
            onClick={() => onAction(jobId, "restart")}
            disabled={!canRestart || busy}
            aria-label={`Restart ${job.name}`}
            title="Restart"
          >
            <RotateCw size={14} />
          </button>
          <button
            type="button"
            className="icon-button"
            onClick={() => onViewLogs(job)}
            disabled={busy}
            aria-label={`View Logs ${job.name}`}
            title="View Logs"
          >
            <FileText size={14} />
          </button>
        </div>
      </td>
    </tr>
  );
}
