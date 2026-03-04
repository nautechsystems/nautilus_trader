/**
 * EmptyTableRow Component
 *
 * "No data" placeholder row for empty tables.
 * Displays centered message with optional icon.
 */

import { memo, type ReactNode } from 'react';
import { cn } from '@/lib/utils';
import { colors } from '@/lib/tokens';

export interface EmptyTableRowProps {
  /** Number of columns to span */
  colSpan: number;
  /** Message to display */
  message?: string;
  /** Optional icon to display */
  icon?: ReactNode;
  /** Additional CSS classes */
  className?: string;
}

/**
 * EmptyTableRow - Placeholder row for empty tables
 *
 * @example
 * <EmptyTableRow
 *   colSpan={5}
 *   message="No trades found"
 *   icon={<SearchIcon />}
 * />
 */
export const EmptyTableRow = memo(function EmptyTableRow({
  colSpan,
  message = 'No data',
  icon,
  className,
}: EmptyTableRowProps) {
  return (
    <tr>
      <td
        colSpan={colSpan}
        className={cn(
          'py-8 text-center text-sm',
          className
        )}
        style={{
          color: colors.text.muted,
        }}
      >
        <div className="flex flex-col items-center gap-2">
          {icon && (
            <div className="opacity-50">
              {icon}
            </div>
          )}
          <span>{message}</span>
        </div>
      </td>
    </tr>
  );
});
