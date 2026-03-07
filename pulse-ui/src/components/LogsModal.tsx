import { RefreshCw, X } from "lucide-react";
import { useEffect, useState } from "react";

import { getJobLogs } from "../api";

interface LogsModalProps {
  jobId: string;
  jobName: string;
  jobCmd?: string | null;
  onClose: () => void;
}

export function LogsModal({ jobId, jobName, jobCmd, onClose }: LogsModalProps) {
  const [logs, setLogs] = useState("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const hasLogs = logs.trim().length > 0;

  async function loadLogs() {
    setLoading(true);
    setError(null);

    try {
      const nextLogs = await getJobLogs(jobId);
      setLogs(nextLogs);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to fetch logs");
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    void loadLogs();
  }, [jobId]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") {
        onClose();
      }
    }

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

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

          <section className="log-panel" aria-label="Log output">
            {loading && !hasLogs ? <p className="empty-state">Loading logs...</p> : null}
            {error ? <p className="error-banner">{error}</p> : null}
            {!loading && !error && !hasLogs ? <p className="empty-state">No logs available</p> : null}
            {hasLogs ? <pre className="log-output">{logs}</pre> : null}
          </section>
        </div>

        <div className="modal__footer">
          <span>Showing last 300 lines</span>
          <span>Press Esc to close</span>
        </div>
      </div>
    </div>
  );
}
