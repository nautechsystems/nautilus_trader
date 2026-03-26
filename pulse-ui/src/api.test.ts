import { calculateStats, groupJobs, type Job } from "./api";

describe("pulse api helpers", () => {
  it("groups jobs by backend metadata and sorts groups and jobs predictably", () => {
    const jobs: Job[] = [
      {
        id: "bridge",
        name: "bridge",
        status: "active",
        group_key: "services",
        group_label: "Services",
        group_order: 20,
      },
      {
        id: "node-b",
        name: "node-b",
        status: "failed",
        group_key: "tokenmm",
        group_label: "TokenMM",
        group_order: 10,
      },
      {
        id: "node-a",
        name: "node-a",
        status: "active",
        group_key: "tokenmm",
        group_label: "TokenMM",
        group_order: 10,
      },
    ];

    const groups = groupJobs(jobs);

    expect(groups.map((group) => group.key)).toEqual(["tokenmm", "services"]);
    expect(groups[0].jobs.map((job) => job.name)).toEqual(["node-a", "node-b"]);
  });

  it("calculates top-bar stats from process jobs", () => {
    const jobs: Job[] = [
      { id: "a", name: "a", status: "active", errors: { count: 0, last_seen: null, preview: null } },
      {
        id: "degraded",
        name: "degraded",
        status: "degraded",
        systemd_status: "active",
        errors: { count: 0, last_seen: null, preview: null },
      },
      { id: "b", name: "b", status: "failed", errors: { count: 2, last_seen: null, preview: "boom" } },
      { id: "c", name: "c", status: "inactive", errors: { count: 1, last_seen: null, preview: "oops" } },
    ];

    expect(calculateStats(jobs)).toEqual({
      total: 4,
      active: 1,
      degraded: 1,
      failed: 1,
      totalErrors: 3,
    });
  });
});
