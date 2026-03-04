/**
 * ConfigViewer component - Display strategy configuration files.
 *
 * Features:
 * - Fetches and displays strategies.ini, relations.ini, catalog excerpts
 * - Syntax highlighting for INI format
 * - Copy to clipboard button
 * - Loading and error states
 * - ESC key to close
 * - Keyboard accessible
 */

import { useEffect, useState, useRef } from 'react';
import { api } from '../../api';
import type { ConfigResponse } from '../../types';
import { useCopyToClipboard } from '@/hooks/useCopyToClipboard';
import { useMobileLayout } from '@/hooks/useMobileLayout';

export type ConfigViewerProps = {
  strategyId: string;
  open: boolean;
  onClose: () => void;
};

export default function ConfigViewer({
  strategyId,
  open,
  onClose
}: ConfigViewerProps) {
  const [config, setConfig] = useState<ConfigResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const modalRef = useRef<HTMLDivElement>(null);
  const previousActiveElement = useRef<Element | null>(null);
  const copyToClipboard = useCopyToClipboard();
  const { isMobile } = useMobileLayout();

  // Fetch config when modal opens
  useEffect(() => {
    if (open) {
      previousActiveElement.current = document.activeElement;
      fetchConfig();
    } else {
      // Return focus when modal closes
      if (previousActiveElement.current instanceof HTMLElement) {
        previousActiveElement.current.focus();
      }
    }
  }, [open, strategyId]);

  const fetchConfig = async () => {
    setLoading(true);
    setError(null);
    try {
      const data = await api.getStrategyConfig(strategyId);
      setConfig(data);
    } catch (e) {
      const msg = e instanceof Error ? e.message : 'Failed to load config';
      setError(msg);
    } finally {
      setLoading(false);
    }
  };

  // Handle ESC key
  useEffect(() => {
    const handleEscape = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && open) {
        onClose();
      }
    };

    document.addEventListener('keydown', handleEscape);
    return () => document.removeEventListener('keydown', handleEscape);
  }, [open, onClose]);

  const handleBackdropClick = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) {
      onClose();
    }
  };

  const handleCopy = async () => {
    if (!config) return;

    const fullConfig = [
      '# strategies.ini',
      config.strategies_ini,
      '',
      '# relations.ini',
      config.relations_ini,
      '',
      '# catalog.ini excerpts',
      config.catalog_excerpts
    ].join('\n');

    const success = await copyToClipboard(fullConfig, {
      successMessage: 'Configuration copied to clipboard',
      errorMessage: 'Failed to copy configuration',
      showPreview: false,
    });

    if (success) {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    }
  };

  if (!open) return null;

  return (
    <div
      className={`fixed inset-0 z-50 bg-black bg-opacity-50 ${isMobile ? 'flex flex-col justify-end' : 'flex items-center justify-center'}`}
      onClick={handleBackdropClick}
      role="dialog"
      aria-modal="true"
      aria-labelledby="config-viewer-title"
    >
      <div
        ref={modalRef}
        className={`bg-neutral-900 border border-neutral-700 ${isMobile ? 'rounded-t-2xl' : 'rounded-lg'} shadow-xl max-w-4xl w-full mx-4 max-h-[90vh] flex flex-col`}
      >
        {/* Header */}
        <div className="flex items-center justify-between p-4 border-b border-neutral-700">
          <h2
            id="config-viewer-title"
            className="text-lg font-semibold text-neutral-100"
          >
            Config: {strategyId}
          </h2>
          <div className="flex items-center gap-2">
            <button
              onClick={handleCopy}
              disabled={!config}
              className="px-3 py-1 bg-neutral-700 hover:bg-neutral-600 disabled:opacity-50 disabled:cursor-not-allowed text-neutral-100 rounded text-sm"
              aria-label="Copy config to clipboard"
            >
              {copied ? '✓ Copied' : 'Copy'}
            </button>
            <button
              onClick={onClose}
              className="text-neutral-400 hover:text-neutral-200 text-2xl leading-none"
              aria-label="Close config viewer"
            >
              ×
            </button>
          </div>
        </div>

        {/* Content */}
        <div className="flex-1 overflow-auto p-4">
          {loading && (
            <div className="flex items-center justify-center py-12">
              <div className="w-8 h-8 border-4 border-neutral-600 border-t-neutral-300 rounded-full animate-spin" />
            </div>
          )}

          {error && (
            <div className="bg-red-900 bg-opacity-20 border border-red-700 rounded p-4">
              <p className="text-red-400">Error loading config: {error}</p>
              <button
                onClick={fetchConfig}
                className="mt-2 px-3 py-1 bg-red-700 hover:bg-red-600 text-white rounded text-sm"
              >
                Retry
              </button>
            </div>
          )}

          {config && !loading && (
            <div className="space-y-4">
              {/* strategies.ini section */}
              <ConfigSection
                title="strategies.ini"
                content={config.strategies_ini}
              />

              {/* relations.ini section */}
              <ConfigSection
                title="relations.ini"
                content={config.relations_ini}
              />

              {/* catalog.ini excerpts */}
              <ConfigSection
                title="catalog.ini (excerpts)"
                content={config.catalog_excerpts}
              />
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="flex justify-end gap-2 p-4 border-t border-neutral-700">
          <button
            onClick={onClose}
            className="px-4 py-2 bg-neutral-700 hover:bg-neutral-600 text-neutral-100 rounded text-sm"
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * ConfigSection - Displays a single config file section with syntax highlighting.
 */
function ConfigSection({ title, content }: { title: string; content: string }) {
  return (
    <div>
      <h3 className="text-sm font-medium text-neutral-300 mb-2">{title}</h3>
      <pre className="bg-neutral-950 border border-neutral-800 rounded p-3 text-xs text-neutral-300 overflow-x-auto font-mono">
        <code>{highlightINI(content)}</code>
      </pre>
    </div>
  );
}

/**
 * Simple syntax highlighting for INI format.
 */
function highlightINI(content: string): React.ReactNode {
  const lines = content.split('\n');

  return lines.map((line, idx) => {
    let className = '';
    let trimmedLine = line;

    // Section headers [section_name]
    if (line.trim().startsWith('[') && line.trim().endsWith(']')) {
      className = 'text-emerald-400 font-semibold';
    }
    // Comments starting with #
    else if (line.trim().startsWith('#')) {
      className = 'text-neutral-500 italic';
    }
    // Key = value pairs
    else if (line.includes('=')) {
      const [key, ...valueParts] = line.split('=');
      const value = valueParts.join('=');
      return (
        <div key={idx}>
          <span className="text-blue-400">{key}</span>
          <span className="text-neutral-500">=</span>
          <span className="text-amber-300">{value}</span>
        </div>
      );
    }

    return (
      <div key={idx} className={className}>
        {trimmedLine || '\u00A0'}
      </div>
    );
  });
}
