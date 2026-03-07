import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import App from "./App";

const jobsPayload = {
  jobs: [
    {
      id: "tokenmm-api",
      name: "tokenmm-api",
      status: "active",
      pid: 1201,
      memory: "48.2M",
      uptime: "15min",
      group_key: "tokenmm",
      group_label: "TokenMM",
      group_order: 10,
      description: "TokenMM API",
      cmd: "python -m flux.runners.tokenmm.run_api",
      errors: { count: 0, last_seen: null, preview: null },
    },
    {
      id: "tokenmm-bridge",
      name: "tokenmm-bridge",
      status: "failed",
      pid: null,
      memory: null,
      uptime: null,
      group_key: "tokenmm",
      group_label: "TokenMM",
      group_order: 10,
      description: "TokenMM Bridge",
      cmd: "python -m flux.runners.tokenmm.run_bridge",
      errors: { count: 1, last_seen: null, preview: "ERROR something bad" },
    },
  ],
  total: 2,
  active: 1,
  failed: 1,
};

describe("App", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
  });

  it("renders a Fluxboard-style shell with Pulse active and TokenMM cross-links", async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.endsWith("/api/pulse/jobs")) {
        return new Response(JSON.stringify(jobsPayload), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      return new Response(null, { status: 404 });
    });

    vi.stubGlobal("fetch", fetchMock);

    render(<App />);

    expect(await screen.findByText("flux")).toBeInTheDocument();
    expect(screen.queryByText("Flux deployment control in the same operator shell as Fluxboard.")).not.toBeInTheDocument();

    const pulseLink = screen.getByRole("link", { name: "Pulse" });
    expect(pulseLink).toHaveAttribute("href", "/pulse/");
    expect(pulseLink).toHaveAttribute("aria-current", "page");

    expect(screen.getByRole("link", { name: "Dashboard" })).toHaveAttribute("href", "/tokenmm");
    expect(screen.getByRole("link", { name: "Signal" })).toHaveAttribute("href", "/tokenmm/signal");
    expect(screen.getByRole("link", { name: "Params" })).toHaveAttribute("href", "/tokenmm/params");
    expect(screen.getByRole("link", { name: "Balances" })).toHaveAttribute("href", "/tokenmm/balances");
    expect(screen.getByRole("link", { name: "Trades" })).toHaveAttribute("href", "/tokenmm/trades");
    expect(screen.getByRole("link", { name: "Alerts" })).toHaveAttribute("href", "/tokenmm/alerts");
  });

  it("builds pulse nav links from configured base path", async () => {
    vi.stubEnv("VITE_PULSE_UI_BASE_PATH", "/ops/pulse/");

    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.endsWith("/api/pulse/jobs")) {
        return new Response(JSON.stringify(jobsPayload), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      return new Response(null, { status: 404 });
    });

    vi.stubGlobal("fetch", fetchMock);

    render(<App />);

    expect(await screen.findByText("flux")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Pulse" })).toHaveAttribute("href", "/ops/pulse/");
    expect(screen.getByRole("link", { name: "Dashboard" })).toHaveAttribute("href", "/ops/tokenmm");
    expect(screen.getByRole("link", { name: "Signal" })).toHaveAttribute("href", "/ops/tokenmm/signal");
    expect(screen.getByRole("link", { name: "Params" })).toHaveAttribute("href", "/ops/tokenmm/params");
    expect(screen.getByRole("link", { name: "Balances" })).toHaveAttribute("href", "/ops/tokenmm/balances");
    expect(screen.getByRole("link", { name: "Trades" })).toHaveAttribute("href", "/ops/tokenmm/trades");
    expect(screen.getByRole("link", { name: "Alerts" })).toHaveAttribute("href", "/ops/tokenmm/alerts");
  });

  it("loads process jobs, renders a grouped table, and exposes logs/actions", async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.endsWith("/api/pulse/jobs")) {
        return new Response(JSON.stringify(jobsPayload), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.includes("/api/pulse/jobs/tokenmm-api/logs")) {
        return new Response("line 1\nline 2", { status: 200 });
      }
      return new Response(null, { status: 404 });
    });

    vi.stubGlobal("fetch", fetchMock);

    render(<App />);

    expect(await screen.findByRole("heading", { name: "Pulse" })).toBeInTheDocument();
    expect(await screen.findByText("TokenMM")).toBeInTheDocument();
    expect(await screen.findByText("tokenmm-api")).toBeInTheDocument();
    expect(await screen.findByText("tokenmm-bridge")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /view logs/i })).toHaveLength(2);
    expect(screen.getByRole("button", { name: /restart all/i })).toBeInTheDocument();

    await userEvent.click(screen.getAllByRole("button", { name: /view logs/i })[0]);

    const logsDialog = await screen.findByRole("dialog", { name: /logs for tokenmm-api/i });

    expect(logsDialog).toBeInTheDocument();
    expect(within(logsDialog).getByText("Job")).toBeInTheDocument();
    expect(within(logsDialog).getByText("Command")).toBeInTheDocument();
    expect(within(logsDialog).getByText("Showing last 300 lines")).toBeInTheDocument();
    expect(within(logsDialog).getByLabelText("Log output")).toBeInTheDocument();
    expect(await within(logsDialog).findByText(/line 1/)).toBeInTheDocument();

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith("/api/pulse/jobs", expect.any(Object));
    });
  });

  it("surfaces backend group-action failures instead of treating them as success", async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
      const url = String(input);
      if (url.endsWith("/api/pulse/jobs")) {
        return new Response(JSON.stringify(jobsPayload), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.endsWith("/api/pulse/jobs/group/tokenmm/restart")) {
        expect(init?.method).toBe("POST");
        return new Response(
          JSON.stringify({
            success: false,
            message: "restarted 0 jobs in group 'tokenmm'",
            errors: [
              "tokenmm-bridge: sudo: The \"no new privileges\" flag is set.",
              "tokenmm-node-a: sudo: The \"no new privileges\" flag is set.",
            ],
          }),
          {
            status: 207,
            headers: { "Content-Type": "application/json" },
          },
        );
      }
      return new Response(null, { status: 404 });
    });

    vi.stubGlobal("fetch", fetchMock);

    render(<App />);

    await screen.findByText("TokenMM");
    await userEvent.click(screen.getByRole("button", { name: /restart all tokenmm/i }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "restarted 0 jobs in group 'tokenmm': tokenmm-bridge: sudo: The \"no new privileges\" flag is set. (+1 more)",
    );
  });

  it("does not treat Ctrl+A as the auto-refresh shortcut", async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.endsWith("/api/pulse/jobs")) {
        return new Response(JSON.stringify(jobsPayload), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      return new Response(null, { status: 404 });
    });

    vi.stubGlobal("fetch", fetchMock);

    render(<App />);

    await screen.findByText("TokenMM");

    const autoRefreshToggle = screen.getByLabelText(/auto-refresh/i);
    expect(autoRefreshToggle).toBeChecked();

    const keydown = new KeyboardEvent("keydown", {
      key: "a",
      ctrlKey: true,
      bubbles: true,
      cancelable: true,
    });

    window.dispatchEvent(keydown);

    expect(autoRefreshToggle).toBeChecked();
    expect(keydown.defaultPrevented).toBe(false);
  });
});
