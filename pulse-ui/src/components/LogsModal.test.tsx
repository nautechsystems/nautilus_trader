import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { getJobLogs } from "../api";
import { LogsModal } from "./LogsModal";

vi.mock("../api", () => ({
  getJobLogs: vi.fn(),
}));

const sampleLogs = [
  "2026-03-06T19:20:48+00:00 host flux-tokenmm-api[237388]: INFO healthy",
  "2026-03-06T19:20:49+00:00 host flux-tokenmm-api[237388]: ERROR first failure",
  "2026-03-06T19:20:50+00:00 host flux-tokenmm-api[237388]: WARNING retrying",
  "2026-03-06T19:20:51+00:00 host flux-tokenmm-api[237388]: CRITICAL latest failure",
].join("\n");

describe("LogsModal", () => {
  const mockedGetJobLogs = vi.mocked(getJobLogs);
  const scrollIntoView = vi.fn();

  beforeEach(() => {
    mockedGetJobLogs.mockResolvedValue(sampleLogs);
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: scrollIntoView,
    });
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("renders severity filters and narrows visible log lines", async () => {
    render(<LogsModal jobId="tokenmm-api" jobName="tokenmm-api" onClose={vi.fn()} />);

    expect(await screen.findByText(/INFO healthy/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Error" }));

    expect(screen.getByText("Showing 2 of 4 lines")).toBeInTheDocument();
    expect(screen.getByText(/ERROR first failure/)).toBeInTheDocument();
    expect(screen.getByText(/CRITICAL latest failure/)).toBeInTheDocument();
    expect(screen.queryByText(/INFO healthy/)).not.toBeInTheDocument();
    expect(screen.queryByText(/WARNING retrying/)).not.toBeInTheDocument();
  });

  it("refetches logs when the selected line window changes", async () => {
    render(<LogsModal jobId="tokenmm-api" jobName="tokenmm-api" onClose={vi.fn()} />);

    await screen.findByText(/INFO healthy/);
    await userEvent.selectOptions(screen.getByLabelText("Line window"), "1000");

    await waitFor(() => {
      expect(mockedGetJobLogs).toHaveBeenLastCalledWith("tokenmm-api", 1000);
    });
  });

  it("keeps a user-selected severity filter after a refetch changes the visible lines", async () => {
    mockedGetJobLogs
      .mockResolvedValueOnce(sampleLogs)
      .mockResolvedValueOnce("2026-03-06T19:21:00+00:00 host flux-tokenmm-api[237388]: INFO only");

    render(<LogsModal jobId="tokenmm-api" jobName="tokenmm-api" onClose={vi.fn()} />);

    await screen.findByText(/INFO healthy/);
    await userEvent.click(screen.getByRole("button", { name: "Error" }));
    await userEvent.selectOptions(screen.getByLabelText("Line window"), "1000");

    await waitFor(() => {
      expect(mockedGetJobLogs).toHaveBeenLastCalledWith("tokenmm-api", 1000);
      expect(screen.getByRole("button", { name: "Error" })).toHaveClass("button--active");
      expect(screen.getByText("Showing 0 of 1 lines")).toBeInTheDocument();
    });

    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  it("opens in error-focused mode and targets the latest matching line", async () => {
    render(<LogsModal {...({ jobId: "tokenmm-api", jobName: "tokenmm-api", onClose: vi.fn(), initialFilter: "ERROR" } as any)} />);

    const latestError = await screen.findByText(/CRITICAL latest failure/);

    expect(latestError).toHaveAttribute("data-targeted", "true");
    await waitFor(() => {
      expect(scrollIntoView).toHaveBeenCalled();
    });
  });
});
