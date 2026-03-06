// Reusable pagination controls

import { ChevronsLeft, ChevronLeft, ChevronRight, ChevronsRight } from 'lucide-react';
import { IconButton, Select } from '../ui';
import { colors, typography } from '@/lib/tokens';

export function Pager({
  page,
  pageSize,
  total,
  onPageChange,
  onPageSizeChange,
  borderPosition = 'top',
  itemLabel = 'rows',
  layout = 'default',
  showRange = true,
  rangeFormat = 'slash',
}: {
  page: number;
  pageSize: number;
  total: number;
  onPageChange: (p: number) => void;
  onPageSizeChange: (s: number) => void;
  borderPosition?: 'top' | 'bottom';
  itemLabel?: string;
  layout?: 'default' | 'split';
  showRange?: boolean;
  rangeFormat?: 'slash' | 'of';
}) {
  const pages = Math.max(1, Math.ceil(total / pageSize));
  const canPrev = page > 1;
  const canNext = page < pages;

  const startRow = total === 0 ? 0 : (page - 1) * pageSize + 1;
  const endRow = Math.min(page * pageSize, total);

  const pageSizeOptions = [
    { label: '50/page', value: '50' },
    { label: '100/page', value: '100' },
    { label: '200/page', value: '200' },
    { label: '500/page', value: '500' },
  ];

  const borderClass = borderPosition === 'top' ? 'border-t' : 'border-b';
  const borderColorKey = borderPosition === 'top' ? 'borderTopColor' : 'borderBottomColor';

  const rangeLabel =
    rangeFormat === 'of'
      ? `${startRow}–${endRow} of ${total} ${itemLabel}`
      : `${startRow}–${endRow} / ${total} ${itemLabel}`;

  const rangeNode = showRange ? (
    <span
      style={{
        color: colors.text.muted,
        fontSize: typography.fontSize.xs,
      }}
    >
      {rangeLabel}
    </span>
  ) : null;

  const controls = (
    <div className="flex items-center gap-2">
      <IconButton
        variant={'secondary' as const}
        size={'xs' as const}
        disabled={!canPrev}
        onClick={() => onPageChange(1)}
        aria-label="First page"
        title="First page"
      >
        <ChevronsLeft className="w-3 h-3" />
      </IconButton>

      <IconButton
        variant={'secondary' as const}
        size={'xs' as const}
        disabled={!canPrev}
        onClick={() => onPageChange(page - 1)}
        aria-label="Previous page"
        title="Previous page"
      >
        <ChevronLeft className="w-3 h-3" />
      </IconButton>

      <span
        style={{
          color: colors.text.secondary,
          fontSize: typography.fontSize.xs,
          minWidth: '80px',
          textAlign: 'center',
        }}
      >
        Page {page} / {pages}
      </span>

      <IconButton
        variant={'secondary' as const}
        size={'xs' as const}
        disabled={!canNext}
        onClick={() => onPageChange(page + 1)}
        aria-label="Next page"
        title="Next page"
      >
        <ChevronRight className="w-3 h-3" />
      </IconButton>

      <IconButton
        variant={'secondary' as const}
        size={'xs' as const}
        disabled={!canNext}
        onClick={() => onPageChange(pages)}
        aria-label="Last page"
        title="Last page"
      >
        <ChevronsRight className="w-3 h-3" />
      </IconButton>

      <div className="ml-1">
        <Select
          value={pageSize.toString()}
          onChange={(val) => onPageSizeChange(parseInt(val, 10))}
          options={pageSizeOptions}
          size={'sm' as const}
        />
      </div>
    </div>
  );

  return (
    <div
      className={`flex items-center justify-between ${borderClass} backdrop-blur-sm`}
      style={{
        padding: '6px 12px',
        backgroundColor: `${colors.bg.surface}cc`, // 80% opacity
        [borderColorKey]: `${colors.neutral[800]}66`, // 40% opacity
        fontSize: typography.fontSize.xs,
      }}
    >
      {layout === 'split' ? (
        <>
          {rangeNode}
          {controls}
        </>
      ) : (
        <>
          {controls}
          {rangeNode}
        </>
      )}
    </div>
  );
}
