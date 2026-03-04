/**
 * AlertDetails Component
 *
 * Modal dialog for displaying full alert JSON details with context expansion.
 * Uses Dialog component from UI library with JSON formatting.
 */

import { useMemo, useCallback } from 'react';
import { Dialog } from '@/components/ui/dialog/Dialog';
import type { Alert } from '@/types';
import { txExplorerUrl } from '@/utils';

export interface AlertDetailsProps {
  /**
   * Alert to display
   */
  alert: Alert | null;

  /**
   * Whether dialog is open
   */
  isOpen: boolean;

  /**
   * Close handler
   */
  onClose: () => void;
}

/**
 * Stable JSON stringify with sorted keys to avoid key reordering flicker
 * Includes size limit to prevent browser freezes on large objects
 */
function stableStringify(value: unknown, space = 2): string {
  const seen = new WeakSet<object>();
  const MAX_LENGTH = 50000; // Limit to ~50KB of JSON

  const helper = (val: any): any => {
    if (val === null || val === undefined) return val;
    if (typeof val !== 'object') return val;

    if (seen.has(val)) return '[Circular]';
    seen.add(val);

    if (Array.isArray(val)) {
      return val.map((v) => helper(v));
    }

    const out: Record<string, any> = {};
    // Sort keys for deterministic ordering
    for (const key of Object.keys(val).sort()) {
      out[key] = helper(val[key]);
    }
    return out;
  };

  try {
    const result = JSON.stringify(helper(value), null, space);
    // Truncate if too large to prevent browser freeze
    if (result.length > MAX_LENGTH) {
      return result.substring(0, MAX_LENGTH) + '\n\n... [Truncated - JSON too large]';
    }
    return result;
  } catch (_err) {
    // Fallback to default stringify on unexpected errors
    try {
      const fallback = JSON.stringify(value, null, space);
      if (fallback.length > MAX_LENGTH) {
        return fallback.substring(0, MAX_LENGTH) + '\n\n... [Truncated - JSON too large]';
      }
      return fallback;
    } catch {
      return String(value);
    }
  }
}

export function AlertDetails({ alert, isOpen, onClose }: AlertDetailsProps) {
  const alertJson = useMemo(() => {
    if (!alert) return undefined;
    return stableStringify(alert, 2);
  }, [alert]);

  if (!alert) return null;

  const context = alert.context as any;
  const details = alert.details as any;

  return (
    <Dialog isOpen={isOpen} onClose={onClose} title="Alert Details" size="lg" variant="sheet">
      <div className="space-y-4 min-h-0">
        {/* Context Section */}
        {context && (
          <div className="flex-shrink-0">
            <div className="text-sm font-semibold text-zinc-400 mb-2">Context</div>
            <div className="font-mono text-xs bg-zinc-950 p-3 rounded space-y-2 break-words border border-zinc-800">
              {context.error && (
                <div className="break-words">
                  <span className="text-red-400">Error:</span> {String(context.error)}
                </div>
              )}
              {context.error_type && (
                <div className="break-words">
                  <span className="text-zinc-500">Type:</span> {String(context.error_type)}
                </div>
              )}
              {typeof context.edge_bps === 'number' && Number.isFinite(context.edge_bps) && (
                <div>
                  <span className="text-zinc-500">Edge:</span> {context.edge_bps.toFixed(2)} bps
                </div>
              )}
              {(context.explorer_url || context.tx_hash) && (
                <div className="break-all">
                  <span className="text-zinc-500">TX Hash:</span>{' '}
                  <a
                    href={
                      typeof (context as any).explorer_url === 'string' &&
                      (context as any).explorer_url.startsWith('http')
                        ? (context as any).explorer_url
                        : txExplorerUrl(
                            (context as any).tx_hash,
                            (context as any)?.chain || (details as any)?.chain
                          )
                    }
                    target="_blank"
                    rel="noopener noreferrer"
                    className="text-blue-400 hover:text-blue-300 underline"
                    onClick={(e) => e.stopPropagation()}
                  >
                    {((context as any).tx_hash || '').substring(0, 10)}...
                    {((context as any).tx_hash || '').slice(-8)}
                  </a>
                </div>
              )}
              {typeof context.gas_used === 'number' && Number.isFinite(context.gas_used) && (
                <div>
                  <span className="text-zinc-500">Gas Used:</span> {context.gas_used.toLocaleString()}
                </div>
              )}
              {context.receipt_status !== undefined && (
                <div>
                  <span className="text-zinc-500">Receipt:</span>{' '}
                  <span className={context.receipt_status === 1 ? 'text-emerald-400' : 'text-red-400'}>
                    {context.receipt_status === 1 ? 'Success' : 'Failed'}
                  </span>
                </div>
              )}
              {context.order_id && (
                <div className="break-words">
                  <span className="text-zinc-500">Order:</span> {String(context.order_id)}
                </div>
              )}
              {context.exchange_code && (
                <div className="break-words">
                  <span className="text-zinc-500">Code:</span> {String(context.exchange_code)}
                </div>
              )}
            </div>
          </div>
        )}

        {/* Full JSON Section */}
        <div className="flex-shrink-0">
          <div className="text-sm font-semibold text-zinc-400 mb-2">
            Full JSON
          </div>
          <div className="font-mono text-xs bg-zinc-950 p-3 rounded overflow-x-auto max-h-96 overflow-y-auto border border-zinc-800 text-zinc-300">
            <pre className="whitespace-pre-wrap break-words">{alertJson}</pre>
          </div>
        </div>
      </div>
    </Dialog>
  );
}
