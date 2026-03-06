/**
 * TableHeader Component
 *
 * Sticky table header with sort capability.
 * Stays at the top of the table when scrolling.
 */

import { memo, type ReactNode } from 'react';
import { cn } from '@/lib/utils';
import { colors, elevation } from '@/lib/tokens';

export interface TableHeaderProps {
  /** Header content */
  children: ReactNode;
  /** Whether the column is sortable */
  sortable?: boolean;
  /** Sort handler */
  onSort?: () => void;
  /** Additional CSS classes */
  className?: string;
  /** Inline styles */
  style?: React.CSSProperties;
}

/**
 * TableHeader - Sticky table header component
 *
 * @example
 * <TableHeader sortable onSort={handleSort}>
 *   Column Name
 * </TableHeader>
 */
export const TableHeader = memo(function TableHeader({
  children,
  sortable = false,
  onSort,
  className,
  style,
}: TableHeaderProps) {
  const handleClick = () => {
    if (sortable && onSort) {
      onSort();
    }
  };

  const handleKeyDown = (event: React.KeyboardEvent) => {
    if (sortable && onSort && (event.key === 'Enter' || event.key === ' ')) {
      event.preventDefault();
      onSort();
    }
  };

  return (
    <th
      className={cn(
        'sticky top-0 px-[10px] py-[6px] text-left text-xs font-semibold tracking-[0.04em] uppercase',
        'select-none border-b border-border bg-[#111214]',
        sortable && 'cursor-pointer hover:bg-bg-hover',
        className
      )}
      style={{
        backgroundColor: '#111214',
        color: colors.text.muted,
        zIndex: elevation.header,
        letterSpacing: '0.04em',
        ...style,
      }}
      onClick={handleClick}
      onKeyDown={handleKeyDown}
      role={sortable ? 'button' : undefined}
      tabIndex={sortable ? 0 : undefined}
      aria-sort={sortable ? 'none' : undefined}
    >
      {children}
    </th>
  );
});
