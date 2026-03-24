import { render, screen } from "@testing-library/react";

import { JobGroup } from "./JobGroup";

describe("JobGroup", () => {
  it("surfaces degraded counts while keeping group restart controls available for running degraded jobs", () => {
    render(
      <table>
        <tbody>
          <JobGroup
            groupKey="tokenmm"
            groupLabel="TokenMM"
            jobs={[
              {
                id: "tokenmm-api",
                name: "tokenmm-api",
                status: "degraded",
                systemd_status: "active",
                errors: { count: 0, last_seen: null, preview: null },
              },
            ]}
            busyJobIds={new Set()}
            busy={false}
            onAction={vi.fn()}
            onGroupAction={vi.fn()}
            onViewLogs={vi.fn()}
            onViewError={vi.fn()}
          />
        </tbody>
      </table>,
    );

    expect(screen.getByText("1 jobs, 0 active, 1 degraded")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Restart All TokenMM" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Stop All TokenMM" })).toBeEnabled();
  });
});
