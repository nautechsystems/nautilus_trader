/**
 * ParamHelpModal component - Full help dialog for a parameter.
 *
 * Features:
 * - Shows detailed description, type, bounds, default, unit
 * - ESC key to close
 * - Click outside to close
 * - Focus trap (keyboard navigation stays within modal)
 * - Returns focus to trigger element on close
 * - ARIA attributes for screen readers
 */

import { useEffect, useRef } from 'react';
import type { ParamDef } from '../../types';
import { useMobileLayout } from '@/hooks/useMobileLayout';

export type ParamHelpModalProps = {
  paramDef: ParamDef | null;
  open: boolean;
  onClose: () => void;
};

export default function ParamHelpModal({
  paramDef,
  open,
  onClose
}: ParamHelpModalProps) {
  const modalRef = useRef<HTMLDivElement>(null);
  const closeButtonRef = useRef<HTMLButtonElement>(null);
  const previousActiveElement = useRef<Element | null>(null);
  const { isMobile } = useMobileLayout();

  // Store previous focus and set focus to close button when modal opens
  useEffect(() => {
    if (open) {
      previousActiveElement.current = document.activeElement;
      setTimeout(() => {
        closeButtonRef.current?.focus();
      }, 100);
    } else {
      // Return focus to previous element when modal closes
      if (previousActiveElement.current instanceof HTMLElement) {
        previousActiveElement.current.focus();
      }
    }
  }, [open]);

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

  // Focus trap
  useEffect(() => {
    if (!open || !modalRef.current) return;

    const modal = modalRef.current;
    const focusableElements = modal.querySelectorAll<HTMLElement>(
      'button, [href], input, select, textarea, [tabindex]:not([tabindex="-1"])'
    );

    const firstElement = focusableElements[0];
    const lastElement = focusableElements[focusableElements.length - 1];

    const handleTab = (e: KeyboardEvent) => {
      if (e.key !== 'Tab') return;

      if (e.shiftKey) {
        // Shift+Tab
        if (document.activeElement === firstElement) {
          e.preventDefault();
          lastElement.focus();
        }
      } else {
        // Tab
        if (document.activeElement === lastElement) {
          e.preventDefault();
          firstElement.focus();
        }
      }
    };

    modal.addEventListener('keydown', handleTab);
    return () => modal.removeEventListener('keydown', handleTab);
  }, [open]);

  if (!open || !paramDef) return null;

  const handleBackdropClick = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget) {
      onClose();
    }
  };

  return (
    <div
      className={`fixed inset-0 z-50 bg-black bg-opacity-50 ${isMobile ? 'flex flex-col justify-end' : 'flex items-center justify-center'}`}
      onClick={handleBackdropClick}
      role="dialog"
      aria-modal="true"
      aria-labelledby="param-modal-title"
    >
      <div
        ref={modalRef}
        className={`bg-neutral-900 border border-neutral-700 ${isMobile ? 'rounded-t-2xl' : 'rounded-lg'} shadow-xl max-w-2xl w-full mx-4 p-6`}
        style={{ maxHeight: '85vh', overflowY: 'auto' }}
      >
        {/* Header */}
        <div className="flex items-start justify-between mb-4">
          <h2
            id="param-modal-title"
            className="text-xl font-semibold text-neutral-100"
          >
            {paramDef.label}
            {paramDef.unit && (
              <span className="text-sm text-neutral-400 ml-2">({paramDef.unit})</span>
            )}
          </h2>
          <button
            ref={closeButtonRef}
            onClick={onClose}
            className="text-neutral-400 hover:text-neutral-200 text-2xl leading-none"
            aria-label="Close help modal"
          >
            ×
          </button>
        </div>

        {/* Content */}
        <div className="space-y-4 text-sm text-neutral-300">
          {/* Description */}
          <div>
            <h3 className="text-neutral-100 font-medium mb-1">Description</h3>
            <p className="text-neutral-300">{paramDef.description}</p>
          </div>

          {/* Type & Constraints */}
          <div className="grid grid-cols-2 gap-4">
            <div>
              <h3 className="text-neutral-100 font-medium mb-1">Type</h3>
              <p className="text-neutral-300 font-mono">{paramDef.type}</p>
            </div>
            <div>
              <h3 className="text-neutral-100 font-medium mb-1">Default</h3>
              <p className="text-neutral-300 font-mono">
                {String(paramDef.default)}
              </p>
            </div>
          </div>

          {/* Bounds (for numeric types) */}
          {(paramDef.type === 'int' || paramDef.type === 'float') && (
            <div className="grid grid-cols-2 gap-4">
              {paramDef.min_value !== null && paramDef.min_value !== undefined && (
                <div>
                  <h3 className="text-neutral-100 font-medium mb-1">Minimum</h3>
                  <p className="text-neutral-300 font-mono">
                    {paramDef.min_value}
                    {paramDef.unit && <span className="text-neutral-500 ml-1">{paramDef.unit}</span>}
                  </p>
                </div>
              )}
              {paramDef.max_value !== null && paramDef.max_value !== undefined && (
                <div>
                  <h3 className="text-neutral-100 font-medium mb-1">Maximum</h3>
                  <p className="text-neutral-300 font-mono">
                    {paramDef.max_value}
                    {paramDef.unit && <span className="text-neutral-500 ml-1">{paramDef.unit}</span>}
                  </p>
                </div>
              )}
            </div>
          )}

          {/* Options (for select types) */}
          {paramDef.options && paramDef.options.length > 0 && (
            <div>
              <h3 className="text-neutral-100 font-medium mb-1">Valid Options</h3>
              <ul className="space-y-1">
                {paramDef.options.map(([value, label]) => (
                  <li key={value} className="text-neutral-300 font-mono">
                    <span className="text-emerald-400">{value}</span>
                    {' → '}
                    <span>{label}</span>
                  </li>
                ))}
              </ul>
            </div>
          )}

          {/* Example (if applicable) */}
          {(paramDef.type === 'float' || paramDef.type === 'int') && (
            <div>
              <h3 className="text-neutral-100 font-medium mb-1">Example Value</h3>
              <p className="text-neutral-300 font-mono bg-neutral-800 px-3 py-2 rounded">
                {paramDef.type === 'float' ? '10.5' : '90'}
                {paramDef.unit && <span className="text-neutral-500 ml-1">{paramDef.unit}</span>}
              </p>
            </div>
          )}

          {/* Deprecation warning */}
          {paramDef.deprecated && (
            <div className="bg-amber-900 bg-opacity-20 border border-amber-700 rounded p-3">
              <p className="text-amber-400 font-medium">
                ⚠️ Deprecated Parameter
              </p>
              {paramDef.replacement && (
                <p className="text-amber-300 text-sm mt-1">
                  Use <span className="font-mono">{paramDef.replacement}</span> instead.
                </p>
              )}
            </div>
          )}
        </div>

        {/* Footer */}
        <div className="mt-6 flex justify-end">
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
