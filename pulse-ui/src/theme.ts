export const REFRESH_INTERVAL_MS = 15000;

export function statusTone(status: string | undefined): "success" | "danger" | "warning" | "info" | "muted" {
  switch (status) {
    case "active":
      return "success";
    case "degraded":
      return "warning";
    case "failed":
      return "danger";
    case "restarting":
      return "warning";
    case "stopping":
      return "info";
    default:
      return "muted";
  }
}
