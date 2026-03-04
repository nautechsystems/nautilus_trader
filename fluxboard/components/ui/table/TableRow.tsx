/**
 * TableRow Component
 *
 * Table row with hover and selection states.
 * Supports dense mode for compact layouts.
 */

import { memo, type ReactNode } from 'react';
import { cn } from '@/lib/utils';
import { spacing } from '@/lib/tokens';

export interface TableRowProps {
  /** Row content */
  children: ReactNode;
  /** Whether the row is selected */
  selected?: boolean;
  /** Click handler */
  onClick?: () => void;
  /** Dense mode (24px height) */
  dense?: boolean;
  /** Additional CSS classes */
  className?: string;
}

/**
 * TableRow - Table row with hover and selection states
 *
 * @example
 * <TableRow selected onClick={handleClick} dense>
 *   <td>Cell content</td>
 * </TableRow>
 */
export const TableRow = memo(function TableRow({
  children,
  selected = false,
  onClick,
  dense = false,
  className,
}: TableRowProps) {
  const isClickable = !!onClick;

  const handleClick = () => {
    if (onClick) {
      onClick();
    }
  };

  const handleKeyDown = (event: React.KeyboardEvent) => {
    if (onClick && (event.key === 'Enter' || event.key === ' ')) {
      event.preventDefault();
      onClick();
    }
  };

  return (
    <tr
      className={cn(
        'transition-colors duration-150 border-b border-border',
        isClickable && 'cursor-pointer',
        !selected && 'hover:bg-bg-hover',
        selected && 'bg-accent/12 border-accent/60',
        className
      )}
      style={{
        height: dense ? spacing.row.compact : spacing.row.normal,
      }}
      onClick={handleClick}
      onKeyDown={handleKeyDown}
      role={isClickable ? 'button' : undefined}
      tabIndex={isClickable ? 0 : undefined}
      aria-selected={selected}
    >
      {children}
    </tr>
  );
});
