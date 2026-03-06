import '@testing-library/jest-dom';

// Let React know we're running inside a test environment so act() warnings
// are suppressed when upstream components (Radix) schedule updates internally.
// See https://react.dev/blog/2022/03/29/react-v18#breaking-changes for details.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
(globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;

// Silence socket warnings in tests (browser environments only)
if (typeof window !== 'undefined') {
  Object.defineProperty(window, 'location', {
    value: { protocol: 'http:', hostname: 'localhost' }
  });
}

// Mock ResizeObserver for Radix UI components
if (typeof window !== 'undefined') {
  global.ResizeObserver = class ResizeObserver {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
}

// Polyfill requestAnimationFrame for scroll preservation tests
if (typeof window !== 'undefined') {
  if (typeof window.requestAnimationFrame === 'undefined') {
    window.requestAnimationFrame = (callback: FrameRequestCallback): number => {
      return setTimeout(callback, 0) as unknown as number;
    };
  }

  if (typeof window.cancelAnimationFrame === 'undefined') {
    window.cancelAnimationFrame = (id: number): void => {
      clearTimeout(id);
    };
  }
}

// Polyfill setPointerCapture for jsdom (used by sonner toast and Radix UI)
if (typeof HTMLElement !== 'undefined') {
  if (typeof HTMLElement.prototype.setPointerCapture === 'undefined') {
    HTMLElement.prototype.setPointerCapture = function() {};
  }

  if (typeof HTMLElement.prototype.releasePointerCapture === 'undefined') {
    HTMLElement.prototype.releasePointerCapture = function() {};
  }

  if (typeof HTMLElement.prototype.hasPointerCapture === 'undefined') {
    HTMLElement.prototype.hasPointerCapture = function() { return false; };
  }
}

// Polyfill scrollIntoView for jsdom (used by Radix UI Select)
if (typeof HTMLElement !== 'undefined') {
  if (typeof HTMLElement.prototype.scrollIntoView === 'undefined') {
    HTMLElement.prototype.scrollIntoView = function() {};
  }
}

// Provide a hermetic fetch implementation for unit tests.
//
// Why: some components/libraries (Socket.IO polling, background refresh hooks) can call `fetch()`
// during tests. Forwarding to a real localhost backend makes the test suite flaky and can surface
// as unhandled rejections when no server is running.
//
// Tests which need to assert on network behavior should stub/replace `global.fetch` explicitly.
type FetchStubResponse = {
  ok: boolean;
  status: number;
  statusText: string;
  headers: Headers;
  json: () => Promise<unknown>;
};

function makeJsonResponse(payload: unknown): FetchStubResponse {
  return {
    ok: true,
    status: 200,
    statusText: 'OK',
    headers: new Headers({ 'content-type': 'application/json' }),
    json: async () => payload,
  };
}

function normalizeFetchUrl(input: RequestInfo | URL): string {
  if (typeof input === 'string') return input;
  if (input instanceof URL) return input.toString();
  const anyInput = input as any;
  if (anyInput && typeof anyInput.url === 'string') return anyInput.url;
  return String(input);
}

function buildDefaultFluxEnvelope(urlText: string): unknown {
  const absolute = urlText.startsWith('/') ? `http://localhost:5000${urlText}` : urlText;
  let pathname = '';
  try {
    pathname = new URL(absolute).pathname;
  } catch {
    pathname = urlText;
  }

  // Default to a generic FluxAPI envelope.
  const envelope: any = { ok: true, data: {} };

  if (pathname.startsWith('/api/v1/signals')) {
    envelope.data = {
      strategies: [],
      server_time: new Date().toISOString().slice(0, 19).replace('T', ' '),
      server_ts_ms: Date.now(),
      balance_summary: null,
    };
    return envelope;
  }

  if (pathname.startsWith('/api/v1/trades/delta')) {
    envelope.data = { rows: [], last_seq: 0, reset_required: false };
    return envelope;
  }

  if (pathname.startsWith('/api/v1/trades')) {
    envelope.data = {
      rows: [],
      total: 0,
      limit: 200,
      offset: 0,
      has_more: false,
      next_offset: null,
      next_cursor: null,
      sort: 'ts_desc',
    };
    return envelope;
  }

  if (pathname.startsWith('/api/v1/balances')) {
    envelope.data = {
      rows: [],
      total: 0,
      totals: { mv_raw: 0, mv_display: '$0.00' },
      generated_at: new Date().toISOString(),
      view: 'parents_only',
      risk_groups: [],
    };
    return envelope;
  }

  if (pathname.startsWith('/api/v1/alerts')) {
    envelope.data = {
      rows: [],
      total: 0,
      limit: 25,
      offset: 0,
      has_more: false,
      next_offset: null,
      next_cursor: null,
    };
    return envelope;
  }

  if (pathname.startsWith('/api/v1/param-schema')) {
    envelope.data = { params: {}, deprecated: {} };
    return envelope;
  }

  if (pathname.startsWith('/api/v1/params')) {
    envelope.data = [];
    return envelope;
  }

  if (pathname.startsWith('/api/v1/strategies')) {
    envelope.data = { strategies: [], rows: [], count: 0 };
    return envelope;
  }

  return envelope;
}

global.fetch = function fetchStub(input: RequestInfo | URL, _init?: RequestInit) {
  const urlText = normalizeFetchUrl(input);
  return Promise.resolve(makeJsonResponse(buildDefaultFluxEnvelope(urlText))) as unknown as ReturnType<typeof fetch>;
} as typeof fetch;
