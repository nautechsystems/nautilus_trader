/**
 * HeaderWithHelp component - Table header with tooltip and modal help.
 *
 * Features:
 * - Tooltip on hover (short description + unit)
 * - Click to open modal with full help
 * - Keyboard accessible (Enter/Space to open)
 * - ARIA attributes for accessibility
 */

import type { DragEvent, KeyboardEvent, CSSProperties } from 'react';
import type { ParamDef } from '../../types';
import { getParamTooltip } from '../../utils/validation';

export type HeaderWithHelpProps = {
  paramDef: ParamDef;
  onModalOpen: () => void;
  // Optional sorting controls (used selectively, e.g., bot_on)
  sortable?: boolean;
  sortActive?: boolean;
  sortDirection?: 'asc' | 'desc' | null;
  onSortToggle?: () => void;
  dragEnabled?: boolean;
  dragState?: 'idle' | 'dragging' | 'over-before' | 'over-after';
  onDragStart?: (event: DragEvent<Element>) => void;
  onDragOver?: (event: DragEvent<Element>) => void;
  onDragEnter?: (event: DragEvent<Element>) => void;
  onDragLeave?: (event: DragEvent<Element>) => void;
  onDrop?: (event: DragEvent<Element>) => void;
  onDragEnd?: (event: DragEvent<Element>) => void;
  className?: string;
  hint?: string;
  style?: CSSProperties;
};

export default function HeaderWithHelp({
  paramDef,
  onModalOpen,
  sortable = false,
  sortActive = false,
  sortDirection = null,
  onSortToggle,
  dragEnabled = false,
  dragState = 'idle',
  onDragStart,
  onDragOver,
  onDragEnter,
  onDragLeave,
  onDrop,
  onDragEnd,
  className,
  hint,
  style
}: HeaderWithHelpProps) {
  const tooltipBase = getParamTooltip(paramDef);
  const tooltip = hint ? `${tooltipBase} — ${hint}` : tooltipBase;

  const handleClick = () => {
    onModalOpen();
  };

  const handleKeyDown = (e: KeyboardEvent) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      onModalOpen();
    }
  };

  const handleDragStart = (e: DragEvent<HTMLButtonElement>) => {
    e.stopPropagation();
    if (onDragStart) {
      onDragStart(e);
    }
  };

  const handleDragEnd = (e: DragEvent<HTMLButtonElement>) => {
    e.stopPropagation();
    if (onDragEnd) {
      onDragEnd(e);
    }
  };

  const alignClasses = paramDef.type === 'int' || paramDef.type === 'float'
    ? 'text-right'
    : 'text-center';
  const justifyClass = paramDef.type === 'int' || paramDef.type === 'float' ? 'justify-end' : 'justify-center';

  const showSortIcon = sortable;
  const displayIndicator = !sortActive || !sortDirection ? '↕' : (sortDirection === 'asc' ? '↑' : '↓');

  const dragHighlightClass = (() => {
    if (!dragEnabled) return '';
    switch (dragState) {
      case 'dragging':
        return 'opacity-75';
      case 'over-before':
        return 'shadow-[inset_3px_0_0_rgba(56,189,248,0.6)]';
      case 'over-after':
        return 'shadow-[inset_-3px_0_0_rgba(56,189,248,0.6)]';
      default:
        return '';
    }
  })();

  const combinedClassName = `relative p-2 bg-neutral-800 border-b border-neutral-700 ${alignClasses} cursor-help whitespace-nowrap overflow-hidden ${dragHighlightClass} ${className ?? ''}`.trim();

  return (
    <th
      className={combinedClassName}
      title={tooltip}
      onClick={handleClick}
      onKeyDown={handleKeyDown}
      tabIndex={0}
      role="button"
      aria-label={`${paramDef.label} - Click for help`}
      aria-describedby={`${paramDef.key}-tooltip`}
      onDragEnter={dragEnabled ? (e) => { if (onDragEnter) onDragEnter(e); } : undefined}
      onDragOver={dragEnabled ? (e) => { if (onDragOver) onDragOver(e); } : undefined}
      onDragLeave={dragEnabled ? (e) => { if (onDragLeave) onDragLeave(e); } : undefined}
      onDrop={dragEnabled ? (e) => { if (onDrop) onDrop(e); } : undefined}
      data-drag-state={dragEnabled ? dragState : undefined}
      aria-sort={sortActive && sortDirection ? (sortDirection === 'asc' ? 'ascending' : 'descending') : 'none'}
      style={style}
    >
      <span className={`group inline-flex items-center ${justifyClass} gap-1 w-full max-w-full whitespace-nowrap overflow-hidden`}>
        {dragEnabled && (
          <button
            type="button"
            draggable
            onDragStart={handleDragStart}
            onDragEnd={handleDragEnd}
            onMouseDown={(e) => e.stopPropagation()}
            onClick={(e) => e.preventDefault()}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === ' ') {
                e.preventDefault();
              }
            }}
            className="flex-none px-1 text-neutral-500 opacity-0 transition-opacity duration-150 group-hover:opacity-100 focus-visible:opacity-100 focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/60 active:cursor-grabbing cursor-grab"
            aria-label={`Reorder ${paramDef.label} column`}
            title="Drag to reorder column"
          >
            ⋮⋮
          </button>
        )}
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            if (sortable && onSortToggle) {
              onSortToggle();
            }
          }}
          className="flex items-center gap-1 truncate underline decoration-dotted decoration-neutral-600 hover:decoration-neutral-400 focus:outline-none focus-visible:ring-2 focus-visible:ring-blue-500/60 rounded-sm"
          aria-label={sortable ? `Sort by ${paramDef.label}` : paramDef.label}
          title={paramDef.label}
        >
          <span className="truncate">{paramDef.label}</span>
          {showSortIcon && (
            <span className="flex-none text-neutral-500 group-hover:text-neutral-200">
              {displayIndicator}
            </span>
          )}
        </button>
      </span>
      <span id={`${paramDef.key}-tooltip`} className="sr-only">
        {tooltip}
      </span>
    </th>
  );
}
