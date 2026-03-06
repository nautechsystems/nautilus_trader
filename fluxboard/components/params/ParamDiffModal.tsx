import { useEffect, useRef } from 'react';
import { useMobileLayout } from '@/hooks/useMobileLayout';
type DiffEntry = {
  key: string;
  mine: string;
  remote: string;
};

type ParamDiffModalProps = {
  open: boolean;
  strategyId: string | null;
  entries: DiffEntry[];
  onApplyRemote: () => void;
  onKeepMine: () => void;
  onClose: () => void;
};

export function ParamDiffModal({
  open,
  strategyId,
  entries,
  onApplyRemote,
  onKeepMine,
  onClose
}: ParamDiffModalProps) {
  const modalRef = useRef<HTMLDivElement>(null);
  const primaryButtonRef = useRef<HTMLButtonElement>(null);
  const previousFocus = useRef<Element | null>(null);
  const { isMobile } = useMobileLayout();

  useEffect(() => {
    if (open) {
      previousFocus.current = document.activeElement;
      const timer = window.setTimeout(() => {
        primaryButtonRef.current?.focus();
      }, 50);
      return () => window.clearTimeout(timer);
    }

    if (!open && previousFocus.current instanceof HTMLElement) {
      previousFocus.current.focus();
    }
    return undefined;
  }, [open]);

  useEffect(() => {
    if (!open) return undefined;

    const handleKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') {
        onClose();
      }
    };
    document.addEventListener('keydown', handleKey);
    return () => document.removeEventListener('keydown', handleKey);
  }, [open, onClose]);

  useEffect(() => {
    if (!open || !modalRef.current) return undefined;
    const modal = modalRef.current;
    const handleTab = (event: KeyboardEvent) => {
      if (event.key !== 'Tab') return;
      const focusables = modal.querySelectorAll<HTMLElement>(
        'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
      );
      if (focusables.length === 0) return;
      const first = focusables[0];
      const last = focusables[focusables.length - 1];
      if (event.shiftKey) {
        if (document.activeElement === first) {
          event.preventDefault();
          last.focus();
        }
      } else if (document.activeElement === last) {
        event.preventDefault();
        first.focus();
      }
    };
    modal.addEventListener('keydown', handleTab);
    return () => modal.removeEventListener('keydown', handleTab);
  }, [open]);

  if (!open || !strategyId) return null;

  return (
    <div
      className={`fixed inset-0 z-50 bg-black/60 ${isMobile ? 'flex flex-col justify-end' : 'flex items-center justify-center'}`}
      role="dialog"
      aria-modal="true"
      aria-label="Param diff modal"
      onClick={(event) => {
        if (event.target === event.currentTarget) {
          onClose();
        }
      }}
    >
      <div
        ref={modalRef}
        className={`w-full ${isMobile ? 'rounded-t-2xl' : 'rounded-lg'} border border-neutral-700 bg-neutral-900 p-6 shadow-2xl`}
        style={{ maxHeight: isMobile ? '85vh' : undefined, overflowY: 'auto' }}
      >
        <div className="flex items-start justify-between mb-4">
          <div>
            <p className="text-sm uppercase tracking-wide text-neutral-400">Strategy</p>
            <h2 className="text-xl font-semibold text-neutral-50">{strategyId}</h2>
          </div>
          <button
            onClick={onClose}
            className="text-neutral-400 hover:text-neutral-200 text-2xl leading-none"
            aria-label="Close diff modal"
          >
            ×
          </button>
        </div>

        <p className="text-sm text-neutral-300 mb-4">
          Remote values changed while you were editing. Compare your local edits to the
          latest remote snapshot and choose which values to keep.
        </p>

        <div className="max-h-64 overflow-y-auto rounded border border-neutral-700">
          <table className="w-full text-sm">
            <thead className="bg-neutral-800 text-neutral-200 text-left text-xs uppercase tracking-wide">
              <tr>
                <th className="px-3 py-2">Parameter</th>
                <th className="px-3 py-2">Mine</th>
                <th className="px-3 py-2">Remote</th>
              </tr>
            </thead>
            <tbody>
              {entries.map((entry) => (
                <tr key={entry.key} className="border-t border-neutral-800">
                  <td className="px-3 py-2 font-mono text-neutral-300">{entry.key}</td>
                  <td className="px-3 py-2 font-mono text-amber-200">{entry.mine ?? ''}</td>
                  <td className="px-3 py-2 font-mono text-sky-200">{entry.remote ?? ''}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>

        <div className="mt-6 flex flex-wrap items-center gap-3">
          <button
            ref={primaryButtonRef}
            type="button"
            onClick={onApplyRemote}
            className="rounded-md bg-sky-500/80 px-4 py-2 text-sm font-semibold text-neutral-900 hover:bg-sky-400"
          >
            Apply Remote Values
          </button>
          <button
            type="button"
            onClick={onKeepMine}
            className="rounded-md border border-emerald-500/70 px-4 py-2 text-sm font-semibold text-emerald-200 hover:bg-emerald-500/20"
          >
            Keep My Values
          </button>
          <button
            type="button"
            onClick={onClose}
            className="rounded-md border border-neutral-600 px-4 py-2 text-sm font-medium text-neutral-300 hover:bg-neutral-800"
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
}
