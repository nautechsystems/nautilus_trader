// Error boundary component for catching React errors

import { Component, ReactNode, ErrorInfo } from 'react';

interface Props {
  children: ReactNode;
  fallback?: (error: Error, reset: () => void) => ReactNode;
  onError?: (error: Error, errorInfo: ErrorInfo) => void;
  context?: string;
}

interface State {
  hasError: boolean;
  error: Error | null;
  errorInfo: ErrorInfo | null;
}

export class ErrorBoundary extends Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { hasError: false, error: null, errorInfo: null };
  }

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error, errorInfo: ErrorInfo) {
    this.setState({ errorInfo });

    const context = this.props.context || 'Unknown';
    if (import.meta.env?.DEV) {
      console.error(`[ErrorBoundary:${context}] Caught error:`, error);
      console.error(`[ErrorBoundary:${context}] Component stack:`, errorInfo.componentStack);
    }

    // Call optional error callback
    if (this.props.onError) {
      this.props.onError(error, errorInfo);
    }

    // Only log to external service in production
    if (import.meta.env.PROD) {
      // TODO: Send to error tracking service
      // sendToErrorTracker(error, errorInfo, context);
    }
  }

  componentDidUpdate(prevProps: Props) {
    // Auto-reset if children change
    if (this.state.hasError && prevProps.children !== this.props.children) {
      this.setState({ hasError: false, error: null, errorInfo: null });
    }
  }

  reset = () => {
    this.setState({ hasError: false, error: null, errorInfo: null });
  };

  render() {
    if (this.state.hasError && this.state.error) {
      if (this.props.fallback) {
        return this.props.fallback(this.state.error, this.reset);
      }

      return (
        <DefaultErrorFallback
          error={this.state.error}
          errorInfo={this.state.errorInfo}
          reset={this.reset}
          context={this.props.context}
        />
      );
    }

    return this.props.children;
  }
}

function DefaultErrorFallback({
  error,
  errorInfo,
  reset,
  context
}: {
  error: Error;
  errorInfo: ErrorInfo | null;
  reset: () => void;
  context?: string;
}) {
  const isDev = import.meta.env.DEV;
  const stackLines = error.stack?.split('\n') ?? [];
  const [, ...restStack] = stackLines;
  const formattedStack = restStack.length > 0 ? restStack.join('\n') : error.stack;

  return (
    <div className="flex items-center justify-center h-screen bg-neutral-950 text-neutral-100 p-8">
      <div className="max-w-2xl w-full">
        <div className="bg-neutral-900 border border-red-400/30 rounded-lg p-6">
          <h1 className="text-2xl font-bold text-red-400 mb-4 flex items-center gap-2">
            <span>⚠️</span>
            <span>Something went wrong</span>
            {context && <span className="text-sm text-neutral-400">({context})</span>}
          </h1>

          <div className="mb-6">
            <p className="text-neutral-300 mb-4">
              An unexpected error occurred. This has been logged and we'll look into it.
            </p>

            <div className="bg-neutral-950 rounded p-4 border border-neutral-700 mb-4">
              <p className="text-sm font-mono text-red-300">{error.message || 'Unknown error'}</p>
            </div>

            {isDev && (
              <details className="bg-neutral-950 rounded p-4 border border-neutral-700">
                <summary className="cursor-pointer text-sm text-neutral-400 hover:text-neutral-300 mb-2">
                  Stack Trace (Dev Only)
                </summary>
                <div className="mt-2">
                  <p className="text-xs font-mono text-red-300 mb-2">{error.name}: {error.message}</p>
                  {formattedStack && (
                    <pre className="text-xs text-neutral-500 overflow-auto max-h-64 whitespace-pre-wrap">
                      {formattedStack}
                    </pre>
                  )}
                  {errorInfo?.componentStack && (
                    <div className="mt-3">
                      <p className="text-xs text-neutral-400 mb-1">Component Stack:</p>
                      <pre className="text-xs text-neutral-600 overflow-auto max-h-32 whitespace-pre-wrap">
                        {errorInfo.componentStack}
                      </pre>
                    </div>
                  )}
                </div>
              </details>
            )}
          </div>

          <div className="flex gap-3">
            <button
              onClick={reset}
              className="px-4 py-2 bg-emerald-600 hover:bg-emerald-700 rounded text-sm font-medium transition-colors"
            >
              Try Again
            </button>
            <button
              onClick={() => window.location.href = '/'}
              className="px-4 py-2 bg-neutral-700 hover:bg-neutral-600 rounded text-sm font-medium transition-colors"
            >
              Go to Dashboard
            </button>
            <button
              onClick={() => window.location.reload()}
              className="px-4 py-2 bg-neutral-700 hover:bg-neutral-600 rounded text-sm font-medium transition-colors"
            >
              Reload Page
            </button>
          </div>
        </div>

        <p className="text-xs text-neutral-500 text-center mt-4">
          {isDev
            ? 'Check the browser console for more details.'
            : 'If this problem persists, please contact support.'
          }
        </p>
      </div>
    </div>
  );
}

// Context-specific error boundary wrappers

/**
 * AppErrorBoundary - Wraps the entire application
 * Provides full-screen error UI with navigation options
 */
export function AppErrorBoundary({ children }: { children: ReactNode }) {
  return (
    <ErrorBoundary
      context="App"
      onError={(error, errorInfo) => {
        console.error('[AppErrorBoundary] Critical app error:', error);
        // Could send to error tracking service here
      }}
    >
      {children}
    </ErrorBoundary>
  );
}

/**
 * PanelErrorBoundary - Wraps individual dashboard panels
 * Provides inline error UI that doesn't break the entire dashboard
 */
export function PanelErrorBoundary({
  children,
  panelName
}: {
  children: ReactNode;
  panelName: string;
}) {
  return (
    <ErrorBoundary
      context={`Panel:${panelName}`}
      fallback={(error, resetError) => (
        <div className="p-4 bg-red-900/20 border border-red-500 rounded-lg">
          <div className="flex items-center justify-between mb-3">
            <h3 className="text-red-400 font-semibold flex items-center gap-2">
              <span>⚠️</span>
              <span>{panelName} Error</span>
            </h3>
            <button
              onClick={resetError}
              className="px-3 py-1 text-xs bg-red-600 hover:bg-red-700 rounded transition-colors"
            >
              Retry
            </button>
          </div>

          <p className="text-sm text-neutral-300 mb-2">
            {error.message || 'An unexpected error occurred in this panel'}
          </p>

          {import.meta.env.DEV && error.stack && (
            <details className="mt-3">
              <summary className="text-xs text-neutral-400 cursor-pointer hover:text-neutral-300">
                Stack Trace (Dev Only)
              </summary>
              <pre className="text-xs text-neutral-500 mt-2 overflow-auto max-h-48 whitespace-pre-wrap bg-neutral-950 p-2 rounded border border-neutral-700">
                {error.stack}
              </pre>
            </details>
          )}

          <p className="text-xs text-neutral-500 mt-3">
            {import.meta.env.DEV
              ? 'Check the browser console for more details.'
              : 'Try refreshing the panel or the entire page.'
            }
          </p>
        </div>
      )}
    >
      {children}
    </ErrorBoundary>
  );
}

export default ErrorBoundary;
