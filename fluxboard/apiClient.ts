// Enhanced API client with timeout, retry, and request deduplication

import { API } from './constants';

export class APIError extends Error {
  constructor(
    public status: number,
    public statusText: string,
    message?: string
  ) {
    super(message || `${status} ${statusText}`);
    this.name = 'APIError';
  }
}

export interface RequestOptions extends RequestInit {
  timeout?: number;
  retries?: number;
  dedupe?: boolean;
}

/**
 * Enhanced API client with:
 * - Automatic timeout handling (default 30s)
 * - Exponential backoff retry for transient failures (3 retries by default)
 * - Request deduplication to prevent duplicate in-flight requests
 * - Proper error handling with APIError
 */
export class APIClient {
  private baseURL: string;
  private pendingRequests = new Map<string, Promise<unknown>>();

  constructor(baseURL: string = '') {
    this.baseURL = baseURL;
  }

  /**
   * Fetch JSON with timeout, retry, and deduplication
   */
  async fetchJSON<T>(
    path: string,
    options: RequestOptions = {}
  ): Promise<T> {
    const isTestEnv = typeof process !== 'undefined' && process.env?.NODE_ENV === 'test';
    const defaultRetries = isTestEnv ? 0 : API.RETRY_ATTEMPTS;

    const {
      timeout = API.REQUEST_TIMEOUT,
      retries = defaultRetries,
      dedupe = true,
      ...fetchOptions
    } = options;

    // Request deduplication - reuse pending request if exists
    const cacheKey = this.getCacheKey(path, fetchOptions);
    if (dedupe && this.pendingRequests.has(cacheKey)) {
      return this.pendingRequests.get(cacheKey) as Promise<T>;
    }

    const promise = this._fetchWithRetry<T>(path, fetchOptions, retries, timeout);

    if (dedupe) {
      this.pendingRequests.set(cacheKey, promise);
      promise.finally(() => {
        this.pendingRequests.delete(cacheKey);
      });
    }

    return promise;
  }

  /**
   * Internal fetch with retry logic and exponential backoff
   */
  private async _fetchWithRetry<T>(
    path: string,
    options: RequestInit,
    retries: number,
    timeout: number
  ): Promise<T> {
    let lastError: Error | null = null;

    for (let attempt = 0; attempt <= retries; attempt++) {
      try {
        const controller = new AbortController();
        const timeoutId = setTimeout(() => controller.abort(), timeout);

        try {
          const response = await fetch(`${this.baseURL}${path}`, {
            ...options,
            signal: controller.signal
          });

          clearTimeout(timeoutId);

          if (!response.ok) {
            // Don't retry on client errors (4xx), only server errors (5xx) and network issues
            if (response.status >= 400 && response.status < 500) {
              throw new APIError(response.status, response.statusText);
            }
            // Retry on 5xx errors
            throw new APIError(response.status, response.statusText);
          }

          return await response.json() as T;
        } catch (error) {
          clearTimeout(timeoutId);

          // Rethrow AbortError (timeout) as a more descriptive error
          if (error instanceof Error && error.name === 'AbortError') {
            throw new Error(`Request timeout after ${timeout}ms`);
          }

          throw error;
        }
      } catch (error) {
        lastError = error instanceof Error ? error : new Error(String(error));

        // Don't retry on client errors (4xx)
        if (error instanceof APIError && error.status >= 400 && error.status < 500) {
          throw error;
        }

        // Don't retry if this was the last attempt
        if (attempt === retries) {
          throw lastError;
        }

        // Exponential backoff: 1s, 2s, 4s
        const delayMs = API.RETRY_DELAY * Math.pow(2, attempt);
        await this._delay(delayMs);
      }
    }

    // Should never reach here, but TypeScript needs this
    throw lastError || new Error('Unexpected error in _fetchWithRetry');
  }

  /**
   * Generate cache key for request deduplication
   */
  private getCacheKey(path: string, options: RequestInit): string {
    const method = options.method || 'GET';
    const body = options.body ? String(options.body) : '';
    return `${method}:${path}:${body}`;
  }

  /**
   * Delay helper for retry backoff
   */
  private _delay(ms: number): Promise<void> {
    return new Promise(resolve => setTimeout(resolve, ms));
  }

  /**
   * Clear all pending requests (useful for cleanup)
   */
  clearPendingRequests(): void {
    this.pendingRequests.clear();
  }

  /**
   * Get count of pending requests (useful for debugging)
   */
  getPendingRequestCount(): number {
    return this.pendingRequests.size;
  }
}
