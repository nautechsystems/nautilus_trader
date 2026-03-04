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

// Set default baseURL for fetch in tests to prevent "Invalid URL" errors
// This polyfill intercepts fetch calls and converts relative URLs to absolute
const originalFetch = global.fetch;
global.fetch = function(input: RequestInfo | URL, init?: RequestInit) {
  if (typeof input === 'string' && input.startsWith('/')) {
    // Convert relative URLs to absolute for tests
    input = `http://localhost:5000${input}`;
  }
  return originalFetch(input, init);
} as typeof fetch;
