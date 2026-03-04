/**
 * TableFilter - Reusable table filter component with column-based filtering
 *
 * Provides standardized filtering UI for all Fluxboard panels with:
 * - Collapsible filter bar with active filter count
 * - Multiple filter types (text, select, date)
 * - Custom controls slot (e.g., auto-refresh toggle)
 * - Dense mode support
 * - Token-based styling
 *
 * @example
 * ```tsx
 * <TableFilter
 *   columns={[
 *     { key: 'status', label: 'Status', type: 'select', options: ['active', 'inactive'] },
 *     { key: 'symbol', label: 'Symbol', type: 'text' }
 *   ]}
 *   onFilterChange={handleFilterChange}
 *   dense={dense}
 *   customControls={
 *     <Switch checked={autoRefresh} onChange={setAutoRefresh}>
 *       Auto-refresh
 *     </Switch>
 *   }
 * />
 * ```
 */

import { useState, useCallback } from 'react';
import { ChevronRight, ChevronDown } from 'lucide-react';
import { Button, Badge } from '../ui';
import { colors, spacing, typography, borderRadius } from '@/lib/tokens';

export type FilterType = 'text' | 'select' | 'date';

export interface ColumnFilter {
  /** Unique key for this filter (matches data field) */
  key: string;
  /** Display label for the filter */
  label: string;
  /** Type of filter control */
  type: FilterType;
  /** Options for select type filters */
  options?: string[];
  /** Placeholder text for text inputs */
  placeholder?: string;
}

export interface FilterValues {
  [key: string]: string;
}

export interface TableFilterProps {
  /** Filter column configurations */
  columns: ColumnFilter[];
  /** Callback when filters change */
  onFilterChange: (filters: FilterValues) => void;
  /** Controlled filter values */
  value?: FilterValues;
  /** Dense mode (reduced padding/spacing) */
  dense?: boolean;
  /** Custom controls slot (e.g., auto-refresh toggle) */
  customControls?: React.ReactNode;
  /** Additional CSS class names */
  className?: string;
}

export function TableFilter({
  columns,
  onFilterChange,
  value,
  dense = false,
  customControls,
  className
}: TableFilterProps) {
  const isControlled = value !== undefined;
  const [internalFilters, setInternalFilters] = useState<FilterValues>(value ?? {});
  const filters = isControlled ? value ?? {} : internalFilters;
  const [isExpanded, setIsExpanded] = useState(false);

  const handleFilterChange = useCallback((key: string, value: string) => {
    const newFilters = { ...filters, [key]: value };
    if (!isControlled) {
      setInternalFilters(newFilters);
    }
    onFilterChange(newFilters);
  }, [filters, onFilterChange, isControlled]);

  const handleClearAll = useCallback(() => {
    if (!isControlled) {
      setInternalFilters({});
    }
    onFilterChange({});
  }, [onFilterChange, isControlled]);

  const activeFilterCount = Object.values(filters).filter(v => v !== '').length;

  const padding = dense ? spacing.padding.dense : spacing.padding.normal;
  const gap = dense ? spacing.gap.xs : spacing.gap.sm;

  return (
    <div
      className={className}
      style={{
        borderBottom: `1px solid ${colors.border.DEFAULT}`,
        backgroundColor: colors.bg.base,
      }}
    >
      {/* Toggle button */}
      <div
        className="flex items-center justify-between"
        style={{ padding }}
      >
        <button
          onClick={() => setIsExpanded(!isExpanded)}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap,
            color: colors.text.muted,
            fontSize: typography.fontSize.xs,
            transition: 'color 150ms ease',
          }}
          onMouseEnter={(e) => {
            e.currentTarget.style.color = colors.text.secondary;
          }}
          onMouseLeave={(e) => {
            e.currentTarget.style.color = colors.text.muted;
          }}
        >
          {isExpanded ? (
            <ChevronDown size={12} />
          ) : (
            <ChevronRight size={12} />
          )}
          <span>Filters</span>
          {activeFilterCount > 0 && (
            <Badge variant={'success' as const} size={'xs' as const}>
              {activeFilterCount}
            </Badge>
          )}
        </button>
        <div className="flex items-center" style={{ gap }}>
          {customControls}
          {activeFilterCount > 0 && (
            <Button
              variant={'ghost' as const}
              size={'xs' as const}
              onClick={handleClearAll}
              style={{
                color: colors.semantic.danger.light,
              }}
            >
              Clear All
            </Button>
          )}
        </div>
      </div>

      {/* Filter inputs */}
      {isExpanded && (
        <div
          className="grid"
          style={{
            backgroundColor: colors.bg.surface,
            gap,
            padding,
            gridTemplateColumns: columns.length === 0
              ? '1fr'
              : 'repeat(auto-fit, minmax(160px, 1fr))',
          }}
        >
          {columns.map((col) => (
            <div key={col.key} className="flex flex-col" style={{ gap: spacing.gap.xs }}>
              <label
                className="uppercase tracking-wide"
                style={{
                  fontSize: typography.fontSize['2xs'],
                  color: colors.text.muted,
                  fontWeight: typography.fontWeight.medium,
                }}
              >
                {col.label}
              </label>
              {col.type === 'text' && (
                <input
                  type="text"
                  value={filters[col.key] || ''}
                  onChange={(e) => handleFilterChange(col.key, e.target.value)}
                  placeholder={col.placeholder || `Filter ${col.label}...`}
                  style={{
                    borderRadius: borderRadius.sm,
                    backgroundColor: colors.bg.base,
                    borderWidth: '1px',
                    borderStyle: 'solid',
                    borderColor: colors.border.DEFAULT,
                    padding: `${spacing.gap.xs} ${spacing.gap.sm}`,
                    fontSize: typography.fontSize.xs,
                    color: colors.text.secondary,
                  }}
                  onFocus={(e) => {
                    e.currentTarget.style.borderColor = colors.border.focus;
                    e.currentTarget.style.outline = `1px solid ${colors.border.focus}`;
                  }}
                  onBlur={(e) => {
                    e.currentTarget.style.borderColor = colors.border.DEFAULT;
                    e.currentTarget.style.outline = 'none';
                  }}
                />
              )}
              {col.type === 'select' && (
                <select
                  value={filters[col.key] || ''}
                  onChange={(e) => handleFilterChange(col.key, e.target.value)}
                  style={{
                    borderRadius: borderRadius.sm,
                    backgroundColor: colors.bg.base,
                    borderWidth: '1px',
                    borderStyle: 'solid',
                    borderColor: colors.border.DEFAULT,
                    padding: `${spacing.gap.xs} ${spacing.gap.sm}`,
                    fontSize: typography.fontSize.xs,
                    color: colors.text.secondary,
                  }}
                  onFocus={(e) => {
                    e.currentTarget.style.borderColor = colors.border.focus;
                    e.currentTarget.style.outline = `1px solid ${colors.border.focus}`;
                  }}
                  onBlur={(e) => {
                    e.currentTarget.style.borderColor = colors.border.DEFAULT;
                    e.currentTarget.style.outline = 'none';
                  }}
                >
                  <option value="">All</option>
                  {col.options?.map((opt) => (
                    <option key={opt} value={opt}>{opt}</option>
                  ))}
                </select>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

type FilterNormalizer<T> = (value: string, row: T) => string;
type FilterMatcher<T> = (row: T, filterValue: string) => boolean;

export interface ApplyFiltersOptions<T> {
  columns?: ColumnFilter[];
  normalizers?: Partial<Record<string, FilterNormalizer<T>>>;
  matchers?: Partial<Record<string, FilterMatcher<T>>>;
}

/**
 * Apply filters to rows
 */
export function applyFilters<T extends Record<string, any>>(
  rows: T[],
  filters: FilterValues,
  options: ApplyFiltersOptions<T> = {}
): T[] {
  if (Object.keys(filters).length === 0) {
    return rows;
  }

  const columnMap = options.columns
    ? Object.fromEntries(options.columns.map((col) => [col.key, col]))
    : undefined;

  return rows.filter((row) => {
    return Object.entries(filters).every(([key, filterValue]) => {
      if (!filterValue) return true;  // Empty filter = show all

      const matcher = options.matchers?.[key];
      if (matcher) {
        try {
          return matcher(row, filterValue);
        } catch {
          return false;
        }
      }

      const rawRowValue = row[key];
      const stringValue = rawRowValue === null || rawRowValue === undefined ? '' : String(rawRowValue);
      const normalizer = options.normalizers?.[key];
      const normalizedValue = normalizer ? normalizer(stringValue, row) : stringValue;
      const searchValue = String(filterValue);
      const searchLower = searchValue.toLowerCase();
      const valueLower = normalizedValue.toLowerCase();

      const column = columnMap?.[key];
      if (column?.type === 'select' || column?.type === 'date') {
        return valueLower === searchLower;
      }

      return valueLower.includes(searchLower);
    });
  });
}
