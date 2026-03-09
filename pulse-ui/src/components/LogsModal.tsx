import { RefreshCw, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { getJobLogs } from "../api";
import { countLogLines, filterLogLines, parseLogLines, type LogFilter } from "../logs";

interface LogsModalProps {
  jobId: string;
  jobName: string;
  jobCmd?: string | null;
  initialFilter?: LogFilter;
  onClose: () => void;
}

const LINE_WINDOW_OPTIONS = [300, 1000] as const;
const FILTER_OPTIONS: Array<{ label: string; value: LogFilter }> = [
  { label: "All", value: "ALL" },
  { label: "Error", value: "ERROR" },
  { label: "Warning", value: "WARNING" },
  { label: "Info", value: "INFO" },
];

export function LogsModal({ jobId, jobName, jobCmd, initialFilter = "ALL", onClose }: LogsModalProps) {
  const [logs, setLogs] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [lineWindow, setLineWindow] = useState<number>(300);
  const [filter, setFilter] = useState<LogFilter>(initialFilter);
  const [autoFallbackNotice, setAutoFallbackNotice] = useState<string | null>(null);
  const initialFilterPending = useRef(initialFilter !== "ALL");
  const targetLineRef = useRef<HTMLDivElement | null>(null);
  const hasLogs = logs.trim().length > 0;
  const parsedLines = parseLogLines(logs);
  const visibleLines = filterLogLines(parsedLines, filter);
  const counts = countLogLines(parsedLines);
  const targetLine = filter === "ALL" ? null : visibleLines[visibleLines.length - 1] ?? null;

  async function loadLogs() {
    setLoading(true);
    setError(null);

    try {
      const nextLogs = await getJobLogs(jobId, lineWindow);
      setLogs(nextLogs);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to fetch logs");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    setFilter(initialFilter);
    setAutoFallbackNotice(null);
    initialFilterPending.current = initialFilter !== "ALL";
  }, [initialFilter, jobId]);

  useEffect(() => {
    void loadLogs();
  }, [jobId, lineWindow]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onClose();
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  useEffect(() => {
    if (!hasLogs || !initialFilterPending.current || filter === "ALL") {
      return;
    }

    initialFilterPending.current = false;
    if (visibleLines.length === 0) {
      setFilter("ALL");
      setAutoFallbackNotice(`No ${initialFilter.toLowerCase()} lines found in this window. Showing all lines.`);
    }
  }, [filter, hasLogs, initialFilter, visibleLines.length]);

  useEffect(() => {
    if (!targetLineRef.current || typeof targetLineRef.current.scrollIntoView !== "function") {
      return;
    }
    targetLineRef.current.scrollIntoView({ block: "nearest" });
  }, [targetLine?.id]);

  return (
    <div className="modal-backdrop" role="presentation" onClick={onClose}>
      <div
        className="modal"
        role="dialog"
        aria-modal="true"
        aria-label={`Logs for ${jobName}`}
        onClick={(event) => event.stopPropagation()}
      >
        <div className="modal__header">
          <div>
            <h2 className="modal__title">Logs</h2>
            <p className="modal__subtitle">Recent output for the selected Pulse job.</p>
          </div>

          <div className="modal__actions">
            <button type="button" className="button button--primary" onClick={() => void loadLogs()} disabled={loading}>
              <RefreshCw className={loading ? "spin" : ""} size={15} />
              <span>Refresh</span>
            </button>
            <button type="button" className="icon-button" onClick={onClose} aria-label="Close logs">
              <X size={15} />
            </button>
          </div>
        </div>

        <div className="modal__body">
          <div className="modal__meta" aria-label="Log details">
            <div className="modal__meta-row">
              <span className="modal__meta-label">Job</span>
              <span className="modal__meta-value mono">{jobName}</span>
            </div>
            <div className="modal__meta-row">
              <span className="modal__meta-label">Command</span>
              <span className="modal__meta-value mono">{jobCmd ?? "Unavailable"}</span>
            </div>
          </div>

          <div className="log-toolbar">
            <div className="log-toolbar__filters" role="group" aria-label="Severity filters">
              {FILTER_OPTIONS.map((option) => {
                const optionCount = option.value === "ALL" ? null : counts[option.value];
                return (
                  <button
                    key={option.value}
                    type="button"
                    aria-label={option.label}
                    className={filter === option.value ? "button button--active" : "button"}
                    onClick={() => {
                      setFilter(option.value);
                      setAutoFallbackNotice(null);
                      initialFilterPending.current = false;
                    }}
                  >
                    {option.label}
                    {optionCount !== null ? (
                      <span className="button__badge" aria-hidden="true">
                        {optionCount}
                      </span>
                    ) : null}
                  </button>
                );
              })}
            </div>

            <label className="log-toolbar__window">
              <span>Window</span>
              <select
                aria-label="Line window"
                value={lineWindow}
                onChange={(event) => {
                  setLineWindow(Number(event.target.value));
                  setAutoFallbackNotice(null);
                }}
              >
                {LINE_WINDOW_OPTIONS.map((option) => (
                  <option key={option} value={option}>
                    {option} lines
                  </option>
                ))}
              </select>
            </label>
          </div>

          <p className="log-toolbar__summary">Showing {visibleLines.length} of {parsedLines.length} lines</p>

          <section className="log-panel" aria-label="Log output">
            {loading && !hasLogs ? <p className="empty-state">Loading logs...</p> : null}
            {error ? <p className="error-banner">{error}</p> : null}
            {autoFallbackNotice ? (
              <p className="info-banner" role="status">
                {autoFallbackNotice}
              </p>
            ) : null}
            {!loading && !error && !hasLogs ? <p className="empty-state">No logs available</p> : null}
            {hasLogs ? (
              <div className="log-output">
                {visibleLines.map((line) => {
                  const isTargeted = targetLine?.id === line.id;
                  return (
                    <div
                      key={line.id}
                      ref={isTargeted ? targetLineRef : null}
                      className={`log-output__line log-output__line--${line.severity.toLowerCase()}${isTargeted ? " log-output__line--targeted" : ""}`}
                      data-targeted={isTargeted ? "true" : undefined}
                    >
                      {line.text}
                    </div>
                  );
                })}
              </div>
            ) : null}
          </section>
        </div>

        <div className="modal__footer">
          <span>Showing last {lineWindow} lines</span>
          <span>Press Esc to close</span>
        </div>
      </div>
    </div>
  );
}
