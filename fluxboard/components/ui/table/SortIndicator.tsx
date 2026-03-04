/**
 * SortIndicator Component
 *
 * Unified sort arrow component that displays sort state for table columns.
 * Shows neutral, ascending, or descending state with appropriate arrow icons.
 */

import { memo } from 'react';
import { cn } from '@/lib/utils';
import { colors } from '@/lib/tokens';

export interface SortIndicatorProps {
  /** The column being displayed */
  column: string;
  /** The currently sorted column */
  sortColumn: string | null;
  /** The current sort direction */
  sortDirection: 'asc' | 'desc' | null;
  /** Additional CSS classes */
  className?: string;
}

/**
 * SortIndicator - Displays sort state for a table column
 *
 * @example
 * <SortIndicator
 *   column="timestamp"
 *   sortColumn="timestamp"
 *   sortDirection="desc"
 * />
 */
export const SortIndicator = memo(function SortIndicator({
  column,
  sortColumn,
  sortDirection,
  className,
}: SortIndicatorProps) {
  const isSorted = sortColumn === column;
  const isAscending = isSorted && sortDirection === 'asc';
  const isDescending = isSorted && sortDirection === 'desc';

  // Determine which arrow to show
  let arrow = '↕'; // Neutral
  if (isAscending) arrow = '↑';
  if (isDescending) arrow = '↓';

  return (
    <span
      className={cn(
        'inline-block ml-1 select-none',
        'transition-colors duration-150',
        className
      )}
      style={{
        fontSize: '10px',
        color: isSorted ? colors.accent.DEFAULT : colors.text.muted,
      }}
      aria-label={
        isSorted
          ? `Sorted ${sortDirection === 'asc' ? 'ascending' : 'descending'}`
          : 'Not sorted'
      }
    >
      {arrow}
    </span>
  );
});
