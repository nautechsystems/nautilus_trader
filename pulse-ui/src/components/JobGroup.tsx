import { ChevronDown, ChevronRight, Play, RotateCw, Square } from "lucide-react";
import { useState } from "react";

import type { Job } from "../api";
import { JobRow } from "./JobRow";

function livenessStatus(job: Job): string {
  return job.systemd_status || job.status || job.state || "inactive";
}

interface JobGroupProps {
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

export function JobGroup({
  groupKey,
  groupLabel,
  jobs,
  busyJobIds,
  busy,
  onAction,
  onGroupAction,
  onViewLogs,
  onViewError,
}: JobGroupProps) {
  const [collapsed, setCollapsed] = useState(false);
  const activeCount = jobs.filter((job) => (job.status || job.state) === "active").length;
  const degradedCount = jobs.filter((job) => (job.status || job.state) === "degraded").length;
  const inactiveCount = jobs.filter((job) => (job.status || job.state) === "inactive").length;
  const failedCount = jobs.filter((job) => (job.status || job.state) === "failed").length;
  const hasRunning = jobs.some((job) => livenessStatus(job) === "active");
  const hasStartable = inactiveCount > 0 || failedCount > 0;

  return (
    <>
      <tr className="group-row">
        <td colSpan={7}>
          <div className="group-row__content">
            <button type="button" className="group-row__toggle" onClick={() => setCollapsed((value) => !value)}>
              {collapsed ? <ChevronRight size={16} /> : <ChevronDown size={16} />}
              <span className="group-row__label">{groupLabel}</span>
              <span className="group-row__summary">
                {jobs.length} jobs, {activeCount} active
                {degradedCount ? `, ${degradedCount} degraded` : ""}
                {failedCount ? `, ${failedCount} failed` : ""}
              </span>
            </button>

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
        </td>
      </tr>

      {collapsed
        ? null
        : jobs.map((job) => (
            <JobRow
              key={job.id || job.name}
              job={job}
              busy={busyJobIds.has(job.id || job.name)}
              onAction={onAction}
              onViewLogs={onViewLogs}
              onViewError={onViewError}
            />
          ))}
    </>
  );
}
