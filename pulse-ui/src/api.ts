export type JobStatus = "active" | "failed" | "inactive" | "restarting" | "stopping";

export interface ErrorInfo {
  count: number;
  last_seen: string | null;
  preview: string | null;
}

export interface Job {
  id?: string;
  name: string;
  status?: JobStatus;
  state?: JobStatus;
  pid?: number | string | null;
  memory?: string | null;
  uptime?: string | null;
  prefix?: string;
  group_key?: string;
  group_label?: string;
  group_order?: number;
  description?: string;
  cmd?: string | null;
  unit?: string;
  errors?: ErrorInfo;
}

export interface ShellLink {
  label: string;
  path: string;
  surface: string;
}

export interface JobsResponse {
  jobs: Job[];
  shell_links?: ShellLink[];
  total: number;
  active: number;
  failed: number;
}

export interface JobStats {
  total: number;
  active: number;
  failed: number;
  totalErrors: number;
  shellLinks: ShellLink[];
}

export interface JobGroup {
  key: string;
  label: string;
  order: number;
  jobs: Job[];
}

export interface ActionResponse {
  success: boolean;
  message?: string;
  pending?: boolean;
  deferred?: string[];
  errors?: string[];
}

const API_BASE = "/api/pulse";
let lastShellLinks: ShellLink[] = [];

async function fetchJSON<T>(url: string, options?: RequestInit): Promise<T> {
  const response = await fetch(url, {
    ...options,
    headers: {
      "Content-Type": "application/json",
      ...options?.headers,
    },
  });

  if (!response.ok) {
    let detail = `${response.status}: ${response.statusText}`;
    try {
      const payload = await response.json();
      if (payload?.error) {
        detail = payload.error;
      }
    } catch {
      // Keep the default message when the response is not JSON.
    }
    throw new Error(detail);
  }

  return response.json() as Promise<T>;
}

export async function getJobs(): Promise<JobsResponse> {
  const payload = await fetchJSON<JobsResponse>(`${API_BASE}/jobs`);
  lastShellLinks = payload.shell_links || [];
  return payload;
}

export async function performJobAction(jobId: string, action: "start" | "stop" | "restart"): Promise<ActionResponse> {
  return fetchJSON<ActionResponse>(`${API_BASE}/jobs/${jobId}/${action}`, {
    method: "POST",
  });
}

export async function performGroupAction(groupKey: string, action: "start" | "stop" | "restart"): Promise<ActionResponse> {
  return fetchJSON<ActionResponse>(`${API_BASE}/jobs/group/${groupKey}/${action}`, {
    method: "POST",
  });
}

export async function getJobLogs(jobId: string, lines = 300): Promise<string> {
  const response = await fetch(`${API_BASE}/jobs/${jobId}/logs?lines=${lines}`);
  if (!response.ok) {
    throw new Error(`Failed to fetch logs for ${jobId}`);
  }
  return response.text();
}

function defaultGroupOrder(groupKey: string): number {
  const defaultOrder: Record<string, number> = {
    tokenmm: 10,
    core: 20,
    services: 30,
    strategies: 40,
    monitoring: 50,
    other: 999,
  };

  return defaultOrder[groupKey] ?? 999;
}

export function groupJobs(jobs: Job[]): JobGroup[] {
  const groups = new Map<string, JobGroup>();

  for (const job of jobs) {
    const key = job.group_key || job.prefix || job.name.split("-", 1)[0] || "other";
    const label = job.group_label || key;
    const order = job.group_order ?? defaultGroupOrder(key);
    const existing = groups.get(key);

    if (existing) {
      existing.jobs.push(job);
      continue;
    }

    groups.set(key, {
      key,
      label,
      order,
      jobs: [job],
    });
  }

  return Array.from(groups.values())
    .map((group) => ({
      ...group,
      jobs: [...group.jobs].sort((left, right) => left.name.localeCompare(right.name)),
    }))
    .sort((left, right) => left.order - right.order || left.label.localeCompare(right.label));
}

export function calculateStats(jobs: Job[]): JobStats {
  return {
    total: jobs.length,
    active: jobs.filter((job) => (job.status || job.state) === "active").length,
    failed: jobs.filter((job) => (job.status || job.state) === "failed").length,
    totalErrors: jobs.reduce((sum, job) => sum + (job.errors?.count || 0), 0),
    shellLinks: lastShellLinks,
  };
}
