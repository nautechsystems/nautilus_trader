import { render, screen } from "@testing-library/react";

import { JobRow } from "./JobRow";

describe("JobRow", () => {
  it("keeps stop and restart enabled for degraded jobs whose process is still active", () => {
    render(
      <table>
        <tbody>
          <JobRow
            job={{
              id: "tokenmm-api",
              name: "tokenmm-api",
              status: "degraded",
              systemd_status: "active",
              errors: { count: 0, last_seen: null, preview: null },
            }}
            busy={false}
            onAction={vi.fn()}
            onViewLogs={vi.fn()}
            onViewError={vi.fn()}
          />
        </tbody>
      </table>,
    );

    expect(screen.getByText("DEGRADED")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start tokenmm-api" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Stop tokenmm-api" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Restart tokenmm-api" })).toBeEnabled();
  });
});
