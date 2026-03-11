import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import App from "./App";

function shellLinks(...surfaces: Array<"tokenmm" | "equities">) {
  const suiteLabels = {
    tokenmm: "TokenMM",
    equities: "Equities",
  } as const;
  const entries = [
    ["Dashboard", ""],
    ["Signal", "signal"],
    ["Params", "params"],
    ["Balances", "balances"],
    ["Trades", "trades"],
    ["Alerts", "alerts"],
  ] as const;

  return surfaces.flatMap((surface) =>
    entries.map(([label, suffix]) => ({
      surface,
      label: `${suiteLabels[surface]} ${label}`,
      path: suffix ? `${surface}/${suffix}` : surface,
    })),
  );
}

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
      errors: { count: 1, last_seen: "2026-03-06T19:20:49+00:00", preview: "ERROR something bad" },
    },
  ],
  shell_links: shellLinks("tokenmm"),
  total: 2,
  active: 1,
  failed: 1,
};

describe("App", () => {
  const originalInnerWidth = window.innerWidth;
  const originalMatchMedia = window.matchMedia;

  function setViewportWidth(width: number) {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      writable: true,
      value: width,
    });
    window.dispatchEvent(new Event("resize"));
  }

  function stubResponsiveViewport(width: number) {
    setViewportWidth(width);
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      writable: true,
      value: vi.fn((query: string) => ({
        matches: query.includes("max-width") ? width <= 760 : false,
        media: query,
        onchange: null,
        addListener: vi.fn(),
        removeListener: vi.fn(),
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        dispatchEvent: vi.fn(),
      })),
    });
  }

  afterEach(() => {
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      writable: true,
      value: originalInnerWidth,
    });
    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      writable: true,
      value: originalMatchMedia,
    });
    window.dispatchEvent(new Event("resize"));
    vi.unstubAllEnvs();
    vi.unstubAllGlobals();
  });

  it("renders a stack switcher from backend-reported surfaces for TokenMM-only hosting", async () => {
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

    expect(await screen.findByRole("link", { name: "TokenMM" })).toHaveAttribute("href", "/tokenmm");
    expect(screen.queryByRole("link", { name: "TokenMM Dashboard" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "TokenMM Signal" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Equities" })).not.toBeInTheDocument();
  });

  it("renders only top-level stack links for multi-surface hosting", async () => {
    vi.stubEnv("VITE_PULSE_UI_BASE_PATH", "/ops/pulse/");
    const sharedHostPayload = {
      ...jobsPayload,
      shell_links: shellLinks("tokenmm", "equities"),
    };

    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.endsWith("/api/pulse/jobs")) {
        return new Response(JSON.stringify(sharedHostPayload), {
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
    expect(await screen.findByRole("link", { name: "TokenMM" })).toHaveAttribute("href", "/ops/tokenmm");
    expect(screen.getByRole("link", { name: "Equities" })).toHaveAttribute("href", "/ops/equities");
    expect(screen.queryByRole("link", { name: "TokenMM Alerts" })).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Equities Signal" })).not.toBeInTheDocument();
  });

  it("does not leak stale shell links across remount when the next jobs fetch fails", async () => {
    const fetchMock = vi
      .fn<(_: RequestInfo | URL) => Promise<Response>>()
      .mockResolvedValueOnce(
        new Response(JSON.stringify(jobsPayload), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        }),
      )
      .mockResolvedValueOnce(
        new Response(JSON.stringify({ error: "boom" }), {
          status: 500,
          headers: { "Content-Type": "application/json" },
        }),
      );

    vi.stubGlobal("fetch", fetchMock);

    const firstRender = render(<App />);

    expect(await screen.findByRole("link", { name: "TokenMM" })).toHaveAttribute("href", "/tokenmm");

    firstRender.unmount();
    render(<App />);

    expect(await screen.findByRole("alert")).toHaveTextContent("boom");
    expect(screen.queryByRole("link", { name: "TokenMM" })).not.toBeInTheDocument();
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
    expect(await screen.findByRole("button", { name: /restart all tokenmm/i })).toBeInTheDocument();
    const apiRow = await screen.findByText("tokenmm-api");
    const bridgeRow = await screen.findByText("tokenmm-bridge");
    expect(apiRow).toBeInTheDocument();
    expect(bridgeRow).toBeInTheDocument();
    expect(apiRow).toHaveAttribute("title", "TokenMM API");
    expect(bridgeRow).toHaveAttribute("title", "TokenMM Bridge");
    expect(screen.queryByText("TokenMM API")).not.toBeInTheDocument();
    expect(screen.queryByText("TokenMM Bridge")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /view latest error tokenmm-api/i })).not.toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: /view logs/i })).toHaveLength(2);
    expect(screen.getByRole("button", { name: /restart all/i })).toBeInTheDocument();

    await userEvent.click(screen.getAllByRole("button", { name: /view logs/i })[0]);

    const logsDialog = await screen.findByRole("dialog", { name: /logs for tokenmm-api/i });

    expect(logsDialog).toBeInTheDocument();
    expect(within(logsDialog).getByRole("button", { name: "All" })).toHaveClass("button--active");
    expect(within(logsDialog).getByText("Job")).toBeInTheDocument();
    expect(within(logsDialog).getByText("Command")).toBeInTheDocument();
    expect(within(logsDialog).getByText("Showing last 300 lines")).toBeInTheDocument();
    expect(within(logsDialog).getByLabelText("Log output")).toBeInTheDocument();
    expect(await within(logsDialog).findByText(/line 1/)).toBeInTheDocument();

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith("/api/pulse/jobs", expect.any(Object));
    });
  });

  it("renders a mobile jobs presentation when the responsive branch selects a narrow viewport", async () => {
    stubResponsiveViewport(390);

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

    expect(await screen.findByRole("link", { name: "TokenMM" })).toHaveAttribute("href", "/tokenmm");
    expect(screen.getByRole("link", { name: "Pulse" })).toHaveAttribute("href", "/pulse/");
    expect(await screen.findByRole("list", { name: "TokenMM jobs" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "View Logs tokenmm-api" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^Restart All TokenMM$/i })).toBeInTheDocument();
  });

  it("supports responsive matchMedia listeners on browsers that only expose addListener/removeListener", async () => {
    const listeners = new Set<(event: { matches: boolean }) => void>();

    Object.defineProperty(window, "matchMedia", {
      configurable: true,
      writable: true,
      value: vi.fn(() => ({
        matches: false,
        media: "(max-width: 760px)",
        onchange: null,
        addListener: vi.fn((listener: (event: { matches: boolean }) => void) => {
          listeners.add(listener);
        }),
        removeListener: vi.fn((listener: (event: { matches: boolean }) => void) => {
          listeners.delete(listener);
        }),
      })),
    });

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

    expect(await screen.findByText("tokenmm-api")).toBeInTheDocument();
    act(() => {
      listeners.forEach((listener) => listener({ matches: true }));
    });
    expect(await screen.findByRole("list", { name: "TokenMM jobs" })).toBeInTheDocument();
  });

  it("opens the error preview in error-focused mode and shows the error recency", async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.endsWith("/api/pulse/jobs")) {
        return new Response(JSON.stringify(jobsPayload), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.includes("/api/pulse/jobs/tokenmm-bridge/logs")) {
        return new Response(
          [
            "2026-03-06T19:20:48+00:00 host flux-tokenmm-bridge[237388]: INFO booting",
            "2026-03-06T19:20:49+00:00 host flux-tokenmm-bridge[237388]: ERROR first failure",
            "2026-03-06T19:20:50+00:00 host flux-tokenmm-bridge[237388]: WARNING retrying",
            "2026-03-06T19:20:51+00:00 host flux-tokenmm-bridge[237388]: CRITICAL latest failure",
          ].join("\n"),
          { status: 200 },
        );
      }
      return new Response(null, { status: 404 });
    });

    vi.stubGlobal("fetch", fetchMock);

    render(<App />);

    expect(await screen.findByText("tokenmm-bridge")).toBeInTheDocument();
    expect(screen.getByText(/Mar 06/i)).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: /view latest error tokenmm-bridge/i }));

    const logsDialog = await screen.findByRole("dialog", { name: /logs for tokenmm-bridge/i });

    expect(within(logsDialog).getByRole("button", { name: "Error" })).toHaveClass("button--active");
    expect(within(logsDialog).getByText("Showing 2 of 4 lines")).toBeInTheDocument();
    expect(within(logsDialog).getByText(/ERROR first failure/)).toBeInTheDocument();
    expect(within(logsDialog).getByText(/CRITICAL latest failure/)).toBeInTheDocument();
    expect(within(logsDialog).queryByText(/INFO booting/)).not.toBeInTheDocument();
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

    await screen.findByRole("button", { name: /^Restart All TokenMM$/i });
    await userEvent.click(screen.getByRole("button", { name: /^Restart All TokenMM$/i }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "restarted 0 jobs in group 'tokenmm': tokenmm-bridge: sudo: The \"no new privileges\" flag is set. (+1 more)",
    );
  });

  it("disables group action buttons while the request is in flight", async () => {
    let resolveGroupAction: ((value: Response | PromiseLike<Response>) => void) | undefined;
    const groupActionResponse = new Promise<Response>((resolve) => {
      resolveGroupAction = resolve;
    });

    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.endsWith("/api/pulse/jobs")) {
        return new Response(JSON.stringify(jobsPayload), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.endsWith("/api/pulse/jobs/group/tokenmm/restart")) {
        return groupActionResponse;
      }
      return new Response(null, { status: 404 });
    });

    vi.stubGlobal("fetch", fetchMock);

    render(<App />);

    await screen.findByRole("button", { name: /^Restart All TokenMM$/i });

    const startButton = screen.getByRole("button", { name: "Start All TokenMM" });
    const stopButton = screen.getByRole("button", { name: "Stop All TokenMM" });
    const restartButton = screen.getByRole("button", { name: "Restart All TokenMM" });

    expect(startButton).toBeEnabled();
    expect(stopButton).toBeEnabled();
    expect(restartButton).toBeEnabled();

    await userEvent.click(restartButton);

    await waitFor(() => {
      expect(startButton).toBeDisabled();
      expect(stopButton).toBeDisabled();
      expect(restartButton).toBeDisabled();
    });

    resolveGroupAction?.(
      new Response(
        JSON.stringify({
          success: true,
          message: "restarted 1 jobs in group 'tokenmm'",
        }),
        {
          status: 200,
          headers: { "Content-Type": "application/json" },
        },
      ),
    );

    expect(await screen.findByRole("status")).toHaveTextContent("restarted 1 jobs in group 'tokenmm'");
  });

  it("includes pending and deferred details in group-action success feedback", async () => {
    const fetchMock = vi.fn(async (input: RequestInfo | URL) => {
      const url = String(input);
      if (url.endsWith("/api/pulse/jobs")) {
        return new Response(JSON.stringify(jobsPayload), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      if (url.endsWith("/api/pulse/jobs/group/tokenmm/restart")) {
        return new Response(
          JSON.stringify({
            success: true,
            message: "restarted 1 jobs in group 'tokenmm'",
            pending: true,
            deferred: ["tokenmm-api"],
          }),
          {
            status: 202,
            headers: { "Content-Type": "application/json" },
          },
        );
      }
      return new Response(null, { status: 404 });
    });

    vi.stubGlobal("fetch", fetchMock);

    render(<App />);

    await screen.findByRole("button", { name: /^Restart All TokenMM$/i });
    await userEvent.click(screen.getByRole("button", { name: /restart all tokenmm/i }));

    const status = await screen.findByRole("status");
    expect(status).toHaveTextContent("restarted 1 jobs in group 'tokenmm'");
    expect(status).toHaveTextContent(/pending/i);
    expect(status).toHaveTextContent("tokenmm-api");
  });

  it("does not submit duplicate group actions while the first request is outstanding", async () => {
    let resolveGroupAction: ((value: Response | PromiseLike<Response>) => void) | undefined;
    const groupActionResponse = new Promise<Response>((resolve) => {
      resolveGroupAction = resolve;
    });

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
        return groupActionResponse;
      }
      return new Response(null, { status: 404 });
    });

    vi.stubGlobal("fetch", fetchMock);

    render(<App />);

    await screen.findByRole("button", { name: /^Restart All TokenMM$/i });

    const restartButton = screen.getByRole("button", { name: /^Restart All TokenMM$/i });
    await userEvent.click(restartButton);
    await userEvent.click(restartButton);

    expect(
      fetchMock.mock.calls.filter(([input]) => String(input).endsWith("/api/pulse/jobs/group/tokenmm/restart")),
    ).toHaveLength(1);

    resolveGroupAction?.(
      new Response(
        JSON.stringify({
          success: true,
          message: "restarted 1 jobs in group 'tokenmm'",
        }),
        {
          status: 200,
          headers: { "Content-Type": "application/json" },
        },
      ),
    );

    expect(await screen.findByRole("status")).toHaveTextContent("restarted 1 jobs in group 'tokenmm'");
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

    await screen.findByRole("button", { name: /^Restart All TokenMM$/i });

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
