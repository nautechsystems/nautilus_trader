import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { ChevronRight, Download, FileJson, ChevronsUpDown, ChevronUp, ChevronDown } from 'lucide-react';
import { toast } from 'sonner';

import { api } from './api';
import { INTERVALS } from './constants';
import { COLUMN_MAP } from './config/columnMap';
import {
  useBalancesStore,
  selectBalancesRows,
  selectBalancesLoading,
  selectBalancesTotals,
  selectBalancesRiskGroups,
  selectBalancesRiskSort,
  shallow,
} from './stores';
import { usePolling } from './hooks';
import { useMobileLayout } from './hooks/useMobileLayout';
import { PanelHeader } from './components/shared/PanelHeader';
import { PanelBody } from './components/shared/PanelBody';
import { DataAgeCell } from './components/shared/DataAgeCell';
import { CoinCell } from './components/shared/CoinCell';
import { PageShell } from './components/layout/PageShell';
import { usePanelHeaderSlots } from './components/layout/PanelWrapper';
import { Button } from './components/ui/button/Button';
import { Switch } from './components/ui/switch';
import { Tooltip, TooltipProvider } from './components/ui/tooltip';
import { TableFilter, applyFilters, type ColumnFilter, type FilterValues } from './components/shared/TableFilter';
import type { BalanceParentRow, BalanceChildRow } from './types';
import { RiskTable, type RiskSortState } from './components/balances/RiskTable';
import {
  DUST_THRESHOLD,
  formatMoney,
  formatMoneyNoSign,
  shortAddress,
} from './utils/balanceFormat';
import { formatMark, formatQty } from './lib/assetFormat';
import { exportCSV, exportJSON, generateTimestampFilename } from './utils/export';
import {
  colors,
  STALE_THRESHOLDS,
} from './lib/tokens';
import { resolvePathnameProfile, type PathProfile } from './config/uiProfiles';

import { cn } from './lib/utils';

const REFRESH_MS = INTERVALS.BALANCES_POLL;
const FILTER_STORAGE_KEY_PREFIX = 'balances:filters:v3';
const LEGACY_FILTER_STORAGE_KEY = 'balances:filters:v2';
const LEGACY_FILTER_STORAGE_KEY_V1 = 'balances:filters:v1';
const EXPANDED_STORAGE_KEY_PREFIX = 'balances:expanded:v2';
const LEGACY_EXPANDED_STORAGE_KEY = 'balances:expanded:v1';

type SortKey = 'mv' | 'time' | 'coin' | 'qty' | 'mark';
type SortDir = 'asc' | 'desc';

type FilterState = {
  hideZero: boolean;
  logicalOnly: boolean;
  stableOnly: boolean;
  sortBy: SortKey;
  sortDir: SortDir;
  columnFilters: FilterValues;
};

type ParentEntry = {
  parent: BalanceParentRow;
  children: BalanceChildRow[];
};

type ChildEntry = {
  parent: BalanceParentRow;
  child: BalanceChildRow;
};

const BALANCE_FILTERS: ColumnFilter[] = [
  { key: 'coin', label: 'Coin', type: 'text', placeholder: 'USDC, ETH, PLUME' },
  { key: 'venue', label: 'Venue', type: 'text', placeholder: 'bybit, wallet, hyperliquid' },
  { key: 'chain', label: 'Chain', type: 'text', placeholder: 'ethereum, bnb, sei' },
  { key: 'wallet', label: 'Wallet / Label', type: 'text', placeholder: 'treasury, cold, hot1' },
];

const DEFAULT_FILTERS: FilterState = {
  hideZero: true,
  logicalOnly: true,
  stableOnly: false,
  sortBy: 'mv',
  sortDir: 'desc',
  columnFilters: {},
};

const formatBalanceMvCell = (mvRaw: number | null | undefined, mvDisplay?: string | null): string => {
  const fallbackRaw = typeof mvDisplay === 'string'
    ? Number(mvDisplay.replace(/[$,]/g, ''))
    : Number.NaN;
  const value = Number.isFinite(mvRaw) ? Number(mvRaw) : fallbackRaw;
  if (!Number.isFinite(value)) return mvDisplay ?? '$0.00';

  const abs = Math.abs(value);
  const formatted = abs.toLocaleString(undefined, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  });
  const sign = value < 0 ? '-' : '';
  return `${sign}$${formatted}`;
};

const getStorageProfile = (): PathProfile => {
  if (typeof window === 'undefined') return 'default';
  return resolvePathnameProfile(window.location?.pathname);
};

const scopedStorageKey = (prefix: string, profile: PathProfile): string => `${prefix}:${profile}`;

const getStoredFilters = (): FilterState => {
  if (typeof window === 'undefined') return DEFAULT_FILTERS;
  try {
    const read = (key: string) => {
      const raw = window.localStorage.getItem(key);
      if (!raw) return null;
      return JSON.parse(raw);
    };

    const profile = getStorageProfile();
    const parsed =
      read(scopedStorageKey(FILTER_STORAGE_KEY_PREFIX, profile))
      ?? (profile === 'default'
        ? read(LEGACY_FILTER_STORAGE_KEY) ?? read(LEGACY_FILTER_STORAGE_KEY_V1)
        : null);
    if (!parsed) return DEFAULT_FILTERS;

    return {
      hideZero: parsed.hideZero ?? DEFAULT_FILTERS.hideZero,
      logicalOnly: parsed.logicalOnly ?? DEFAULT_FILTERS.logicalOnly,
      stableOnly: parsed.stableOnly ?? DEFAULT_FILTERS.stableOnly,
      sortBy: parsed.sortBy ?? DEFAULT_FILTERS.sortBy,
      sortDir: parsed.sortDir ?? DEFAULT_FILTERS.sortDir,
      columnFilters: parsed.columnFilters ?? {},
    } satisfies FilterState;
  } catch {
    return DEFAULT_FILTERS;
  }
};

type StoredExpandedState = {
  ids: Set<string>;
  hasStoredPreference: boolean;
};

const getStoredExpanded = (): StoredExpandedState => {
  if (typeof window === 'undefined') return { ids: new Set(), hasStoredPreference: false };
  try {
    const profile = getStorageProfile();
    const raw = (
      window.localStorage.getItem(scopedStorageKey(EXPANDED_STORAGE_KEY_PREFIX, profile))
      ?? (profile === 'default' ? window.localStorage.getItem(LEGACY_EXPANDED_STORAGE_KEY) : null)
    );
    if (!raw) return { ids: new Set(), hasStoredPreference: false };
    const parsed = JSON.parse(raw);
    if (Array.isArray(parsed)) {
      return { ids: new Set(parsed.map(String)), hasStoredPreference: true };
    }
  } catch {
    /* ignore */
  }
  return { ids: new Set(), hasStoredPreference: false };
};

const buildChildTooltip = (child: BalanceChildRow): string => {
  const parts = [child.venue, child.label ?? child.wallet, shortAddress(child.address)]
    .filter(Boolean)
    .map(String);
  return parts.join(' / ');
};

const ZERO_GUARD = (value: number | null | undefined) => Math.abs(value ?? 0) >= DUST_THRESHOLD;
const HAS_VISIBLE_BALANCE_EXPOSURE = (
  row: Pick<BalanceParentRow | BalanceChildRow, 'mv_raw' | 'qty_raw'>,
) => ZERO_GUARD(row.mv_raw) || ZERO_GUARD(row.qty_raw);

type SortMetrics = {
  canonical: string;
  qty: number;
  mv: number;
  mark: number | null;
  lastTs: number | null;
};

const compareBySortKey = (a: SortMetrics, b: SortMetrics, sortKey: SortKey, dir: SortDir): number => {
  const base = (() => {
    switch (sortKey) {
      case 'coin':
        return a.canonical.localeCompare(b.canonical);
      case 'qty':
        return a.qty - b.qty;
      case 'time':
        return (a.lastTs ?? 0) - (b.lastTs ?? 0);
      case 'mark':
        return (a.mark ?? Number.NEGATIVE_INFINITY) - (b.mark ?? Number.NEGATIVE_INFINITY);
      case 'mv':
      default:
        return a.mv - b.mv;
    }
  })();
  return dir === 'asc' ? base : -base;
};

const parentComparator =
  (sortKey: SortKey, dir: SortDir) => (a: ParentEntry, b: ParentEntry) =>
    compareBySortKey(
      {
        canonical: a.parent.canonical,
        qty: a.parent.qty_raw ?? 0,
        mv: a.parent.mv_raw ?? 0,
        mark: a.parent.mark_raw ?? null,
        lastTs: a.parent.last_ts ?? null,
      },
      {
        canonical: b.parent.canonical,
        qty: b.parent.qty_raw ?? 0,
        mv: b.parent.mv_raw ?? 0,
        mark: b.parent.mark_raw ?? null,
        lastTs: b.parent.last_ts ?? null,
      },
      sortKey,
      dir,
    );

const childComparator =
  (sortKey: SortKey, dir: SortDir) => (a: ChildEntry, b: ChildEntry) =>
    compareBySortKey(
      {
        canonical: a.parent.canonical,
        qty: a.child.qty_raw ?? 0,
        mv: a.child.mv_raw ?? 0,
        mark: a.child.mark_raw ?? null,
        lastTs: a.child.last_ts ?? null,
      },
      {
        canonical: b.parent.canonical,
        qty: b.child.qty_raw ?? 0,
        mv: b.child.mv_raw ?? 0,
        mark: b.child.mark_raw ?? null,
        lastTs: b.child.last_ts ?? null,
      },
      sortKey,
      dir,
    );

const SortIndicator = ({ active, dir }: { active: boolean; dir: SortDir }) => {
  if (!active) {
    return <ChevronsUpDown className="h-3 w-3 text-text-muted" aria-hidden="true" />;
  }
  return dir === 'asc' ? (
    <ChevronUp className="h-3 w-3 text-text-muted" aria-hidden="true" />
  ) : (
    <ChevronDown className="h-3 w-3 text-text-muted" aria-hidden="true" />
  );
};

export default function Balances({
  dense = false,
  className = '',
  onRemove,
  showHeader = true,
}: {
  dense?: boolean;
  className?: string;
  onRemove?: () => void;
  showHeader?: boolean;
} = {}) {
  const rows = useBalancesStore(selectBalancesRows, shallow);
  const totals = useBalancesStore(selectBalancesTotals);
  const loading = useBalancesStore(selectBalancesLoading);
  const riskGroups = useBalancesStore(selectBalancesRiskGroups, shallow);
  const riskSort = useBalancesStore(selectBalancesRiskSort);
  const setData = useBalancesStore((state) => state.setData);
  const setLoading = useBalancesStore((state) => state.setLoading);
  const setRiskSort = useBalancesStore((state) => state.setRiskSort);

  const [filters, setFilters] = useState<FilterState>(getStoredFilters);
  const storedExpanded = useMemo(getStoredExpanded, []);
  const [expanded, setExpanded] = useState<Set<string>>(storedExpanded.ids);
  const [hasExpandedPreference, setHasExpandedPreference] = useState(storedExpanded.hasStoredPreference);
  const [lastOkMs, setLastOkMs] = useState<number | null>(null);
  const [mode, setMode] = useState<'holdings' | 'risk'>('holdings');
  const [riskSearch, setRiskSearch] = useState('');
  const [riskNonZeroOnly, setRiskNonZeroOnly] = useState(false);
  const [selectedRiskKey, setSelectedRiskKey] = useState<string | null>(null);
  const [selectedRiskLabel, setSelectedRiskLabel] = useState<string | null>(null);
  const { isMobile } = useMobileLayout();

  const renderMarkCell = (symbol: string, mark: number | null | undefined) => {
    const formatted = formatMark(symbol, mark);
    return (
      <span className="inline-flex items-center justify-end gap-1 font-semibold text-zinc-300">
        {formatted}
      </span>
    );
  };

  const abortRef = useRef<AbortController | null>(null);
  const inFlight = useRef(false);

  const persistFilters = useCallback((state: FilterState) => {
    if (typeof window === 'undefined') return;
    try {
      window.localStorage.setItem(
        scopedStorageKey(FILTER_STORAGE_KEY_PREFIX, getStorageProfile()),
        JSON.stringify(state),
      );
    } catch {
      /* ignore persistence errors */
    }
  }, []);

  const persistExpanded = useCallback((ids: Set<string>) => {
    if (typeof window === 'undefined') return;
    try {
      window.localStorage.setItem(
        scopedStorageKey(EXPANDED_STORAGE_KEY_PREFIX, getStorageProfile()),
        JSON.stringify(Array.from(ids)),
      );
    } catch {
      /* ignore persistence errors */
    }
  }, []);

  const fetchBalances = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;

    abortRef.current?.abort();
    const ac = new AbortController();
    abortRef.current = ac;

    setLoading(true);
    try {
      const data = await api.getBalances();
      if (!ac.signal.aborted) {
        setData(data);
        setLastOkMs(Date.now());
      }
    } catch (error) {
      if (error instanceof Error && error.name === 'AbortError') return;
      console.error('[balances] Failed to load:', error);
    } finally {
      inFlight.current = false;
      if (!ac.signal.aborted) {
        setLoading(false);
      }
    }
  }, [setData, setLoading]);

  usePolling(fetchBalances, REFRESH_MS);

  useEffect(() => {
    return () => {
      abortRef.current?.abort();
    };
  }, []);

  useEffect(() => {
    persistFilters(filters);
  }, [filters, persistFilters]);

  useEffect(() => {
    persistExpanded(expanded);
  }, [expanded, persistExpanded]);

  const updateFilters = useCallback((partial: Partial<FilterState>) => {
    setFilters((prev) => ({ ...prev, ...partial }));
  }, []);

  const handleColumnFiltersChange = useCallback((next: FilterValues) => {
    setFilters((prev) => ({ ...prev, columnFilters: next }));
  }, []);

  const handleSortChange = useCallback((key: SortKey) => {
    setFilters((prev) => {
      const nextDir: SortDir =
        prev.sortBy === key ? (prev.sortDir === 'asc' ? 'desc' : 'asc') : 'asc';
      return { ...prev, sortBy: key, sortDir: nextDir };
    });
  }, []);

  const toggleExpanded = useCallback((id: string) => {
    setHasExpandedPreference(true);
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }, []);

  const baseParentEntries = useMemo<ParentEntry[]>(() => {
    return rows
      .filter((parent) => (filters.stableOnly ? parent.stable : true))
      .map((parent) => {
        const filteredChildren = parent.children.filter((child) => {
          if (selectedRiskKey && (child.risk_key ?? null) !== selectedRiskKey) {
            return false;
          }
          return filters.hideZero ? HAS_VISIBLE_BALANCE_EXPOSURE(child) : true;
        });
        return { parent, children: filteredChildren };
      })
      .filter(({ parent, children }) => {
        if (selectedRiskKey && children.length === 0) {
          return false;
        }
        if (!filters.hideZero) return true;
        return HAS_VISIBLE_BALANCE_EXPOSURE(parent) || children.length > 0;
      });
  }, [rows, filters.hideZero, filters.logicalOnly, filters.stableOnly, selectedRiskKey]);

  const filteredParentEntries = useMemo<ParentEntry[]>(() => {
    if (!filters.logicalOnly) return baseParentEntries;
    const filterable = baseParentEntries.map((entry) => ({
      key: entry.parent.id,
      coin: `${entry.parent.canonical} ${entry.parent.coin}`,
      venue: entry.children.map((c) => c.venue ?? '').join(' '),
      chain: entry.children.map((c) => c.chain ?? '').join(' '),
      wallet: entry.children.map((c) => c.wallet ?? c.label ?? '').join(' '),
      entry,
    }));
    const filtered = applyFilters(filterable, filters.columnFilters, { columns: BALANCE_FILTERS });
    return filtered.map((item) => item.entry);
  }, [baseParentEntries, filters.columnFilters, filters.logicalOnly]);

  const sortedParentEntries = useMemo<ParentEntry[]>(() => {
    const list = [...filteredParentEntries];
    list.sort(parentComparator(filters.sortBy, filters.sortDir));
    return list;
  }, [filteredParentEntries, filters.sortBy, filters.sortDir]);

  const sortedChildEntries = useMemo<ChildEntry[]>(() => {
    if (filters.logicalOnly) return [];
    const filterable: Array<{
      coin: string;
      venue: string;
      chain: string;
      wallet: string;
      entry: ChildEntry;
    }> = [];
    baseParentEntries.forEach(({ parent, children }) => {
      children.forEach((child) => {
        filterable.push({
          coin: `${child.coin} ${parent.canonical}`,
          venue: child.venue ?? '',
          chain: child.chain ?? '',
          wallet: child.wallet ?? child.label ?? '',
          entry: { parent, child },
        });
      });
    });
    const filtered = applyFilters(filterable, filters.columnFilters, { columns: BALANCE_FILTERS });
    const entries = filtered.map((item) => item.entry);
    entries.sort(childComparator(filters.sortBy, filters.sortDir));
    return entries;
  }, [baseParentEntries, filters.columnFilters, filters.logicalOnly, filters.sortBy, filters.sortDir]);

  const expandableParentIds = useMemo(() => {
    if (!filters.logicalOnly) return [] as string[];
    return filteredParentEntries
      .filter(({ children }) => children.length > 0)
      .map(({ parent }) => parent.id);
  }, [filters.logicalOnly, filteredParentEntries]);

  const allExpanded = useMemo(() => {
    if (!expandableParentIds.length) return false;
    return expandableParentIds.every((id) => expanded.has(id));
  }, [expandableParentIds, expanded]);

  useEffect(() => {
    if (hasExpandedPreference) return;
    if (!filters.logicalOnly) return;
    if (!expandableParentIds.length) return;
    setExpanded(new Set(expandableParentIds));
    setHasExpandedPreference(true);
  }, [expandableParentIds, filters.logicalOnly, hasExpandedPreference]);

  const handleToggleExpandAll = useCallback(() => {
    if (!expandableParentIds.length) return;
    setHasExpandedPreference(true);
    setExpanded((prev) => {
      const next = new Set(prev);
      if (allExpanded) {
        expandableParentIds.forEach((id) => next.delete(id));
      } else {
        expandableParentIds.forEach((id) => next.add(id));
      }
      return next;
    });
  }, [allExpanded, expandableParentIds, setExpanded]);

  const exportRows = useMemo(() => {
    if (filters.logicalOnly) {
      return sortedParentEntries.map(({ parent }) => ({
        coin: parent.canonical,
        qty: parent.qty_raw,
        mv: parent.mv_raw,
        mark: parent.mark_raw ?? '',
        last_ts: parent.last_ts ?? '',
        time_iso: parent.time_iso ?? '',
      }));
    }
    return sortedChildEntries.map(({ parent, child }) => ({
      parent: parent.canonical,
      coin: child.coin,
      form: child.form ?? '',
      chain: child.chain ?? '',
      venue: child.venue ?? '',
      wallet: child.wallet ?? '',
      label: child.label ?? '',
      address: child.address ?? '',
      contract: child.contract ?? '',
      qty: child.qty_raw,
      mv: child.mv_raw,
      mark: child.mark_raw ?? '',
      last_ts: child.last_ts ?? '',
      time_iso: child.time_iso ?? '',
    }));
  }, [filters.logicalOnly, sortedParentEntries, sortedChildEntries]);

  const visibleTotalRaw = useMemo(() => {
    if (filters.logicalOnly) {
      return sortedParentEntries.reduce((sum, entry) => sum + entry.parent.mv_raw, 0);
    }
    return sortedChildEntries.reduce((sum, entry) => sum + entry.child.mv_raw, 0);
  }, [filters.logicalOnly, sortedChildEntries, sortedParentEntries]);

  const visibleTotalDisplay = useMemo(() => formatMoney(visibleTotalRaw), [visibleTotalRaw]);

  const filtersApplied = useMemo(() => {
    if (!totals) return false;
    const authoritativeRaw = (
      totals.net_mv_raw
      ?? ((totals.mv_raw ?? 0) > 0 ? totals.mv_raw : null)
      ?? visibleTotalRaw
    );
    return Math.abs(authoritativeRaw - visibleTotalRaw) > 0.5;
  }, [totals, visibleTotalRaw]);

  const summaryTotals = useMemo(() => {
    if (!totals) return null;
    const available = [
      totals.net_mv_raw,
      totals.net_mv_display,
      totals.mv_raw,
      totals.mv_display,
      totals.gross_mv_raw,
      totals.gross_mv_display,
      totals.long_mv_raw,
      totals.long_mv_display,
      totals.short_mv_raw,
      totals.short_mv_display,
      totals.stable_mv_raw,
      totals.stable_mv_display,
      totals.non_stable_mv_raw,
      totals.non_stable_mv_display,
      totals.account_equity_raw,
      totals.account_equity_display,
      totals.withdrawable_raw,
      totals.withdrawable_display,
    ];
    if (available.every((val) => val === undefined || val === null)) {
      return null;
    }
    return totals;
  }, [totals]);

  const authoritativeNetDisplay = useMemo(() => {
    if (!totals) return null;
    if (totals.net_mv_display) return totals.net_mv_display;
    if ((totals.mv_raw ?? 0) > 0 && totals.mv_display) return totals.mv_display;
    return null;
  }, [totals]);

  const handleRiskSortChange = useCallback(
    (next: RiskSortState) => {
      setRiskSort(next.column, next.direction);
    },
    [setRiskSort],
  );

  const handleRiskRowClick = useCallback((riskKey: string, label: string) => {
    setMode('holdings');
    setSelectedRiskKey(riskKey || null);
    setSelectedRiskLabel(label || riskKey || null);
    setFilters((prev) => ({
      ...prev,
      logicalOnly: false,
    }));
  }, []);

  const nowMs = Date.now();

  const handleCopy = useCallback((value: string) => {
    if (!navigator.clipboard) {
      toast.error('Clipboard not available');
      return;
    }
    navigator.clipboard
      .writeText(value)
      .then(() => toast.success('Address copied'))
      .catch(() => toast.error('Failed to copy'));
  }, []);

  const renderAddressButton = (address?: string | null) => {
    const displayAddress = shortAddress(address);
    if (!address || !displayAddress) {
      return null;
    }
    return (
      <button
        type="button"
        className="ml-3 rounded px-1.5 py-0.5 text-xs font-mono text-zinc-600 transition-colors hover:bg-zinc-900 hover:text-zinc-400 focus-visible:outline focus-visible:outline-1 focus-visible:outline-offset-1 focus-visible:outline-zinc-700"
        aria-label="Copy wallet address"
        title={address}
        onClick={(event) => {
          event.stopPropagation();
          handleCopy(address);
        }}
      >
        {displayAddress}
      </button>
    );
  };

  const handleManualRefresh = useCallback(() => {
    fetchBalances();
  }, [fetchBalances]);

  const handleExportCSV = useCallback(() => {
    exportCSV(exportRows, generateTimestampFilename('balances', 'csv'));
  }, [exportRows]);

  const handleExportJSON = useCallback(() => {
    exportJSON(exportRows, generateTimestampFilename('balances', 'json'));
  }, [exportRows]);

  const panelHeaderSlots = usePanelHeaderSlots();

  const headerActions = useMemo(() => (
    <div className="flex items-center gap-2">
      <div className="inline-flex rounded border border-border bg-bg-surface/60">
        <Button
          variant={mode === 'holdings' ? 'secondary' : 'ghost'}
          size="xs"
          onClick={() => {
            setMode('holdings');
            setSelectedRiskKey(null);
            setSelectedRiskLabel(null);
          }}
          className="rounded-none border-0"
        >
          Holdings
        </Button>
        <Button
          variant={mode === 'risk' ? 'secondary' : 'ghost'}
          size="xs"
          onClick={() => setMode('risk')}
          className="rounded-none border-0"
        >
          Risk
        </Button>
      </div>

      {mode === 'holdings' && (
        <>
          <Button
            variant="ghost"
            size="xs"
            onClick={handleToggleExpandAll}
            title={allExpanded ? "Collapse all rows" : "Expand all rows"}
            disabled={!expandableParentIds.length || !filters.logicalOnly}
          >
            {allExpanded ? 'Collapse all' : 'Expand all'}
          </Button>
          <Button
            variant="ghost"
            size="xs"
            onClick={handleExportCSV}
            title="Export as CSV"
          >
            <Download size={14} />
          </Button>
          <Button
            variant="ghost"
            size="xs"
            onClick={handleExportJSON}
            title="Export as JSON"
          >
            <FileJson size={14} />
          </Button>
        </>
      )}
    </div>
  ), [
    mode,
    allExpanded,
    expandableParentIds,
    filters.logicalOnly,
    handleExportCSV,
    handleExportJSON,
    handleToggleExpandAll,
    setMode,
  ]);

  useEffect(() => {
    if (!panelHeaderSlots) return;
    if (showHeader) {
      panelHeaderSlots.setActions(null);
      panelHeaderSlots.setTitleActions(null);
      return;
    }
    panelHeaderSlots.setActions(headerActions);
    panelHeaderSlots.setTitleActions(null);
    return () => {
      panelHeaderSlots.setActions(null);
      panelHeaderSlots.setTitleActions(null);
    };
  }, [panelHeaderSlots, headerActions, showHeader]);

  const containerClasses = `flex flex-col h-full overflow-hidden ${className}`;

  const renderParentRow = (entry: ParentEntry) => {
    const { parent, children } = entry;
    const hasChildren = children.length > 0;
    const isExpanded = expanded.has(parent.id);
    const badge = parent.coin.endsWith('_LOGICAL');
    const groupMvs = children.reduce(
      (acc, child) => {
        const mv = child.mv_raw ?? 0;
        if (mv >= 0) acc.long += mv;
        else acc.short += mv;
        return acc;
      },
      { long: 0, short: 0 }
    );
    const groupGross = groupMvs.long - groupMvs.short;

    // Use Tailwind classes for row styling
    const rowClass = cn(
      "border-t transition-colors",
      isExpanded ? "bg-bg-surface" : "hover:bg-bg-hover"
    );

    const nestedChildRowClass = "border-t border-border/50 bg-bg-surface/30 hover:bg-bg-hover/60 text-sm";
    const paddingClass = dense ? "px-3 py-1" : "px-4 py-2";

    return (
      <tbody key={parent.id} className="group">
        <tr className={rowClass} style={{ borderTopColor: colors.border.DEFAULT }}>
          <td className={cn("text-left", paddingClass)}>
            <div className="flex items-center gap-3">
              {hasChildren ? (
                <button
                  type="button"
                  onClick={() => toggleExpanded(parent.id)}
                  className="flex h-5 w-5 items-center justify-center rounded border transition-colors"
                  style={{
                    borderColor: colors.border.DEFAULT,
                    backgroundColor: colors.bg.surface,
                    color: colors.text.secondary
                  }}
                >
                  <ChevronRight
                    className={cn("h-3 w-3 transition-transform", isExpanded && "rotate-90")}
                  />
                </button>
              ) : (
                <span className="inline-block h-5 w-5" />
              )}
              <div className="flex items-center gap-2">
                <span className="font-semibold text-sm" style={{ color: colors.text.primary }}>
                  {parent.canonical}
                </span>
                {badge && (
                  <span
                    className="rounded border px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide"
                    style={{
                      borderColor: colors.border.DEFAULT,
                      color: colors.text.muted
                    }}
                  >
                    Logical
                  </span>
                )}
              </div>
              {isMobile && (
                <div className="mt-2 flex flex-wrap gap-3 text-[11px] text-zinc-500">
                  <span className="flex items-center gap-1">
                    {renderMarkCell(parent.canonical, parent.mark_raw)}
                  </span>
                  <span className="flex items-center gap-1">
                    <DataAgeCell timestamp={parent.last_ts} />
                  </span>
                </div>
              )}
            </div>
          </td>
          <td className={cn("text-right font-mono tabular-nums", paddingClass)}>
            <span className="text-zinc-300">
              {formatQty(parent.canonical, parent.qty_raw, parent.mark_raw)}
            </span>
          </td>
          <td className={cn("text-right font-mono tabular-nums font-medium", paddingClass)}>
            <Tooltip
              content={
                hasChildren
                  ? `Long: ${formatMoney(groupMvs.long)} • Short: ${formatMoney(groupMvs.short)} • Gross: ${formatMoney(groupGross)}`
                  : undefined
              }
              disabled={!hasChildren}
            >
              <span className="text-zinc-300">{formatBalanceMvCell(parent.mv_raw, parent.mv_display)}</span>
            </Tooltip>
          </td>
          {!isMobile && (
            <td className={cn("text-right font-mono tabular-nums", paddingClass)}>
              {renderMarkCell(parent.canonical, parent.mark_raw)}
            </td>
          )}
          {!isMobile && (
            <td className={cn("text-right font-mono tabular-nums", paddingClass)}>
              <DataAgeCell timestamp={parent.last_ts} />
            </td>
          )}
        </tr>
        {isExpanded &&
          children.map((child) => (
            <tr
              key={child.id}
              className={nestedChildRowClass}
            >
              <td className={cn("text-left", paddingClass)}>
                <div className="ml-2 border-l border-zinc-800 pl-6">
                  <CoinCell
                    symbol={child.display_name_short ?? child.coin}
                    chain={(child as any).chain}
                    form={(child as any).form}
                    venue={child.venue}
                    walletLabel={child.wallet}
                    contract={(child as any).contract}
                    isChild={true}
                  />
                  {renderAddressButton(child.address)}
                  {isMobile && (
                    <div className="mt-1 flex flex-wrap gap-3 text-[10px] text-zinc-500">
                      <span>{renderMarkCell(child.inventory_asset ?? child.base_asset ?? child.coin, child.mark_raw)}</span>
                      <span className="flex items-center gap-1">
                        <DataAgeCell timestamp={child.last_ts} />
                      </span>
                    </div>
                  )}
                </div>
          </td>
          <td className={cn("text-right font-mono tabular-nums", paddingClass)}>
            <span className="text-zinc-300">
              {formatQty(child.inventory_asset ?? child.base_asset ?? child.coin, child.qty_raw, child.mark_raw)}
            </span>
          </td>
          <td className={cn("text-right font-mono tabular-nums", paddingClass)}>
            <span className="text-zinc-300">
              {formatBalanceMvCell(child.mv_raw, child.mv_display)}
            </span>
          </td>
              {!isMobile && (
                <td className={cn("text-right font-mono tabular-nums", paddingClass)}>
                  {renderMarkCell(child.inventory_asset ?? child.base_asset ?? child.coin, child.mark_raw)}
                </td>
              )}
              {!isMobile && (
                <td className={cn("text-right font-mono tabular-nums", paddingClass)}>
                  <DataAgeCell timestamp={child.last_ts} />
                </td>
              )}
            </tr>
          ))}
      </tbody>
    );
  };

  const renderChildRow = (entry: ChildEntry) => {
    const { parent, child } = entry;
    const paddingClass = dense ? "px-3 py-1" : "px-4 py-2";

    return (
      <tr
        key={`${parent.id}:${child.id}`}
        className="border-t border-zinc-800 hover:bg-zinc-900/50 transition-colors"
      >
        <td className={cn("text-left", paddingClass)}>
          <div className="ml-2 border-l border-zinc-800 pl-6">
          <CoinCell
            symbol={child.display_name_short ?? child.coin}
            chain={(child as any).chain}
            form={(child as any).form}
            venue={child.venue}
            walletLabel={child.wallet}
            contract={(child as any).contract}
            isChild={true}
          />
          {renderAddressButton(child.address)}
          {isMobile && (
            <div className="mt-1 flex flex-wrap gap-3 text-[10px] text-zinc-500">
              <span>{renderMarkCell(child.inventory_asset ?? child.base_asset ?? child.coin, child.mark_raw)}</span>
              <span className="flex items-center gap-1">
                <DataAgeCell timestamp={child.last_ts} />
              </span>
            </div>
          )}
        </div>
      </td>
      <td className={cn("text-right font-mono tabular-nums text-zinc-300", paddingClass)}>
        {formatQty(child.inventory_asset ?? child.base_asset ?? child.coin, child.qty_raw, child.mark_raw)}
      </td>
      <td className={cn("text-right font-mono tabular-nums text-zinc-300", paddingClass)}>
        {formatBalanceMvCell(child.mv_raw, child.mv_display)}
      </td>
      {!isMobile && (
        <td className={cn("text-right font-mono tabular-nums", paddingClass)}>
          {renderMarkCell(child.inventory_asset ?? child.base_asset ?? child.coin, child.mark_raw)}
        </td>
      )}
      {!isMobile && (
        <td className={cn("text-right font-mono tabular-nums", paddingClass)}>
          <DataAgeCell timestamp={child.last_ts} />
        </td>
      )}
    </tr>
  );
  };

  const renderBody = () => {
    const columnCount = isMobile ? 3 : 5;
    if (loading && sortedParentEntries.length === 0 && sortedChildEntries.length === 0) {
      return (
        <tbody>
          <tr>
            <td colSpan={columnCount} className="py-12 text-center text-sm text-zinc-500">
              Loading balances...
            </td>
          </tr>
        </tbody>
      );
    }

    if (sortedParentEntries.length === 0 && (!sortedChildEntries.length || filters.logicalOnly)) {
      return (
        <tbody>
          <tr>
            <td colSpan={columnCount} className="py-12 text-center text-sm text-zinc-500">
              No balances found
            </td>
          </tr>
        </tbody>
      );
    }

    if (filters.logicalOnly) {
      return <>{sortedParentEntries.map(renderParentRow)}</>;
    }

    return (
      <tbody>{sortedChildEntries.map(renderChildRow)}</tbody>
    );
  };

  const renderHeaderCell = (
    key: SortKey,
    label: string,
    opts: { align?: 'left' | 'right'; minWidth?: number } = {},
  ) => {
    const active = filters.sortBy === key;
    const justify = opts.align === 'right' ? 'justify-end' : 'justify-start';
    return (
      <th
        className={cn(opts.align === 'right' ? 'text-right' : 'text-left')}
        style={{ minWidth: opts.minWidth }}
        scope="col"
      >
        <button
          type="button"
          onClick={() => handleSortChange(key)}
          className={cn(
            'flex w-full items-center gap-2 text-xs font-semibold uppercase tracking-wide transition-colors',
            justify,
            'text-text-muted hover:text-text-primary'
          )}
          aria-sort={active ? (filters.sortDir === 'asc' ? 'ascending' : 'descending') : 'none'}
          aria-label={`Sort by ${label}`}
        >
          <span>{label}</span>
          <SortIndicator active={active} dir={filters.sortDir} />
        </button>
      </th>
    );
  };

  const content = (
    <div
      className={containerClasses}
      style={{
        color: colors.text.secondary,
      }}
    >
      {showHeader && (
        <PanelHeader
          title="Balances"
          onRefresh={handleManualRefresh}
          refreshing={loading}
          lastUpdate={lastOkMs ?? undefined}
          staleThresholdMs={STALE_THRESHOLDS.SLOW}
          onRemove={onRemove}
          actions={headerActions}
        />
      )}
      {summaryTotals && (
        <div className="flex flex-wrap items-center gap-4 border-b border-border bg-bg-surface px-4 py-3 text-sm font-medium text-text-primary">
          {[
            { label: 'Net Equity (Σ MV)', raw: summaryTotals.net_mv_raw, display: summaryTotals.net_mv_display },
            { label: 'Gross MV', raw: summaryTotals.gross_mv_raw, display: summaryTotals.gross_mv_display },
            { label: 'Long', raw: summaryTotals.long_mv_raw, display: summaryTotals.long_mv_display },
            { label: 'Short', raw: summaryTotals.short_mv_raw, display: summaryTotals.short_mv_display },
            { label: 'Stables', raw: summaryTotals.stable_mv_raw, display: summaryTotals.stable_mv_display },
            { label: 'Non-stables (net)', raw: summaryTotals.non_stable_mv_raw, display: summaryTotals.non_stable_mv_display },
            { label: 'Account Equity', raw: summaryTotals.account_equity_raw, display: summaryTotals.account_equity_display },
            { label: 'Withdrawable', raw: summaryTotals.withdrawable_raw, display: summaryTotals.withdrawable_display },
          ]
            .filter((item) => item.display != null || item.raw != null)
            .map((item) => (
            <div key={item.label} className="flex items-center gap-2 whitespace-nowrap">
              <span className="text-xs uppercase tracking-wide text-text-muted">{item.label}</span>
              <span className="font-semibold">{item.display ?? formatMoney(item.raw ?? 0)}</span>
            </div>
          ))}
        </div>
      )}
      {mode === 'holdings' ? (
        <>
          <TableFilter
            columns={BALANCE_FILTERS}
            onFilterChange={handleColumnFiltersChange}
            value={filters.columnFilters}
            dense={dense}
            customControls={(
              <div className="flex flex-wrap items-center gap-3 text-text-muted">
                <Switch
                  size="sm"
                  checked={filters.hideZero}
                  onCheckedChange={(value) => updateFilters({ hideZero: value })}
                  label="Hide zero"
                />
                <Switch
                  size="sm"
                  checked={filters.logicalOnly}
                  onCheckedChange={(value) => updateFilters({ logicalOnly: value })}
                  label="Logical only"
                />
              <Switch
                size="sm"
                checked={filters.stableOnly}
                onCheckedChange={(value) => updateFilters({ stableOnly: value })}
                label="Stables only"
              />
              {selectedRiskKey && (
                <Button
                  variant="ghost"
                  size="xs"
                  onClick={() => {
                    setSelectedRiskKey(null);
                    setSelectedRiskLabel(null);
                  }}
                >
                  Risk: {selectedRiskLabel ?? selectedRiskKey}
                </Button>
              )}
            </div>
          )}
        />
          <PanelBody className="bg-bg-surface">
            <TooltipProvider>
              <table className="terminal-table min-w-full text-sm">
                <thead>
                  <tr>
                    {renderHeaderCell('coin', 'Coin', { align: 'left', minWidth: COLUMN_MAP.coin.min })}
                    {renderHeaderCell('qty', 'Qty', { align: 'right', minWidth: COLUMN_MAP.qty.min })}
                    {renderHeaderCell('mv', 'MV', { align: 'right', minWidth: COLUMN_MAP.mv.min })}
                    {!isMobile && (
                      <>
                        {renderHeaderCell('mark', 'Mark', { align: 'right', minWidth: COLUMN_MAP.mark.min })}
                        {renderHeaderCell('time', 'Time', { align: 'right', minWidth: COLUMN_MAP.time.min })}
                      </>
                    )}
                  </tr>
                </thead>
                {renderBody()}
              </table>
            </TooltipProvider>
          </PanelBody>
        </>
      ) : (
        <PanelBody className="bg-bg-surface">
          <div className="flex flex-wrap items-end justify-between gap-3 border-b border-border px-4 py-3">
            <div className="w-full max-w-sm">
              <label className="mb-1 block text-xs font-semibold uppercase tracking-wide text-text-muted">
                Underlying filter
              </label>
              <div className="relative">
                <input
                  type="text"
                  value={riskSearch}
                  onChange={(e) => setRiskSearch(e.target.value)}
                  placeholder="Filter underlying (e.g., NVDA, GOOGL)"
                  className="w-full rounded border border-border bg-bg-surface px-3 py-2 pr-8 text-sm text-text-primary focus:outline-none focus:ring-2 focus:ring-border-strong"
                />
                {riskSearch && (
                  <button
                    type="button"
                    onClick={() => setRiskSearch('')}
                    className="absolute right-2 top-1/2 -translate-y-1/2 text-xs text-text-muted transition-colors hover:text-text-primary"
                    aria-label="Clear risk filter"
                  >
                    ✕
                  </button>
                )}
              </div>
            </div>
            <div className="flex items-center gap-3">
              <Switch
                size="sm"
                checked={riskNonZeroOnly}
                onCheckedChange={(value) => setRiskNonZeroOnly(Boolean(value))}
                label="Non-zero gross only"
              />
              <span className="text-xs text-text-muted">
                {riskGroups?.length ?? 0} underlyings
              </span>
            </div>
          </div>
          <RiskTable
            rows={riskGroups || []}
            search={riskSearch}
            nonZeroOnly={riskNonZeroOnly}
            sort={riskSort as RiskSortState}
            onSortChange={handleRiskSortChange}
            onRowClick={handleRiskRowClick}
          />
        </PanelBody>
      )}
      <div className="border-t border-border bg-bg-surface/80 px-4 py-2 text-right text-sm backdrop-blur">
        <span className="font-medium text-text-primary">
          Net Equity (Σ MV): {authoritativeNetDisplay ?? visibleTotalDisplay}
        </span>
        {filtersApplied && totals && (
          <span className="ml-2 text-xs text-text-muted">
            (Global {totals.net_mv_display ?? totals.mv_display})
          </span>
        )}
      </div>
    </div>
  );

  if (showHeader) {
    return (
      <PageShell>
        {content}
      </PageShell>
    );
  }

  return content;
}
