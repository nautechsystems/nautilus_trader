<!-- DOCID: ui-standards.md@v1 -->
Last updated: 2025-12-10 · commit 70da26dc

<!-- DOCID: fluxboard/ui-standards@v1 -->
# Fluxboard UI Standards

**Version:** 1.0
**Last Updated:** 2025-11-03
**Status:** Living Document

## Purpose

This document defines the UI standardization guidelines for Fluxboard panels, ensuring visual consistency, maintainability, and optimal user experience across all dashboard components.

## Scope

- Panel structure (PanelHeader, TableFilter, DataTable, custom tables)
- Design token system (`lib/tokens.ts`) for colors, spacing, typography, and density
- Live update patterns (polling, WebSocket, freshness)
- Interaction, accessibility, and virtualization requirements for Fluxboard panels

## Interface

- Core components:
  - `PanelHeader`
  - `TableFilter`
  - `DataTable`
- Token exports: `colors`, `spacing`, `typography`, `severity`, `tradeSide`, `INTERVALS`, `STALE_THRESHOLDS`
- Density control: `dense?: boolean` prop passed through panel tree

## Prereqs

- Fluxboard React codebase checked out and running
- Familiarity with React, TypeScript, and Tailwind (for layout-only classes)

## Procedure

1. Start from the **Panel Structure Template** and choose the closest pattern (simple, custom actions, filters, dual-context).
2. Apply token system rules for all colors, spacing, and typography.
3. Implement density support via a `dense` prop and `getDensityStyles`.
4. Wire live update hooks (polling/WebSocket) and freshness indicators.
5. Validate accessibility and interaction patterns as described below.

## Validation

- Use the **Migration Checklist** and **Accessibility Requirements** sections to verify new or refactored panels.
- Confirm no Tailwind color/spacing classes remain; all such values must come from tokens.

## Rollback

- If a migration causes regressions, revert the panel to its previous implementation while keeping tokenized helpers and shared components intact where possible.

## Troubleshooting

- Refer to the **Migration Checklist** and examples when panels diverge from standards or exhibit layout/interaction issues.

## FAQ

- **Q:** When can I use a custom table instead of `DataTable`?
  **A:** Only for performance-critical or highly specialized cases; document the exception in the component and follow the **Virtualization Patterns** guidance.

## Examples

- Minimal and full-featured panel patterns are referenced later in this document.
- Additional UI examples live alongside components under `fluxboard/components/domain/**`.

## References

- Tokens: `fluxboard/lib/tokens.ts`
- Shared components: `fluxboard/components/shared/*`
- Scanners performance standards: `fluxboard/docs/ScannersPerfV2.md`

## Changelog

- 2025-11-20: Added standard doc sections and removed lint suppressions; main content unchanged.

## Table of Contents

1. [Panel Structure Template](#panel-structure-template)
2. [Token System Usage](#token-system-usage)
3. [Dense Mode Standards](#dense-mode-standards)
4. [Live Update Patterns](#live-update-patterns)
5. [Component Selection Guide](#component-selection-guide)
6. [Color & Typography](#color--typography)
7. [Interaction Patterns](#interaction-patterns)
8. [Accessibility Requirements](#accessibility-requirements)
9. [Virtualization Patterns](#virtualization-patterns)

---

## Panel Structure Template

All panels MUST follow this standardized structure:

```tsx
// Example: see examples/ui/MyPanel.example.tsx for full implementation.
```

### DataTable Controlled State (New)

Use controlled props when you need to persist UI state across re-renders:

```tsx
// Example: see examples/ui/DataTableControlledState.example.tsx for full implementation.
```

This enables external persistence (e.g., localStorage) and synchronized UI across nested components.

### Panel Structure Rules

**MUST:**

- Use `PanelHeader` for all panels (no custom headers)
- Use `TableFilter` for panels with column-based filtering
- Use token system for ALL colors, spacing, typography
- Support `dense` prop for density control
- Include `lastUpdate` timestamp for freshness tracking

**SHOULD:**

- Use `DataTable` for tabular data (unless performance requires custom table)
- Use `usePolling` and `useWebSocket` hooks for live updates
- Provide loading and empty states
- Support keyboard navigation

**MUST NOT:**

- Use direct Tailwind color classes (`bg-neutral-900`, `text-red-400`, etc.)
- Create custom header components
- Implement custom filter UI (use TableFilter)
- Use inline styles for colors/spacing (use token system)

---

## Token System Usage

The design token system (`lib/tokens.ts`) provides centralized theming. **All color, spacing, and typography values MUST come from tokens.**

### Import Pattern

```tsx
import { colors, spacing, typography, severity } from '@/lib/tokens';
```

### Color Tokens

**Background Colors:**

```tsx
// DO THIS ✅
style={{ backgroundColor: colors.bg.base }}        // Main background
style={{ backgroundColor: colors.bg.surface }}     // Panel surface
style={{ backgroundColor: colors.bg.hover }}       // Hover states
style={{ backgroundColor: colors.bg.active }}      // Active states

// NOT THIS ❌
className="bg-neutral-900"
className="bg-neutral-800"
```

**Text Colors:**

```tsx
// DO THIS ✅
style={{ color: colors.text.primary }}    // Primary text (neutral-50)
style={{ color: colors.text.secondary }}  // Secondary text (neutral-300)
style={{ color: colors.text.tertiary }}   // Tertiary text (neutral-400)
style={{ color: colors.text.muted }}      // Muted text (neutral-500)

// NOT THIS ❌
className="text-neutral-50"
className="text-neutral-400"
```

**Semantic Colors:**

```tsx
// DO THIS ✅
style={{ color: severity.critical }}  // Red (critical alerts, errors)
style={{ color: severity.warning }}   // Amber (warnings)
style={{ color: severity.info }}      // Blue (info messages)
style={{ color: severity.success }}   // Emerald (success states)

// NOT THIS ❌
className="text-red-400"
className="text-amber-400"
```

**Border Colors:**

```tsx
// DO THIS ✅
style={{ borderColor: colors.border.DEFAULT }}  // Default border
style={{ borderColor: colors.border.hover }}    // Hover border
style={{ borderColor: colors.border.focus }}    // Focus border

// NOT THIS ❌
className="border-neutral-700"
```

### Spacing Tokens

**Row Heights:**

```tsx
// DO THIS ✅
style={{ height: spacing.row.compact }}  // 24px (dense mode)
style={{ height: spacing.row.normal }}   // 28px (normal mode)
style={{ height: spacing.row.header }}   // 36px (panel headers)

// NOT THIS ❌
className="h-6"
className="h-7"
```

**Padding:**

```tsx
// DO THIS ✅
style={{ padding: spacing.padding.dense }}       // Dense mode: 4px
style={{ padding: spacing.padding.normal }}      // Normal mode: 8px
style={{ padding: spacing.padding.comfortable }} // Comfortable: 12px

// NOT THIS ❌
className="p-2"
className="px-3 py-2"
```

**Gaps:**

```tsx
// DO THIS ✅
style={{ gap: spacing.gap.xs }}   // 4px
style={{ gap: spacing.gap.sm }}   // 8px
style={{ gap: spacing.gap.md }}   // 12px
style={{ gap: spacing.gap.lg }}   // 16px

// NOT THIS ❌
className="gap-2"
className="gap-4"
```

### Typography Tokens

```tsx
// DO THIS ✅
style={{
  fontSize: typography.fontSize.sm,      // 11px
  fontWeight: typography.fontWeight.medium,
  fontFamily: typography.fontFamily.sans
}}

// NOT THIS ❌
className="text-xs font-medium"
```

### Migration Strategy

**Phase 1: New Code**

- All new components MUST use token system
- No exceptions for color/spacing values

**Phase 2: Existing Code**

- Use codemod script to migrate Tailwind classes
- Migrate file-by-file during refactoring

**Phase 3: Enforcement**

- ESLint rule blocks direct Tailwind color classes
- Pre-commit hook validates token usage

---

## Dense Mode Standards

All panels MUST support density control via a single `dense` prop.

### Standard Prop

```tsx
interface PanelProps {
  dense?: boolean;  // Default: false
}
```

### Density Values

| Aspect | Normal Mode | Dense Mode |
|--------|-------------|------------|
| **Row Height** | `spacing.row.normal` (28px) | `spacing.row.compact` (24px) |
| **Padding** | `spacing.padding.normal` (8px) | `spacing.padding.dense` (4px) |
| **Font Size** | `typography.fontSize.base` (12px) | `typography.fontSize.sm` (11px) |
| **Gap** | `spacing.gap.md` (12px) | `spacing.gap.sm` (8px) |

### Implementation Pattern

```tsx
export function MyPanel({ dense = false }: { dense?: boolean }) {
  return (
    <div
      style={{
        padding: dense ? spacing.padding.dense : spacing.padding.normal,
        gap: dense ? spacing.gap.sm : spacing.gap.md
      }}
    >
      <DataTable
        data={data}
        columns={columns}
        dense={dense}  // Pass through to child components
      />
    </div>
  );
}
```

### Dense Mode Checklist

- [ ] Accept `dense` prop in component interface
- [ ] Apply density to row heights
- [ ] Apply density to padding values
- [ ] Apply density to font sizes
- [ ] Pass `dense` to child components (DataTable, TableFilter, etc.)
- [ ] Test both modes visually

---

## Live Update Patterns

All panels with dynamic data MUST use standardized live update hooks.

### Hook Usage

**Polling Hook:**

```tsx
import { usePolling } from '@/hooks';

usePolling({
  onPoll: async () => {
    const data = await fetchData();
    setData(data);
    setLastUpdate(Date.now());
  },
  interval: 3000,           // 3 seconds
  enabled: !wsConnected     // Disable when WebSocket is active
});
```

**WebSocket Hook:**

```tsx
import { useWebSocket } from '@/hooks';

const { connected: wsConnected } = useWebSocket({
  event: 'myPanel:update',
  onMessage: (data) => {
    setData(data);
    setLastUpdate(Date.now());
  }
});
```

### Update Strategy Decision Tree

```
Is data available via WebSocket?
├─ YES: Use WebSocket primary + polling fallback
│  └─ Pattern: Hybrid (WS + polling when disconnected)
└─ NO: Use polling only
   └─ Pattern: Polling only
```

### Standard Intervals

```tsx
const INTERVALS = {
  CRITICAL: 1000,   // 1s - Trade blotter, critical alerts
  FAST: 2000,       // 2s - Strategy params, signal data
  NORMAL: 3000,     // 3s - Alerts, market data fallback
  SLOW: 5000,       // 5s - Balances, FX rates
  MANUAL: 10000     // 10s - PnL calculations, manual refresh
};
```

### Freshness Indicators

All panels MUST show data freshness:

```tsx
<PanelHeader
  title="Panel Name"
  lastUpdate={lastUpdate}
  staleThresholdMs={10000}  // 10 seconds
/>
```

**Freshness Rules:**

- Green dot: Data fresh (< threshold)
- Yellow dot: Data stale (> threshold, < 2x threshold)
- Red dot: Data very stale (> 2x threshold)
- Timestamp tooltip on hover

---

## Component Selection Guide

### When to Use PanelHeader

**ALWAYS** - Every panel must use PanelHeader.

```tsx
<PanelHeader
  title="Panel Name"              // Required
  onRefresh={handleRefresh}       // Optional: adds refresh button
  lastUpdate={lastUpdate}         // Optional: shows freshness indicator
  staleThresholdMs={10000}       // Optional: threshold for stale data
  onRemove={handleRemove}         // Optional: adds remove button
  onFullPage={handleFullPage}     // Optional: adds full-page button
  collapsed={collapsed}           // Optional: collapse state
  onCollapse={setCollapsed}       // Optional: toggle collapse
  actions={<CustomActions />}     // Optional: custom action buttons
/>
```

### When to Use TableFilter

**Use TableFilter when:**

- Panel has 2+ filterable columns
- Panel needs column-based filtering (text, select, date)
- Panel has secondary controls (auto-refresh toggle, etc.)

**Don't use TableFilter when:**

- Panel has only one filter (use inline search instead)
- Panel has no filterable data
- Filtering is complex/custom (document exception)

```tsx
<TableFilter
  columns={[
    { key: 'status', label: 'Status', type: 'select', options: [...] },
    { key: 'symbol', label: 'Symbol', type: 'text' }
  ]}
  onFilterChange={handleFilterChange}
  compact={dense}
  customControls={
    <Switch checked={autoRefresh} onChange={setAutoRefresh}>
      Auto-refresh
    </Switch>
  }
/>
```

### When to Use DataTable

**Use DataTable when:**

- Data is tabular (rows x columns)
- Need sorting, row selection, standard table features
- Performance is acceptable (< 1000 rows without virtualization)

**Use Custom Table when:**

- Performance is critical (> 1000 rows, need virtualization)
- Table has complex inline editing (e.g., Params panel)
- Table has custom expand/collapse logic
- **Document exception in component comment**

```tsx
// ✅ Good - Standard tabular data
<DataTable
  data={balances}
  columns={balanceColumns}
  dense={dense}
  loading={isLoading}
  emptyMessage="No balances found"
/>

// ✅ Good - Performance-critical (documented)
/**
 * Custom table implementation for performance.
 * Reason: 10,000+ rows, requires virtualization.
 * Future: Migrate to DataTable when virtualization added.
 */
<TradesTable data={trades} dense={dense} />
```

### When to Use Button vs IconButton

**Button:**

- Primary/secondary actions with labels
- When text clarifies action ("Refresh", "Export", "Save All")

**IconButton:**

- Toolbar actions with clear icons
- Space-constrained areas
- MUST have tooltip/aria-label

```tsx
// DO THIS ✅
<Button size="sm" variant="primary">
  Export CSV
</Button>

<IconButton
  icon={RefreshCw}
  onClick={handleRefresh}
  aria-label="Refresh data"
  title="Refresh"
/>

// NOT THIS ❌
<button className="px-3 py-1 bg-blue-500">Export</button>
```

---

## Color & Typography

### Color Usage Guidelines

**Semantic Color Mapping:**

| Use Case | Token | Color | When to Use |
|----------|-------|-------|-------------|
| Critical alerts, errors, sell side | `severity.critical` | Red (#f87171) | System errors, failed trades, critical warnings |
| Warnings, pending states | `severity.warning` | Amber (#fbbf24) | Warning alerts, pending operations |
| Info messages, buy side | `severity.info` | Blue (#60a5fa) | Informational alerts, buy side trades |
| Success states, positive trends | `severity.success` | Emerald (#34d399) | Successful operations, positive PnL |

**Trade Side Colors:**

```tsx
import { tradeSide } from '@/lib/tokens';

// DO THIS ✅
style={{ color: tradeSide.buy }}   // Blue for buy side
style={{ color: tradeSide.sell }}  // Red for sell side

// NOT THIS ❌
className="text-emerald-400"  // Wrong semantic mapping
```

**Background Hierarchy:**

```tsx
colors.bg.base     // #171717 - Outermost container
colors.bg.surface  // #262626 - Panel surface
colors.bg.hover    // #404040 - Hover states
colors.bg.active   // #525252 - Active/selected states
```

### Typography Scale

**Font Sizes:**

```tsx
typography.fontSize['2xs']  // 10px - Tiny labels, timestamps
typography.fontSize.xs      // 11px - Dense mode, secondary text
typography.fontSize.sm      // 11px - Dense mode, primary text
typography.fontSize.base    // 12px - Normal mode, primary text
typography.fontSize.lg      // 14px - Section headers
typography.fontSize.xl      // 16px - Panel titles
typography.fontSize['2xl']  // 20px - Dashboard headers
```

**Font Weights:**

```tsx
typography.fontWeight.normal   // 400 - Body text
typography.fontWeight.medium   // 500 - Emphasized text, labels
typography.fontWeight.semibold // 600 - Headers, important values
typography.fontWeight.bold     // 700 - Critical values, alerts
```

---

## Interaction Patterns

### Control Placement Standards

**PanelHeader (Top Bar):**

- Left: Panel title
- Center: (empty, reserved for future breadcrumbs)
- Right: Primary actions (Refresh, Export, Full-page, Remove)

**TableFilter (Filter Bar):**

- Left: Filter controls (dropdowns, search)
- Right: Secondary controls (Auto-refresh toggle, density toggle)

**Content Area:**

- Scrollable content
- Empty/loading states centered
- No controls in content area (move to header/filter bar)

### Button Hierarchy

**Primary Actions:**

```tsx
<Button variant="primary" size="sm">Save Changes</Button>
```

**Secondary Actions:**

```tsx
<Button variant="secondary" size="sm">Export CSV</Button>
```

**Danger Actions:**

```tsx
<Button variant="danger" size="sm">Clear All</Button>
```

**Ghost Actions (icon-only):**

```tsx
<Button variant="ghost" size="xs">
  <RefreshCw className="w-4 h-4" />
</Button>
```

### Loading States

**Panel Loading:**

```tsx
{isLoading ? (
  <LoadingState message="Loading panel data..." />
) : (
  <DataTable data={data} columns={columns} />
)}
```

**Inline Loading (refresh):**

```tsx
<Button
  variant="ghost"
  size="xs"
  onClick={handleRefresh}
  disabled={isRefreshing}
>
  <RefreshCw className={cn("w-4 h-4", isRefreshing && "animate-spin")} />
</Button>
```

### Empty States

**Standard Empty:**

```tsx
<EmptyState
  icon={Database}
  message="No data available"
  description="Data will appear here when available"
/>
```

**Empty with Action:**

```tsx
<EmptyState
  icon={Filter}
  message="No results found"
  description="Try adjusting your filters"
  action={<Button onClick={clearFilters}>Clear Filters</Button>}
/>
```

---

## Accessibility Requirements

### Keyboard Navigation

**All interactive elements MUST:**

- Be focusable (tab order)
- Have focus indicators (ring-2 ring-offset-2)
- Support keyboard activation (Enter/Space for buttons)

**Focus Management:**

```tsx
// Auto-focus on panel open
<input ref={autoFocusRef} />

// Trap focus in modals
<Dialog open={open} onClose={onClose}>
  <Dialog.Content> {/* Focus trapped here */} </Dialog.Content>
</Dialog>
```

### ARIA Labels

**Icon Buttons:**

```tsx
<IconButton
  icon={RefreshCw}
  onClick={handleRefresh}
  aria-label="Refresh panel data"  // Required for screen readers
  title="Refresh"                  // Tooltip for sighted users
/>
```

**Data Tables:**

```tsx
<table aria-label="Balances table">
  <thead>
    <tr>
      <th scope="col">Exchange</th> {/* scope required */}
      <th scope="col">Asset</th>
    </tr>
  </thead>
</table>
```

**Live Regions:**

```tsx
<div
  role="status"
  aria-live="polite"
  aria-atomic="true"
>
  {updateMessage}
</div>
```

### Color Contrast

**Minimum Contrast Ratios (WCAG AA):**

- Normal text: 4.5:1
- Large text (>18px): 3:1
- Interactive elements: 3:1

**Token System Compliance:**
All token colors meet WCAG AA standards for their intended use.

---

## Migration Checklist

Use this checklist when migrating an existing panel:

### Phase 1: Structure

- [ ] Replace custom header with `<PanelHeader />`
- [ ] Replace custom filters with `<TableFilter />` (if applicable)
- [ ] Ensure `dense` prop is accepted and passed through
- [ ] Add `lastUpdate` state and freshness tracking

### Phase 2: Token Migration

- [ ] Replace all `className` color classes with `style` + `colors.*`
- [ ] Replace all spacing classes with `spacing.*`
- [ ] Replace all font size/weight classes with `typography.*`
- [ ] Remove all direct Tailwind color classes

### Phase 3: Components

- [ ] Migrate to `DataTable` (if applicable, or document exception)
- [ ] Use `Button` component for all buttons
- [ ] Use `Badge` component for all badges
- [ ] Use `Switch` for toggles, `Select` for dropdowns

### Phase 4: Live Updates

- [ ] Replace direct `socket.on()` with `useWebSocket()` hook
- [ ] Replace `setInterval` with `usePolling()` hook
- [ ] Add connection state tracking
- [ ] Use standard intervals from `INTERVALS` constant

### Phase 5: Testing

- [ ] Visual test in normal mode
- [ ] Visual test in dense mode
- [ ] Keyboard navigation test
- [ ] Screen reader test (basic)
- [ ] Live update test (WS + polling fallback)

---

## Examples

### Minimal Panel

```tsx
import { PanelHeader } from '@/components/shared/PanelHeader';
import { DataTable } from '@/components/ui/table/DataTable';
import { colors } from '@/lib/tokens';

export function MinimalPanel({ dense = false }) {
  const [data, setData] = useState([]);

  return (
    <div style={{ backgroundColor: colors.bg.base }}>
      <PanelHeader title="Minimal Panel" />
      <DataTable data={data} columns={columns} dense={dense} />
    </div>
  );
}
```

### Full-Featured Panel

```tsx
// Example: see examples/ui/FullFeaturedPanel.example.tsx for full implementation.
```

---

## Virtualization Patterns

Panels that can exceed ~200 rows MUST virtualize scrolling to keep DOM nodes and layout thrash under control. Follow the Params implementation pattern:

1. **Use `PanelBody` as the scroll container** and forward its ref into `useVirtualizer`.
2. **Leverage `@tanstack/react-virtual`** for row virtualization with `estimateSize` tied to density (`32px` dense, `44px` relaxed) and `overscan ≥ 8`.
3. **Preserve table semantics** by rendering spacer `<tr aria-hidden>` elements for `paddingTop`/`paddingBottom` instead of absolutely positioned `<div>`s.
4. **Measure real row heights** by passing `rowVirtualizer.measureElement` as the `ref` for each `<tr>` (needed for conflict banners or expanded rows).
5. **Integrate keyboard navigation** by storing the virtualizer in a ref and calling `scrollToIndex` before focusing a cell (Arrow keys, validation focus, etc.).

Reference snippet:

```tsx
const scrollRef = useRef<HTMLDivElement>(null);
const rowVirtualizer = useVirtualizer<HTMLDivElement, HTMLTableRowElement>({
  count: visibleRows.length,
  getScrollElement: () => scrollRef.current,
  estimateSize: () => (dense ? 32 : 44),
  overscan: 8,
});

<PanelBody ref={scrollRef}>
  <table>
    <thead>…</thead>
    <tbody>
      {paddingTop > 0 && (
        <tr aria-hidden>
          <td colSpan={colCount} style={{ height: paddingTop }} />
        </tr>
      )}
      {rowVirtualizer.getVirtualItems().map((virtualRow) => (
        <DataRow
          key={rows[virtualRow.index].id}
          idx={virtualRow.index}
          measureRow={rowVirtualizer.measureElement}
          {...rows[virtualRow.index]}
        />
      ))}
      {paddingBottom > 0 && (
        <tr aria-hidden>
          <td colSpan={colCount} style={{ height: paddingBottom }} />
        </tr>
      )}
    </tbody>
  </table>
</PanelBody>
```

This keeps sticky headers intact, maintains accessibility (real `<table>` semantics), and ensures smooth auto-focus behavior because the spacer rows preserve total scroll height. See `fluxboard/Params.tsx` for a full implementation with density toggles and conflict banners.

## Maintenance

### Adding New Panels

1. Copy minimal or full-featured example
2. Follow structure template
3. Use token system exclusively
4. Complete migration checklist
5. Add to Storybook

### Modifying Existing Standards

1. Propose change in team meeting
2. Update this document
3. Create migration plan for existing panels
4. Update examples
5. Run visual regression tests

### Questions?

See `fluxboard/docs/SELECTORS_GUIDE.md` for additional component documentation, or ask in team chat.

---

**Last Review:** 2025-11-03
**Next Review:** 2025-12-03
**Maintainer:** Fluxboard Team
