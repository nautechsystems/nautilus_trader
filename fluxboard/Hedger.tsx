import { FormEvent, ReactNode, useCallback, useEffect, useState, type CSSProperties } from 'react';
import { toast } from 'sonner';
import { api } from './api';
import type {
  HedgerGeometry,
  HedgerSnapshot,
  HedgerStatus,
  HedgerThresholdOverrides,
  HedgerThresholds,
  HedgerConfig,
} from './types';
import { INTERVALS } from './constants';
import { usePolling } from './hooks';
import { Button } from './components/ui/button/Button';
import { formatDecimal, fmtBalanceMark, fmtBalanceMV, fmtPrice } from './utils';
import { StatusPill } from './components/shared/StatusPill';
import { cn } from './lib/utils';
import { colors as tokenColors } from './lib/tokens';
import { PageShell } from './components/layout/PageShell';
import { Dialog, DialogFooter } from './components/ui/dialog/Dialog';

const fallbackColors = {
  text: {
    primary: '#e6e7ea',
    secondary: '#c2c4c8',
    muted: '#80838b',
  },
  bg: {
    surface: '#101112',
    base: '#0b0b0c',
    hover: '#151618',
  },
  border: {
    DEFAULT: '#1f2024',
  },
  semantic: {
    danger: {
      DEFAULT: '#c64c58',
    },
  },
} as const;

const colors = tokenColors ?? fallbackColors;

const formatPlumePerEthPrice = (value: string): string => {
  const num = Number(value);
  if (!Number.isFinite(num)) return value;
  try {
    return num.toLocaleString(undefined, { minimumFractionDigits: 1, maximumFractionDigits: 4 });
  } catch {
    return value;
  }
};

const formatQty = (value: string, decimals: number): string => {
  const num = Number(value);
  if (!Number.isFinite(num)) return value;
  try {
    return num.toLocaleString(undefined, {
      minimumFractionDigits: decimals,
      maximumFractionDigits: decimals,
    });
  } catch {
    return value;
  }
};

const formatPercentValue = (value: string): string => {
  if (!value) return '-';
  return `${formatDecimal(value, 2)}%`;
};

const formatUsdDisplay = (value?: string | number | null): string => {
  if (value === null || value === undefined || value === '') return '-';
  const formatted = fmtBalanceMV(value);
  return formatted === '' ? '$0' : formatted;
};

const formatPercentDisplay = (value?: string | number | null, digits = 1): string => {
  const num = parseNumber(value ?? null);
  if (num === null) return '-';
  return `${num.toFixed(digits)}%`;
};

const formatTimestampLabel = (value: number | null | undefined): string => {
  if (!value) return '—';
  try {
    return new Date(value * 1000).toLocaleString([], {
      year: 'numeric',
      month: '2-digit',
      day: '2-digit',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
  } catch (error) {
    return '—';
  }
};

const renderExposureRow = (
  label: string,
  qty: string | undefined,
  usd: string | undefined,
  decimals: number,
  emphasize = false
) => (
  <div
    className="flex items-center justify-between gap-2"
    key={label}
  >
    <span
      className={cn(
        "text-sm",
        emphasize ? "font-semibold" : "font-medium"
      )}
      style={{ color: emphasize ? colors.text.primary : colors.text.muted }}
    >
      {label}
    </span>
    <span
      className={cn(
        "text-right text-sm",
        emphasize ? "font-semibold" : "font-normal"
      )}
      style={{ color: emphasize ? colors.text.primary : colors.text.secondary }}
    >
      <div className="font-mono tabular-nums">{formatQty(qty ?? '0', decimals)}</div>
      <div className="text-[10px]" style={{ color: colors.text.muted }}>{formatUsdDisplay(usd)}</div>
    </span>
  </div>
);

const renderConfigRow = (label: string, value: ReactNode, hint?: ReactNode) => (
  <div className="flex justify-between gap-2" key={label}>
    <dt className="text-sm" style={{ color: colors.text.muted }}>
      {label}
    </dt>
    <dd className="text-right" style={{ color: colors.text.secondary }}>
      <div className="text-sm leading-normal">
        {value}
      </div>
      {hint ? <div className="text-[10px]" style={{ color: colors.text.muted }}>{hint}</div> : null}
    </dd>
  </div>
);

const isEditedValue = (effective?: string | null, base?: string | null): boolean => {
  if (!effective || !base) return false;
  return String(effective) !== String(base);
};

const parseNumber = (value?: string | number | null): number | null => {
  if (value === null || value === undefined) return null;
  const num = typeof value === 'number' ? value : Number(value);
  if (!Number.isFinite(num)) return null;
  return num;
};

type PriceThresholdForm = Pick<HedgerThresholds, 'price_move_pct'>;

export default function Hedger() {
  const [selectedHedgerId, setSelectedHedgerId] = useState<string>('eth_plume_lp');
  const [hedgerInstances, setHedgerInstances] = useState<
    { id: string; label?: string | null; token0_symbol?: string | null; token1_symbol?: string | null; api_key_hint?: string | null }[]
  >([]);
  const [status, setStatus] = useState<HedgerStatus | null>(null);
  const [lastSnapshot, setLastSnapshot] = useState<HedgerSnapshot | null>(null);
  const [loading, setLoading] = useState(false);
  const [geometryEditorOpen, setGeometryEditorOpen] = useState(false);
  const [geometrySaving, setGeometrySaving] = useState(false);
  const [geometryForm, setGeometryForm] = useState<HedgerGeometry>({
    initial_eth: '',
    initial_plume: '',
    price_lower: '',
    price_upper: '',
  });
  const [thresholdEditorOpen, setThresholdEditorOpen] = useState(false);
  const [thresholdSaving, setThresholdSaving] = useState(false);
  const [thresholdForm, setThresholdForm] = useState<PriceThresholdForm>({
    price_move_pct: '',
  });
  const [hedgerToggleBusy, setHedgerToggleBusy] = useState(false);
  const [eventsClearing, setEventsClearing] = useState(false);
  const [configEditorOpen, setConfigEditorOpen] = useState(false);
  const [configForm, setConfigForm] = useState<HedgerConfig | null>(null);
  const [configLoading, setConfigLoading] = useState(false);
  const [configSaving, setConfigSaving] = useState(false);
  const [configError, setConfigError] = useState<string | null>(null);

  useEffect(() => {
    let mounted = true;
    api.listHedgerInstances()
      .then(list => {
        if (!mounted) return;
        setHedgerInstances(list);
        if (list.length > 0 && !list.find(item => item.id === selectedHedgerId)) {
          setSelectedHedgerId(list[0].id);
        }
      })
      .catch(err => {
        if (import.meta.env?.DEV) {
          // eslint-disable-next-line no-console
          console.error('Failed to load hedger instances', err);
        }
      });
    return () => {
        mounted = false;
    };
  }, [selectedHedgerId]);

  const loadStatus = useCallback(async () => {
    try {
      const data = await api.getHedgerStatusById(selectedHedgerId);
      setStatus(data);
       const snap = (data as HedgerStatus | null)?.snapshot ?? null;
       if (snap && typeof snap === 'object') {
         setLastSnapshot(snap as HedgerSnapshot);
       }
    } catch (err) {
      if (import.meta.env?.DEV) {
        // eslint-disable-next-line no-console
        console.error('Failed to load hedger status', err);
      }
    }
  }, [selectedHedgerId]);

  usePolling(
    async () => {
      if (loading) return;
      setLoading(true);
      try {
        await loadStatus();
      } finally {
        setLoading(false);
      }
    },
    INTERVALS.HEDGER_POLL,
    true
  );

  const jobStatus = status?.job_status ?? 'unknown';
  const isRunning = jobStatus === 'running';
  const snapshot = (() => {
    const raw = status?.snapshot ?? null;
    if (raw && typeof raw === 'object') {
      return raw as HedgerSnapshot;
    }
    if (isRunning && lastSnapshot) {
      return lastSnapshot;
    }
    return raw;
  })();
  const isEthPlume = selectedHedgerId === 'eth_plume_lp' || selectedHedgerId === 'eth_plume_lp_band2';
  const isEthBand2 = selectedHedgerId === 'eth_plume_lp_band2';
  const instanceMeta = hedgerInstances.find(item => item.id === selectedHedgerId);
  const lastTickTs = status?.last_tick_ts ?? snapshot?.timestamp ?? null;
  const isDryRun = Boolean(
    status?.dry_run ??
      (snapshot && typeof snapshot === 'object' ? (snapshot as any).dry_run : null)
  );
  const hedgerEnabled = status?.hedger_enabled ?? snapshot?.hedger_enabled ?? false;
  const configSummary = (status?.config_summary ?? {}) as Record<string, string | number | null>;
  const hedgerLabel =
    (configSummary.label as string | undefined) ||
    (instanceMeta?.label as string | undefined) ||
    'ETH/PLUME LP Hedger';
  const apiKeyHint =
    (configSummary.api_key_hint as string | undefined) ||
    (instanceMeta?.api_key_hint as string | undefined) ||
    null;
  const hedgerSubtitle = (() => {
    const token0 = (configSummary.token0_symbol as string | undefined) || (instanceMeta?.token0_symbol as string | undefined);
    const token1 = (configSummary.token1_symbol as string | undefined) || (instanceMeta?.token1_symbol as string | undefined);
    if (token0 && token1) {
      return `${token0}/${token1} LP hedge bot – manages exposure via perps.`;
    }
    return 'WETH/WPLUME LP hedge bot – manages ETH exposure via Bybit ETHUSDT perp.';
  })();
  const symbol0 = (configSummary.token0_symbol as string | undefined) || (instanceMeta?.token0_symbol as string | undefined) || (isEthPlume ? 'ETH' : 'Token0');
  const symbol1 = (configSummary.token1_symbol as string | undefined) || (instanceMeta?.token1_symbol as string | undefined) || (isEthPlume ? 'PLUME' : 'Token1');
  const perpSymbol0 =
    (configSummary.perp_symbol_token0 as string | null | undefined) ??
    (snapshot?.perp_symbol_token0 as string | null | undefined) ??
    (configSummary.eth_symbol as string | undefined);
  const perpSymbol1 =
    (configSummary.perp_symbol_token1 as string | null | undefined) ??
    (snapshot?.perp_symbol_token1 as string | null | undefined);
  const hedgeToken0 = Boolean((configSummary.hedge_token0 as boolean | undefined) ?? true);
  const hedgeToken1 = Boolean((configSummary.hedge_token1 as boolean | undefined) ?? true);
  const decimals0 =
    parseNumber((configSummary.token0_decimals as number | string | undefined) ?? snapshot?.token0_decimals ?? null) ??
    6;
  const decimals1 =
    parseNumber((configSummary.token1_decimals as number | string | undefined) ?? snapshot?.token1_decimals ?? null) ??
    2;
  const displayDecimals0 = Math.min(decimals0, 4);
  const displayDecimals1 = Math.min(decimals1, 4);
  const baseGeometry: HedgerGeometry = {
    initial_eth: String(configSummary.initial_eth ?? snapshot?.initial_eth_base ?? ''),
    initial_plume: String(configSummary.initial_plume ?? snapshot?.initial_plume_base ?? ''),
    price_lower: String(configSummary.price_lower ?? snapshot?.price_lower_base ?? ''),
    price_upper: String(configSummary.price_upper ?? snapshot?.price_upper_base ?? ''),
  };
  const geometryEffective: HedgerGeometry = (status?.geometry_effective ?? {
    initial_eth: snapshot?.initial_eth_effective ?? baseGeometry.initial_eth,
    initial_plume: snapshot?.initial_plume_effective ?? baseGeometry.initial_plume,
    price_lower: snapshot?.price_lower_effective ?? baseGeometry.price_lower,
    price_upper: snapshot?.price_upper_effective ?? baseGeometry.price_upper,
  }) as HedgerGeometry;

  const thresholdOverrides = (status?.threshold_overrides ?? {}) as HedgerThresholdOverrides;
  const thresholdEffective = (status?.threshold_effective ?? null) as HedgerThresholds | null;
  const priceMoveThresholdBase = String(
    configSummary.price_move_pct ?? snapshot?.price_move_pct_base ?? ''
  );
  const priceMoveThresholdEffective =
    thresholdEffective?.price_move_pct ?? snapshot?.price_move_pct_effective ?? priceMoveThresholdBase;
  const priceMoveThresholdOverride = thresholdOverrides.price_move_pct ?? '';
  const baseEthExposureThreshold = String(
    configSummary.eth_exposure_usd_threshold ?? snapshot?.eth_exposure_usd_threshold_base ?? ''
  );
  const basePlumeExposureThreshold = String(
    configSummary.plume_exposure_usd_threshold ?? snapshot?.plume_exposure_usd_threshold_base ?? ''
  );
  const effectiveEthExposureThreshold =
    thresholdEffective?.eth_exposure_usd_threshold ??
    snapshot?.eth_exposure_usd_threshold_effective ??
    baseEthExposureThreshold;
  const effectivePlumeExposureThreshold =
    thresholdEffective?.plume_exposure_usd_threshold ??
    snapshot?.plume_exposure_usd_threshold_effective ??
    basePlumeExposureThreshold;
  const overrideEthExposureThreshold = thresholdOverrides.eth_exposure_usd_threshold;
  const overridePlumeExposureThreshold = thresholdOverrides.plume_exposure_usd_threshold;
  const legacyUsdThresholdCopy = 'Not used - hedger only triggers on price move since last hedge';

  const lpEthUsd = snapshot?.lp_eth_usd ?? snapshot?.lp_token0_usd ?? '0';
  const lpPlumeUsd = snapshot?.lp_plume_usd ?? snapshot?.lp_token1_usd ?? '0';
  const perpEthUsd = snapshot?.perp_eth_usd ?? snapshot?.perp_token0_usd ?? '0';
  const perpPlumeUsd = snapshot?.perp_plume_usd ?? snapshot?.perp_token1_usd ?? '0';
  const netEthUsd = snapshot?.net_eth_usd ?? snapshot?.net_token0_usd ?? '0';
  const netPlumeUsd = snapshot?.net_plume_usd ?? snapshot?.net_token1_usd ?? '0';
  const minOrderEth = parseNumber(configSummary.min_order_qty_eth ?? snapshot?.min_order_qty_eth ?? null) ?? 0;
  const minOrderPlume = parseNumber(
    configSummary.min_order_qty_plume ?? snapshot?.min_order_qty_plume ?? null
  ) ?? 0;
  const currentNetEth = parseNumber(snapshot?.net_eth ?? snapshot?.net_token0 ?? null) ?? 0;
  const currentNetPlume = parseNumber(snapshot?.net_plume ?? snapshot?.net_token1 ?? null) ?? 0;
  const netEthLarge = Math.abs(currentNetEth) >= minOrderEth && minOrderEth > 0;
  const netPlumeLarge = Math.abs(currentNetPlume) >= minOrderPlume && minOrderPlume > 0;
  const totalLpUsd = snapshot?.total_lp_value_usd ?? '0';
  const totalPerpUsd = snapshot?.total_perp_notional_usd ?? '0';
  const netDeltaUsd = snapshot?.net_delta_value_usd ?? '0';
  const lpMixEthPct = parseNumber(snapshot?.lp_mix_eth_pct ?? null) ?? 0;
  const lpMixPlumePct = parseNumber(snapshot?.lp_mix_plume_pct ?? null) ?? 0;
  const rangePct = parseNumber(snapshot?.range_pct ?? null);
  const nearLowerBound = Boolean(snapshot?.near_lower_bound);
  const nearUpperBound = Boolean(snapshot?.near_upper_bound);
  const lastHedgeTs = status?.last_hedge_ts ?? null;
  const lastHedgePrice = snapshot?.last_hedge_price ?? status?.last_hedge_price ?? null;
  const hasHedge = lastHedgeTs !== null && lastHedgeTs !== undefined;
  const priceMoveSinceLastHedgePct = (() => {
    if (!snapshot || !hasHedge) return null;
    const current = parseNumber(snapshot.price_plume_per_eth ?? snapshot.price_token1_per_token0);
    const last = parseNumber(lastHedgePrice);
    if (current === null || last === null || last === 0) return null;
    return Math.abs(((current - last) / last) * 100);
  })();
  const priceMoveSinceLastHedgeDisplayValue =
    hasHedge && priceMoveSinceLastHedgePct !== null
      ? priceMoveSinceLastHedgePct.toString()
      : snapshot?.price_move_pct ?? '';
  const rangePositionDisplay = rangePct === null ? '-' : `${(rangePct * 100).toFixed(1)}%`;
  const rangeBadgeLabel = nearLowerBound ? 'Near Lower Bound' : nearUpperBound ? 'Near Upper Bound' : null;
  const token0Mark = parseNumber(snapshot?.eth_mark ?? snapshot?.token0_mark ?? null);
  const token1Mark = parseNumber(snapshot?.plume_mark ?? snapshot?.token1_mark ?? null);
  const priceToken1PerToken0 = parseNumber(
    snapshot?.price_token1_per_token0 ?? snapshot?.price_plume_per_eth ?? null
  );
  const priceToken0PerToken1 =
    priceToken1PerToken0 && priceToken1PerToken0 !== 0
      ? 1 / priceToken1PerToken0
      : token0Mark !== null && token1Mark ? token0Mark / token1Mark : null;
  const priceRows: { label: string; value: string }[] = [];
  if (snapshot) {
    if (isEthPlume) {
      priceRows.push(
        {
          label: `Perp ${symbol1}/${symbol0} (Bybit)`,
          value: formatPlumePerEthPrice(
            snapshot.price_plume_per_eth ?? snapshot.price_token1_per_token0 ?? ''
          ),
        },
        {
          label: `Pool ${symbol1}/${symbol0} (Rooster)`,
          value: snapshot.pool_price_plume_per_eth ?? snapshot.pool_price_token1_per_token0
            ? formatPlumePerEthPrice(
                snapshot.pool_price_plume_per_eth ?? snapshot.pool_price_token1_per_token0 ?? ''
              )
            : '-',
        }
      );
    } else {
      if (hedgeToken0 && perpSymbol0) {
        priceRows.push({
          label: `Perp ${symbol0}/${symbol1} (Bybit)`,
          value: priceToken1PerToken0 !== null ? formatPlumePerEthPrice(String(priceToken1PerToken0)) : '-',
        });
      }
      if (hedgeToken1 && perpSymbol1) {
        priceRows.push({
          label: `Perp ${symbol1}/${symbol0} (Bybit)`,
          value: priceToken0PerToken1 !== null ? formatPlumePerEthPrice(String(priceToken0PerToken1)) : '-',
        });
      }
      priceRows.push({
        label: `Pool ${symbol0}/${symbol1} (Rooster)`,
        value:
          snapshot.pool_price_token1_per_token0 ?? snapshot.pool_price_plume_per_eth
            ? formatPlumePerEthPrice(
                snapshot.pool_price_token1_per_token0 ?? snapshot.pool_price_plume_per_eth ?? ''
              )
            : '-',
      });
    }
    if (hasHedge) {
      priceRows.push(
        {
          label: 'Last Hedge Price',
          value: lastHedgePrice ? formatPlumePerEthPrice(lastHedgePrice) : '-',
        },
        {
          label: 'Move Since Last Hedge',
          value: formatPercentValue(priceMoveSinceLastHedgeDisplayValue),
        }
      );
    }
    priceRows.push(
      {
        label: `${symbol0} Mark (Bybit)`,
        value: fmtBalanceMark(snapshot.eth_mark ?? snapshot.token0_mark),
      },
      {
        label: `${symbol1} Mark (Bybit)`,
        value: fmtPrice(snapshot.plume_mark ?? snapshot.token1_mark),
      },
      {
        label: 'Price Bounds',
        value: `${formatPlumePerEthPrice(
          snapshot.price_lower_effective ?? baseGeometry.price_lower
        )} — ${formatPlumePerEthPrice(snapshot.price_upper_effective ?? baseGeometry.price_upper)}`,
      },
      {
        label: 'Range Position',
        value: rangePositionDisplay,
      },
      {
        label: 'Perp Price Source',
        value: snapshot.price_source || 'unknown',
      }
    );
  }

  const token0Error = snapshot?.eth_error ?? snapshot?.token0_error ?? '0';
  const token1Error = snapshot?.plume_error ?? snapshot?.token1_error ?? '0';
  const token0UsdError = snapshot ? Number(snapshot.eth_usd_error ?? snapshot.token0_usd_error ?? 0) : 0;
  const token1UsdError = snapshot ? Number(snapshot.plume_usd_error ?? snapshot.token1_usd_error ?? 0) : 0;
  const effectivePriceMoveThresholdNumber = parseNumber(priceMoveThresholdEffective ?? null) ?? 0;
  const currentPriceMoveNumber =
    priceMoveSinceLastHedgePct ?? parseNumber(snapshot?.price_move_pct ?? null) ?? 0;
  const priceMoveThresholdEdited = isEditedValue(priceMoveThresholdEffective, priceMoveThresholdBase);
  const hedgerShouldHedgeButDisabled =
    !hedgerEnabled &&
    effectivePriceMoveThresholdNumber > 0 && currentPriceMoveNumber >= effectivePriceMoveThresholdNumber;

  const formatGeometryValue = (value: string, decimals: number, isPrice = false): string => {
    if (!value) return '-';
    return isPrice ? formatPlumePerEthPrice(value) : formatQty(value, decimals);
  };

  const handleGeometryInput = (field: keyof HedgerGeometry, value: string) => {
    setGeometryForm(prev => ({ ...prev, [field]: value }));
  };

  const onOpenGeometryEditor = () => {
    setGeometryForm({
      initial_eth: geometryEffective.initial_eth || '',
      initial_plume: geometryEffective.initial_plume || '',
      price_lower: geometryEffective.price_lower || '',
      price_upper: geometryEffective.price_upper || '',
    });
    setGeometryEditorOpen(true);
  };

  const onOpenThresholdEditor = () => {
    setThresholdForm({
      price_move_pct: priceMoveThresholdEffective || '',
    });
    setThresholdEditorOpen(true);
  };

  const handlePriceThresholdInput = (value: string) => {
    setThresholdForm({ price_move_pct: value });
  };

  const saveGeometryOverrides = useCallback(
    async (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      setGeometrySaving(true);
      try {
        if (!isEthPlume) {
          toast.info('Geometry overrides are available for ETH/PLUME hedgers only.');
          setGeometryEditorOpen(false);
          return;
        }
        const payload = {
          initial_eth: geometryForm.initial_eth.trim(),
          initial_plume: geometryForm.initial_plume.trim(),
          price_lower: geometryForm.price_lower.trim(),
          price_upper: geometryForm.price_upper.trim(),
        };
        if (isEthBand2) {
          await api.setHedgerBand2GeometryOverrides(payload);
        } else {
          await api.setHedgerGeometryOverrides(payload);
        }
        toast.success(`${hedgerLabel} geometry updated`);
        await loadStatus();
        setGeometryEditorOpen(false);
      } catch (error) {
        const message = error instanceof Error ? error.message : 'Failed to update geometry';
        toast.error(message);
      } finally {
        setGeometrySaving(false);
      }
    },
    [geometryForm, hedgerLabel, isEthBand2, isEthPlume, loadStatus]
  );

  const resetGeometryOverrides = useCallback(async () => {
    setGeometrySaving(true);
    try {
      if (!isEthPlume) {
        toast.info('Geometry overrides are available for ETH/PLUME hedgers only.');
        return;
      }
      if (isEthBand2) {
        await api.clearHedgerBand2GeometryOverrides();
      } else {
        await api.clearHedgerGeometryOverrides();
      }
      toast.success(`${hedgerLabel} geometry reset to INI values`);
      await loadStatus();
      setGeometryEditorOpen(false);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to reset geometry';
      toast.error(message);
    } finally {
      setGeometrySaving(false);
    }
  }, [hedgerLabel, isEthBand2, isEthPlume, loadStatus]);

  const saveThresholdOverrides = useCallback(
    async (event: FormEvent<HTMLFormElement>) => {
      event.preventDefault();
      setThresholdSaving(true);
      try {
        if (!isEthPlume) {
          toast.info('Threshold overrides are available for ETH/PLUME hedgers only.');
          setThresholdEditorOpen(false);
          return;
        }
        const payload = {
          price_move_pct: thresholdForm.price_move_pct.trim(),
        };
        if (isEthBand2) {
          await api.setHedgerBand2ThresholdOverrides(payload);
        } else {
          await api.setHedgerThresholdOverrides(payload);
        }
        toast.success(`${hedgerLabel} exposure thresholds updated`);
        await loadStatus();
        setThresholdEditorOpen(false);
      } catch (error) {
        const message = error instanceof Error ? error.message : 'Failed to update thresholds';
        toast.error(message);
      } finally {
        setThresholdSaving(false);
      }
    },
    [hedgerLabel, isEthBand2, isEthPlume, thresholdForm, loadStatus]
  );

  const resetThresholdOverrides = useCallback(async () => {
    setThresholdSaving(true);
    try {
      if (!isEthPlume) {
        toast.info('Threshold overrides are available for ETH/PLUME hedgers only.');
        return;
      }
      if (isEthBand2) {
        await api.clearHedgerBand2ThresholdOverrides();
      } else {
        await api.clearHedgerThresholdOverrides();
      }
      toast.success(`${hedgerLabel} exposure thresholds reset to INI values`);
      await loadStatus();
      setThresholdEditorOpen(false);
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to reset thresholds';
      toast.error(message);
    } finally {
      setThresholdSaving(false);
    }
  }, [hedgerLabel, isEthBand2, isEthPlume, loadStatus]);

  const toggleHedgerEnabled = useCallback(async () => {
    if (hedgerToggleBusy) return;
    setHedgerToggleBusy(true);
    try {
      if (!isEthPlume) {
        toast.info('Enable/disable toggle is available for ETH/PLUME hedgers only.');
        return;
      }
      const nextState = isEthBand2
        ? await api.setHedgerBand2Enabled(!hedgerEnabled)
        : await api.setHedgerEnabled(!hedgerEnabled);
      toast.success(nextState.hedger_enabled ? `${hedgerLabel} enabled` : `${hedgerLabel} disabled`);
      await loadStatus();
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to toggle hedger';
      toast.error(message);
    } finally {
      setHedgerToggleBusy(false);
    }
  }, [hedgerLabel, hedgerToggleBusy, hedgerEnabled, isEthBand2, isEthPlume, loadStatus]);

  const restartHedger = useCallback(async () => {
    if (hedgerToggleBusy) return;
    setHedgerToggleBusy(true);
    try {
      await api.setHedgerJobStateById(selectedHedgerId, 'restart');
      toast.success(`${hedgerLabel} restart requested`);
      await loadStatus();
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to restart hedger';
      toast.error(message);
    } finally {
      setHedgerToggleBusy(false);
    }
  }, [hedgerLabel, hedgerToggleBusy, loadStatus, selectedHedgerId]);

  const clearRecentHedges = useCallback(async () => {
    if (eventsClearing) return;
    const confirmClear =
      typeof window !== 'undefined'
        ? window.confirm('Clear recent hedges? This only removes the UI log, not exchange history.')
        : true;
    if (!confirmClear) return;
    setEventsClearing(true);
    try {
      if (!isEthPlume) {
        toast.info('Event log clearing is available for ETH/PLUME hedgers only.');
        return;
      }
      if (isEthBand2) {
        await api.clearHedgerBand2Events();
      } else {
        await api.clearHedgerEvents();
      }
      toast.success('Recent hedges cleared');
      await loadStatus();
    } catch (error) {
      const message = error instanceof Error ? error.message : 'Failed to clear recent hedges';
      toast.error(message);
    } finally {
      setEventsClearing(false);
    }
  }, [eventsClearing, isEthBand2, isEthPlume, loadStatus]);

  return (
    <>
      <PageShell>
        <div className="h-full overflow-auto" style={{ color: colors.text.secondary }}>
          <div className="flex flex-col gap-6 px-4 py-4">
            <div className="flex flex-wrap items-center justify-between gap-6">
              <div className="flex flex-col gap-3">
                <div className="flex items-center gap-3">
                  <div className="flex flex-col">
                    <label
                      className="text-[11px] uppercase tracking-wide"
                      style={{ color: colors.text.muted }}
                      htmlFor="hedger-select"
                    >
                      Hedger
                    </label>
                    <select
                      id="hedger-select"
                      className="rounded-md border px-2 py-1 text-sm"
                      style={{
                        borderColor: colors.border.DEFAULT,
                        color: colors.text.primary,
                        backgroundColor: colors.bg.surface,
                      }}
                      value={selectedHedgerId}
                      onChange={e => setSelectedHedgerId(e.target.value)}
                    >
                      {(hedgerInstances.length
                        ? hedgerInstances
                        : [{ id: 'eth_plume_lp', label: 'ETH/PLUME LP Hedger' }]).map(item => (
                        <option key={item.id} value={item.id}>
                          {item.label || item.id}
                        </option>
                      ))}
                    </select>
                  </div>
                </div>
                <div>
                  <h1 className="text-lg font-semibold" style={{ color: colors.text.primary }}>
                    {hedgerLabel}
                  </h1>
                  <p className="text-xs" style={{ color: colors.text.muted }}>
                    {hedgerSubtitle}
                  </p>
                  {apiKeyHint ? (
                    <p className="text-[11px]" style={{ color: colors.text.muted }}>
                      Bybit key: {apiKeyHint}
                    </p>
                  ) : null}
                  <div className="mt-2 flex gap-2">
                    <Button
                      size="xs"
                      variant="secondary"
                      disabled={isEthPlume || configLoading}
                      onClick={async () => {
                        if (isEthPlume) return;
                        setConfigError(null);
                        setConfigLoading(true);
                        try {
                          const cfg = await api.getHedgerConfig(selectedHedgerId);
                          setConfigForm(cfg);
                          setConfigEditorOpen(true);
                        } catch (error) {
                          const message = error instanceof Error ? error.message : 'Failed to load config';
                          toast.error(message);
                        } finally {
                          setConfigLoading(false);
                        }
                      }}
                    >
                      Edit Config
                    </Button>
                  </div>
                </div>
              </div>
              <div className="flex flex-wrap items-center justify-end gap-2 text-right">
                {isDryRun && <StatusPill status="warning" label="Dry Run" size="xs" tone="subtle" />}
                <StatusPill
                  status={isRunning ? 'success' : 'muted'}
                  label={isRunning ? 'Running' : 'Stopped'}
                  size="xs"
                  tone="subtle"
                />
                <StatusPill
                  status={hedgerEnabled ? 'ok' : 'muted'}
                  label={hedgerEnabled ? 'Hedging On' : 'Hedging Off'}
                  size="xs"
                  tone="subtle"
                />
                <Button size="xs" variant="secondary" disabled={hedgerToggleBusy} onClick={restartHedger}>
                  Restart
                </Button>
                <Button
                  size="xs"
                  variant={hedgerEnabled ? 'destructive' : 'success'}
                  disabled={hedgerToggleBusy || !isEthPlume}
                  onClick={toggleHedgerEnabled}
                >
                  {hedgerEnabled ? 'Disable Hedger' : 'Enable Hedger'}
                </Button>
              </div>
            </div>

            <div className="flex flex-col gap-6">
              <div className="grid grid-cols-1 gap-5 lg:grid-cols-2 xl:grid-cols-3">
                {/* Exposure Card */}
                <div
                  className={cn(
                    'flex flex-col gap-4 rounded-xl border p-6',
                    hedgerShouldHedgeButDisabled ? 'border-amber-500/50 ring-1 ring-amber-500/20' : ''
                  )}
                  style={{
                    backgroundColor: colors.bg.surface,
                    borderColor: hedgerShouldHedgeButDisabled ? undefined : colors.border.DEFAULT,
                  }}
                >
                  <div className="flex items-center justify-between">
                    <h2 className="text-sm font-semibold" style={{ color: colors.text.primary }}>
                      Exposure
                    </h2>
                    {hedgerShouldHedgeButDisabled && (
                      <StatusPill
                        status="warning"
                        label="Threshold tripped"
                        subLabel="Hedger disabled"
                        size="xs"
                        tone="subtle"
                      />
                    )}
                  </div>
                  {snapshot ? (
                    <div className="flex flex-col gap-6 text-xs">
                      <div>
                        <div
                          className="text-[10px] uppercase tracking-wider font-medium"
                          style={{ color: colors.text.muted }}
                        >
                          Exposure
                        </div>
                        <div className="mt-2 flex flex-col gap-4">
                          <div>
                            <div className="text-sm font-medium" style={{ color: colors.text.primary }}>
                              {symbol0}
                            </div>
                            <div className="mt-1 flex flex-col gap-1">
                              {renderExposureRow(`LP ${symbol0}`, snapshot.lp_eth ?? snapshot.lp_token0, lpEthUsd, displayDecimals0)}
                              {renderExposureRow(
                                `Perp ${symbol0}`,
                                snapshot.perp_eth ?? snapshot.perp_token0,
                                perpEthUsd,
                                displayDecimals0
                              )}
                              {renderExposureRow(
                                `Net ${symbol0}`,
                                snapshot.net_eth ?? snapshot.net_token0,
                                netEthUsd,
                                displayDecimals0,
                                true
                              )}
                            </div>
                          </div>
                          <div>
                            <div className="text-sm font-medium" style={{ color: colors.text.primary }}>
                              {symbol1}
                            </div>
                            <div className="mt-1 flex flex-col gap-1">
                              {renderExposureRow(
                                `LP ${symbol1}`,
                                snapshot.lp_plume ?? snapshot.lp_token1,
                                lpPlumeUsd,
                                displayDecimals1
                              )}
                              {renderExposureRow(
                                `Perp ${symbol1}`,
                                snapshot.perp_plume ?? snapshot.perp_token1,
                                perpPlumeUsd,
                                displayDecimals1
                              )}
                              {renderExposureRow(
                                `Net ${symbol1}`,
                                snapshot.net_plume ?? snapshot.net_token1,
                                netPlumeUsd,
                                displayDecimals1,
                                true
                              )}
                            </div>
                          </div>
                        </div>
                      </div>

                      <div>
                        <div
                          className="text-[10px] uppercase tracking-wider font-medium"
                          style={{ color: colors.text.muted }}
                        >
                          Errors
                        </div>
                        <div className="mt-1 flex flex-col gap-1">
                          <div className="flex justify-between gap-2">
                            <span className="text-sm" style={{ color: colors.text.muted }}>
                              {symbol0} Error
                            </span>
                            <span
                              className={cn('text-right text-sm', netEthLarge ? 'text-amber-400' : '')}
                              style={{ color: netEthLarge ? undefined : colors.text.secondary }}
                            >
                              <div className="font-mono tabular-nums">
                                {formatQty(token0Error ?? '0', displayDecimals0)}
                              </div>
                              <div className="text-[10px]" style={{ color: colors.text.muted }}>
                                {formatUsdDisplay(token0UsdError)}
                              </div>
                            </span>
                          </div>
                          <div className="flex justify-between gap-2">
                            <span className="text-sm" style={{ color: colors.text.muted }}>
                              {symbol1} Error
                            </span>
                            <span
                              className={cn('text-right text-sm', netPlumeLarge ? 'text-amber-400' : '')}
                              style={{ color: netPlumeLarge ? undefined : colors.text.secondary }}
                            >
                              <div className="font-mono tabular-nums">
                                {formatQty(token1Error ?? '0', displayDecimals1)}
                              </div>
                              <div className="text-[10px]" style={{ color: colors.text.muted }}>
                                {formatUsdDisplay(token1UsdError)}
                              </div>
                            </span>
                          </div>
                        </div>
                      </div>

                      <div>
                        <div
                          className="text-[10px] uppercase tracking-wider font-medium"
                          style={{ color: colors.text.muted }}
                        >
                          Portfolio
                        </div>
                        <div className="mt-1 flex flex-col gap-1 text-sm">
                          <div className="flex items-center justify-between">
                            <span style={{ color: colors.text.muted }}>Total LP Value</span>
                            <span style={{ color: colors.text.secondary }}>{formatUsdDisplay(totalLpUsd)}</span>
                          </div>
                          <div className="flex items-center justify-between">
                            <span style={{ color: colors.text.muted }}>Perp Hedge Value</span>
                            <span style={{ color: colors.text.secondary }}>{formatUsdDisplay(totalPerpUsd)}</span>
                          </div>
                          <div className="flex items-center justify-between">
                            <span style={{ color: colors.text.muted }}>Net Delta Value</span>
                            <span style={{ color: colors.text.secondary }}>{formatUsdDisplay(netDeltaUsd)}</span>
                          </div>
                        </div>
                        <div
                          className="mt-1 flex justify-between pt-1 text-[10px]"
                          style={{ color: colors.text.muted }}
                        >
                          <span>Last tick: {formatTimestampLabel(lastTickTs)}</span>
                          <span>Last hedge: {formatTimestampLabel(lastHedgeTs)}</span>
                        </div>
                      </div>

                      <div className="text-[10px]" style={{ color: colors.text.muted }}>
                        {hedgerEnabled
                          ? 'Hedging active – bot will send Bybit orders when thresholds hit.'
                          : 'Hedging disabled – monitoring only, no Bybit orders.'}
                      </div>
                    </div>
                  ) : jobStatus === 'running' ? (
                    <p className="text-xs" style={{ color: colors.text.muted }}>
                      Running, awaiting first snapshot…
                    </p>
                  ) : (
                    <p className="text-xs" style={{ color: colors.text.muted }}>
                      Hedger is stopped.
                    </p>
                  )}
                </div>

                {/* Pricing Card */}
                <div
                  className="flex flex-col gap-4 rounded-xl border p-6"
                  style={{ backgroundColor: colors.bg.surface, borderColor: colors.border.DEFAULT }}
                >
                  <div className="flex items-center justify-between gap-2">
                    <h2 className="text-sm font-semibold" style={{ color: colors.text.primary }}>
                      Pricing
                    </h2>
                    {rangeBadgeLabel && <StatusPill status="warning" label={rangeBadgeLabel} size="xs" tone="subtle" />}
                  </div>
                  {snapshot ? (
                    <div className="flex flex-col gap-2 text-xs">
                      <dl className="flex flex-col gap-1">
                        {priceRows.map(({ label, value }) => (
                          <div className="flex justify-between gap-2" key={label}>
                            <dt style={{ color: colors.text.muted }}>{label}</dt>
                            <dd style={{ color: colors.text.secondary }}>{value}</dd>
                          </div>
                        ))}
                      </dl>

                      <div className="border-t pt-2 flex flex-col gap-1" style={{ borderColor: colors.border.DEFAULT }}>
                        <div className="flex items-center justify-between gap-2">
                          <span
                            className="text-[10px] uppercase tracking-wider"
                            style={{ color: colors.text.muted }}
                          >
                            LP Exposure Mix
                          </span>
                          <span className="text-xs" style={{ color: colors.text.secondary }}>
                            {symbol0} {formatPercentDisplay(lpMixEthPct)} · {symbol1} {formatPercentDisplay(lpMixPlumePct)}
                          </span>
                        </div>
                        <div className="h-2 w-full overflow-hidden rounded-full" style={{ backgroundColor: colors.bg.base }}>
                          <div
                            className="h-full rounded-full bg-emerald-500"
                            style={{ width: `${Math.min(Math.max(lpMixEthPct, 0), 100)}%` }}
                          />
                        </div>
                      </div>

                      <div
                        className="flex justify-between border-t pt-2 text-[10px]"
                        style={{ borderColor: colors.border.DEFAULT, color: colors.text.muted }}
                      >
                        <span>Last tick: {formatTimestampLabel(lastTickTs)}</span>
                        <span>Last hedge: {formatTimestampLabel(lastHedgeTs)}</span>
                      </div>
                    </div>
                  ) : jobStatus === 'running' ? (
                    <p className="text-xs" style={{ color: colors.text.muted }}>
                      Running, awaiting pricing snapshot…
                    </p>
                  ) : (
                    <p className="text-xs" style={{ color: colors.text.muted }}>
                      Hedger is stopped.
                    </p>
                  )}
                </div>

                {/* Config Card */}
                <div
                  className="flex flex-col gap-4 rounded-xl border p-6"
                  style={{ backgroundColor: colors.bg.surface, borderColor: colors.border.DEFAULT }}
                >
                  <div className="flex items-center justify-between gap-2">
                    <h2 className="text-sm font-semibold" style={{ color: colors.text.primary }}>
                      Config
                    </h2>
                    <div className="flex gap-1">
                      <Button size="xs" variant="secondary" onClick={onOpenThresholdEditor}>
                        Edit Thresholds
                      </Button>
                      <Button size="xs" variant="secondary" onClick={onOpenGeometryEditor}>
                        Edit Geometry
                      </Button>
                    </div>
                  </div>
                  {status?.config_summary ? (
                    <div className="flex flex-col gap-6 text-xs">
                      <div>
                        <div
                          className="text-[10px] uppercase tracking-wider font-medium"
                          style={{ color: colors.text.muted }}
                        >
                          Symbols
                        </div>
                        <dl className="mt-1 flex flex-col gap-1">
                          {renderConfigRow(
                            'Pool',
                            <span className="block max-w-[12rem] truncate">
                              {String(configSummary.pool_address || '')}
                            </span>
                          )}
                          {renderConfigRow(`LP Token0`, String(configSummary.token0_symbol || symbol0))}
                          {renderConfigRow(`LP Token1`, String(configSummary.token1_symbol || symbol1))}
                          {renderConfigRow(`Perp Token0`, perpSymbol0 || '—')}
                          {renderConfigRow(`Perp Token1`, perpSymbol1 || '—')}
                          {renderConfigRow(
                            'Price Move Threshold (%)',
                            <>
                              {formatPercentValue(priceMoveThresholdEffective)}
                              {priceMoveThresholdEdited && (
                                <span className="ml-1 text-[10px] text-amber-500">(edited)</span>
                              )}
                            </>,
                            priceMoveThresholdEdited
                              ? `Base: ${formatPercentValue(priceMoveThresholdBase)}`
                              : undefined
                          )}
                        </dl>
                      </div>

                      <div>
                        <div
                          className="text-[10px] uppercase tracking-wider font-medium"
                          style={{ color: colors.text.muted }}
                        >
                          Order Sizes
                        </div>
                        <dl className="mt-1 flex flex-col gap-1">
                          {renderConfigRow(`Min Order ${symbol0}`, String(configSummary.min_order_qty_eth || ''))}
                          {renderConfigRow(`Min Order ${symbol1}`, String(configSummary.min_order_qty_plume || ''))}
                          {renderConfigRow(`Qty Step ${symbol0}`, String(configSummary.qty_step_eth || ''))}
                          {renderConfigRow(`Qty Step ${symbol1}`, String(configSummary.qty_step_plume || ''))}
                        </dl>
                      </div>

                      <div>
                        <div
                          className="text-[10px] uppercase tracking-wider font-medium"
                          style={{ color: colors.text.muted }}
                        >
                          LP Geometry
                        </div>
                        <dl className="mt-1 flex flex-col gap-1">
                          {renderConfigRow(
                            `Initial ${symbol0}`,
                            <>
                              {formatGeometryValue(geometryEffective.initial_eth, decimals0)}
                              {isEditedValue(geometryEffective.initial_eth, baseGeometry.initial_eth) && (
                                <span className="ml-1 text-[10px] text-amber-500">(edited)</span>
                              )}
                            </>,
                            isEditedValue(geometryEffective.initial_eth, baseGeometry.initial_eth)
                              ? `Base: ${formatGeometryValue(baseGeometry.initial_eth, decimals0)}`
                              : undefined
                          )}
                          {renderConfigRow(
                            `Initial ${symbol1}`,
                            <>
                              {formatGeometryValue(geometryEffective.initial_plume, decimals1)}
                              {isEditedValue(geometryEffective.initial_plume, baseGeometry.initial_plume) && (
                                <span className="ml-1 text-[10px] text-amber-500">(edited)</span>
                              )}
                            </>,
                            isEditedValue(geometryEffective.initial_plume, baseGeometry.initial_plume)
                              ? `Base: ${formatGeometryValue(baseGeometry.initial_plume, decimals1)}`
                              : undefined
                          )}
                          {renderConfigRow(
                            'Price Lower',
                            <>
                              {formatGeometryValue(geometryEffective.price_lower, 4, true)}
                              {isEditedValue(geometryEffective.price_lower, baseGeometry.price_lower) && (
                                <span className="ml-1 text-[10px] text-amber-500">(edited)</span>
                              )}
                            </>,
                            isEditedValue(geometryEffective.price_lower, baseGeometry.price_lower)
                              ? `Base: ${formatGeometryValue(baseGeometry.price_lower, 4, true)}`
                              : undefined
                          )}
                          {renderConfigRow(
                            'Price Upper',
                            <>
                              {formatGeometryValue(geometryEffective.price_upper, 4, true)}
                              {isEditedValue(geometryEffective.price_upper, baseGeometry.price_upper) && (
                                <span className="ml-1 text-[10px] text-amber-500">(edited)</span>
                              )}
                            </>,
                            isEditedValue(geometryEffective.price_upper, baseGeometry.price_upper)
                              ? `Base: ${formatGeometryValue(baseGeometry.price_upper, 4, true)}`
                              : undefined
                          )}
                        </dl>
                      </div>
                    </div>
                  ) : (
                    <p className="text-xs" style={{ color: colors.text.muted }}>
                      Config summary unavailable.
                    </p>
                  )}
                </div>
              </div>

              {geometryEditorOpen && (
                <div
                  className="mt-6 flex flex-col gap-4 rounded-xl border p-6"
                  style={{ backgroundColor: colors.bg.surface, borderColor: colors.border.DEFAULT }}
                >
                  <h3 className="text-sm font-semibold" style={{ color: colors.text.primary }}>
                    Edit LP Geometry
                  </h3>
                  <form className="flex flex-col gap-4" onSubmit={saveGeometryOverrides}>
                    <div>
                      <label
                        className="mb-1 block text-xs"
                        style={{ color: colors.text.muted }}
                        htmlFor="geom-initial-eth"
                      >
                        Initial ETH
                      </label>
                      <input
                        id="geom-initial-eth"
                        type="text"
                        value={geometryForm.initial_eth}
                        onChange={event => handleGeometryInput('initial_eth', event.target.value)}
                        className="w-full rounded-md border px-3 py-2 text-sm focus:border-emerald-500 focus:outline-none"
                        style={{ backgroundColor: colors.bg.base, borderColor: colors.border.DEFAULT, color: colors.text.primary }}
                      />
                    </div>
                    <div>
                      <label
                        className="mb-1 block text-xs"
                        style={{ color: colors.text.muted }}
                        htmlFor="geom-initial-plume"
                      >
                        Initial PLUME
                      </label>
                      <input
                        id="geom-initial-plume"
                        type="text"
                        value={geometryForm.initial_plume}
                        onChange={event => handleGeometryInput('initial_plume', event.target.value)}
                        className="w-full rounded-md border px-3 py-2 text-sm focus:border-emerald-500 focus:outline-none"
                        style={{ backgroundColor: colors.bg.base, borderColor: colors.border.DEFAULT, color: colors.text.primary }}
                      />
                    </div>
                    <div>
                      <label
                        className="mb-1 block text-xs"
                        style={{ color: colors.text.muted }}
                        htmlFor="geom-price-lower"
                      >
                        Price Lower (PLUME/ETH)
                      </label>
                      <input
                        id="geom-price-lower"
                        type="text"
                        value={geometryForm.price_lower}
                        onChange={event => handleGeometryInput('price_lower', event.target.value)}
                        className="w-full rounded-md border px-3 py-2 text-sm focus:border-emerald-500 focus:outline-none"
                        style={{ backgroundColor: colors.bg.base, borderColor: colors.border.DEFAULT, color: colors.text.primary }}
                      />
                    </div>
                    <div>
                      <label
                        className="mb-1 block text-xs"
                        style={{ color: colors.text.muted }}
                        htmlFor="geom-price-upper"
                      >
                        Price Upper (PLUME/ETH)
                      </label>
                      <input
                        id="geom-price-upper"
                        type="text"
                        value={geometryForm.price_upper}
                        onChange={event => handleGeometryInput('price_upper', event.target.value)}
                        className="w-full rounded-md border px-3 py-2 text-sm focus:border-emerald-500 focus:outline-none"
                        style={{ backgroundColor: colors.bg.base, borderColor: colors.border.DEFAULT, color: colors.text.primary }}
                      />
                    </div>
                    <div className="flex flex-wrap gap-2">
                      <Button type="submit" size="xs" variant="default" disabled={geometrySaving}>
                        Save
                      </Button>
                      <Button
                        type="button"
                        size="xs"
                        variant="secondary"
                        disabled={geometrySaving}
                        onClick={resetGeometryOverrides}
                      >
                        Reset to INI
                      </Button>
                      <Button
                        type="button"
                        size="xs"
                        variant="secondary"
                        disabled={geometrySaving}
                        onClick={() => setGeometryEditorOpen(false)}
                      >
                        Cancel
                      </Button>
                    </div>
                  </form>
                </div>
              )}

              {thresholdEditorOpen && (
                <div
                  className="mt-6 flex flex-col gap-4 rounded-xl border p-6"
                  style={{ backgroundColor: colors.bg.surface, borderColor: colors.border.DEFAULT }}
                >
                  <h3 className="text-sm font-semibold" style={{ color: colors.text.primary }}>
                    Edit Exposure Thresholds
                  </h3>
                  <form className="flex flex-col gap-4" onSubmit={saveThresholdOverrides}>
                    <p className="text-xs" style={{ color: colors.text.muted }}>
                      USD exposure thresholds are legacy diagnostics only. Hedger will trigger once the absolute % move
                      since the last hedge exceeds this threshold.
                    </p>
                    <div>
                      <label
                        className="mb-1 block text-xs"
                        style={{ color: colors.text.muted }}
                        htmlFor="thresh-price-move"
                      >
                        Price Move % (abs since last hedge)
                      </label>
                      <input
                        id="thresh-price-move"
                        type="text"
                        value={thresholdForm.price_move_pct}
                        onChange={event => handlePriceThresholdInput(event.target.value)}
                        className="w-full rounded-md border px-3 py-2 text-sm focus:border-emerald-500 focus:outline-none"
                        style={{ backgroundColor: colors.bg.base, borderColor: colors.border.DEFAULT, color: colors.text.primary }}
                      />
                    </div>
                    <div className="flex flex-wrap gap-2">
                      <Button type="submit" size="xs" variant="default" disabled={thresholdSaving}>
                        Save
                      </Button>
                      <Button
                        type="button"
                        size="xs"
                        variant="secondary"
                        disabled={thresholdSaving}
                        onClick={resetThresholdOverrides}
                      >
                        Reset to INI
                      </Button>
                      <Button
                        type="button"
                        size="xs"
                        variant="secondary"
                        disabled={thresholdSaving}
                        onClick={() => setThresholdEditorOpen(false)}
                      >
                        Cancel
                      </Button>
                    </div>
                  </form>
                </div>
              )}

              <div
                className="mt-6 flex flex-col gap-4 rounded-xl border p-6"
                style={{ backgroundColor: colors.bg.surface, borderColor: colors.border.DEFAULT }}
              >
                <div className="flex items-center justify-between gap-2">
                  <h2 className="text-sm font-semibold" style={{ color: colors.text.primary }}>
                    Recent Hedges
                  </h2>
                  <Button
                    type="button"
                    size="xs"
                    variant="secondary"
                    onClick={clearRecentHedges}
                    disabled={eventsClearing}
                  >
                    Clear
                  </Button>
                </div>
                {status?.recent_events?.length ? (
                  <div className="overflow-x-auto">
                    <table className="min-w-full text-sm" style={{ color: colors.text.secondary }}>
                      <thead className="border-b" style={{ borderColor: colors.border.DEFAULT }}>
                        <tr>
                          {[
                            { label: 'Date / Time', align: 'left' },
                            { label: 'Asset', align: 'left' },
                            { label: 'Side', align: 'left' },
                            { label: 'Qty', align: 'right' },
                            { label: 'Net After', align: 'right' },
                            { label: 'USD Notional', align: 'right' },
                            { label: 'Net After (USD)', align: 'right' },
                            { label: 'Reason', align: 'left' },
                            { label: 'Source', align: 'left' },
                          ].map(({ label, align }) => (
                            <th
                              key={label}
                              className={cn(
                                'px-2 py-2 text-xs font-medium',
                                align === 'right' ? 'text-right' : 'text-left'
                              )}
                              style={{ color: colors.text.muted }}
                            >
                              {label}
                            </th>
                          ))}
                        </tr>
                      </thead>
                      <tbody>
                        {status.recent_events.map(ev => (
                          <tr
                            key={`${ev.timestamp}-${ev.side}-${ev.qty}`}
                            className="border-b transition-colors hover:bg-bg-hover"
                            style={{ borderColor: colors.border.DEFAULT }}
                          >
                            <td className="px-2 py-2 text-xs">{formatTimestampLabel(ev.timestamp)}</td>
                            <td className="px-2 py-2 text-xs">{ev.asset || '-'}</td>
                            <td className="px-2 py-2 text-xs">{ev.side}</td>
                            <td className="px-2 py-2 text-xs text-right font-mono tabular-nums">
                              {formatQty(ev.qty, ev.asset === symbol1 ? decimals1 : decimals0)}
                            </td>
                            <td className="px-2 py-2 text-xs text-right font-mono tabular-nums">
                              {formatQty(
                                ev.asset === symbol1 ? ev.net_plume_after ?? ev.net_token1_after ?? '0' : ev.net_eth_after ?? ev.net_token0_after ?? '0',
                                ev.asset === symbol1 ? decimals1 : decimals0
                              )}
                            </td>
                            <td className="px-2 py-2 text-xs text-right font-mono tabular-nums">
                              {formatUsdDisplay(ev.usd_notional)}
                            </td>
                            <td className="px-2 py-2 text-xs text-right font-mono tabular-nums">
                              {formatUsdDisplay(ev.net_after_usd)}
                            </td>
                            <td className="px-2 py-2 text-xs">{ev.trigger_reason || '-'}</td>
                            <td className="px-2 py-2 text-xs">{ev.price_source || 'unknown'}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                ) : (
                  <p className="text-xs" style={{ color: colors.text.muted }}>
                    No hedges recorded yet.
                  </p>
                )}
              </div>
            </div>
          </div>
        </div>
      </PageShell>

      <Dialog
        isOpen={configEditorOpen}
        onClose={() => setConfigEditorOpen(false)}
        title="Edit Hedger Config"
        size="lg"
      >
        {configLoading ? (
          <p className="text-sm" style={{ color: colors.text.secondary }}>
            Loading config…
          </p>
        ) : configForm ? (
          <form
            className="flex flex-col gap-3"
            onSubmit={async e => {
              e.preventDefault();
              setConfigSaving(true);
              setConfigError(null);
              try {
                const patchPayload = {
                  label: configForm.label,
                  lp_pool: {
                    token0_symbol: configForm.lp_pool.token0_symbol,
                    token1_symbol: configForm.lp_pool.token1_symbol,
                    initial_token0: configForm.lp_pool.initial_token0,
                    initial_token1: configForm.lp_pool.initial_token1,
                    price_lower: configForm.lp_pool.price_lower,
                    price_upper: configForm.lp_pool.price_upper,
                  },
                  hedge: {
                    hedge_token0: configForm.hedge?.hedge_token0 ?? true,
                    hedge_token1: configForm.hedge?.hedge_token1 ?? true,
                  },
                  ...(isEthPlume
                    ? {}
                    : {
                        bybit: {
                          perp_symbol_token0: configForm.bybit?.perp_symbol_token0 ?? '',
                          perp_symbol_token1: configForm.bybit?.perp_symbol_token1 ?? '',
                        },
                      }),
                };
                await api.patchHedgerConfig(selectedHedgerId, patchPayload);
                toast.success('Config saved & hedger restart requested');
                setConfigEditorOpen(false);
                await loadStatus();
              } catch (error) {
                const message = error instanceof Error ? error.message : 'Failed to save config';
                setConfigError(message);
                toast.error(message);
              } finally {
                setConfigSaving(false);
              }
            }}
          >
            <div className="grid grid-cols-2 gap-3">
              <label className="flex flex-col gap-1 text-sm">
                <span style={{ color: colors.text.muted }}>Label</span>
                <input
                  className="rounded-md border px-2 py-1"
                  style={{
                    borderColor: colors.border.DEFAULT,
                    backgroundColor: colors.bg.surface,
                    color: colors.text.primary,
                  }}
                  value={configForm.label || ''}
                  onChange={e => setConfigForm(cfg => (cfg ? { ...cfg, label: e.target.value } : cfg))}
                />
              </label>
              <label className="flex flex-col gap-1 text-sm">
                <span style={{ color: colors.text.muted }}>Token0 Symbol</span>
                <input
                  className="rounded-md border px-2 py-1"
                  style={{
                    borderColor: colors.border.DEFAULT,
                    backgroundColor: colors.bg.surface,
                    color: colors.text.primary,
                  }}
                  value={configForm.lp_pool.token0_symbol || ''}
                  onChange={e =>
                    setConfigForm(cfg =>
                      cfg ? { ...cfg, lp_pool: { ...cfg.lp_pool, token0_symbol: e.target.value } } : cfg
                    )
                  }
                />
              </label>
              <label className="flex flex-col gap-1 text-sm">
                <span style={{ color: colors.text.muted }}>Token1 Symbol</span>
                <input
                  className="rounded-md border px-2 py-1"
                  style={{
                    borderColor: colors.border.DEFAULT,
                    backgroundColor: colors.bg.surface,
                    color: colors.text.primary,
                  }}
                  value={configForm.lp_pool.token1_symbol || ''}
                  onChange={e =>
                    setConfigForm(cfg =>
                      cfg ? { ...cfg, lp_pool: { ...cfg.lp_pool, token1_symbol: e.target.value } } : cfg
                    )
                  }
                />
              </label>
              <label className="flex flex-col gap-1 text-sm">
                <span style={{ color: colors.text.muted }}>Initial Token0</span>
                <input
                  className="rounded-md border px-2 py-1"
                  style={{
                    borderColor: colors.border.DEFAULT,
                    backgroundColor: colors.bg.surface,
                    color: colors.text.primary,
                  }}
                  value={configForm.lp_pool.initial_token0 ?? ''}
                  onChange={e =>
                    setConfigForm(cfg =>
                      cfg ? { ...cfg, lp_pool: { ...cfg.lp_pool, initial_token0: e.target.value } } : cfg
                    )
                  }
                />
              </label>
              <label className="flex flex-col gap-1 text-sm">
                <span style={{ color: colors.text.muted }}>Initial Token1</span>
                <input
                  className="rounded-md border px-2 py-1"
                  style={{
                    borderColor: colors.border.DEFAULT,
                    backgroundColor: colors.bg.surface,
                    color: colors.text.primary,
                  }}
                  value={configForm.lp_pool.initial_token1 ?? ''}
                  onChange={e =>
                    setConfigForm(cfg =>
                      cfg ? { ...cfg, lp_pool: { ...cfg.lp_pool, initial_token1: e.target.value } } : cfg
                    )
                  }
                />
              </label>
              <label className="flex flex-col gap-1 text-sm">
                <span style={{ color: colors.text.muted }}>Price Lower</span>
                <input
                  className="rounded-md border px-2 py-1"
                  style={{
                    borderColor: colors.border.DEFAULT,
                    backgroundColor: colors.bg.surface,
                    color: colors.text.primary,
                  }}
                  value={configForm.lp_pool.price_lower ?? ''}
                  onChange={e =>
                    setConfigForm(cfg =>
                      cfg ? { ...cfg, lp_pool: { ...cfg.lp_pool, price_lower: e.target.value } } : cfg
                    )
                  }
                />
              </label>
              <label className="flex flex-col gap-1 text-sm">
                <span style={{ color: colors.text.muted }}>Price Upper</span>
                <input
                  className="rounded-md border px-2 py-1"
                  style={{
                    borderColor: colors.border.DEFAULT,
                    backgroundColor: colors.bg.surface,
                    color: colors.text.primary,
                  }}
                  value={configForm.lp_pool.price_upper ?? ''}
                  onChange={e =>
                    setConfigForm(cfg =>
                      cfg ? { ...cfg, lp_pool: { ...cfg.lp_pool, price_upper: e.target.value } } : cfg
                    )
                  }
                />
              </label>
              <label className="flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={configForm.hedge?.hedge_token0 ?? true}
                  onChange={e =>
                    setConfigForm(cfg =>
                      cfg
                        ? {
                            ...cfg,
                            hedge: {
                              hedge_token0: e.target.checked,
                              hedge_token1: cfg.hedge?.hedge_token1 ?? true,
                            },
                          }
                        : cfg
                    )
                  }
                />
                <span style={{ color: colors.text.primary }}>
                  Hedge Token0 ({configForm.lp_pool.token0_symbol || 'Token0'})
                </span>
              </label>
              <label className="flex items-center gap-2 text-sm">
                <input
                  type="checkbox"
                  checked={configForm.hedge?.hedge_token1 ?? true}
                  onChange={e =>
                    setConfigForm(cfg =>
                      cfg
                        ? {
                            ...cfg,
                            hedge: {
                              hedge_token0: cfg.hedge?.hedge_token0 ?? true,
                              hedge_token1: e.target.checked,
                            },
                          }
                        : cfg
                    )
                  }
                />
                <span style={{ color: colors.text.primary }}>
                  Hedge Token1 ({configForm.lp_pool.token1_symbol || 'Token1'})
                </span>
              </label>
              {!isEthPlume && (
                <>
                  <label className="flex flex-col gap-1 text-sm">
                    <span style={{ color: colors.text.muted }}>Perp Symbol Token0</span>
                    <input
                      className="rounded-md border px-2 py-1"
                      style={{
                        borderColor: colors.border.DEFAULT,
                        backgroundColor: colors.bg.surface,
                        color: colors.text.primary,
                      }}
                      value={configForm.bybit?.perp_symbol_token0 ?? ''}
                      onChange={e =>
                        setConfigForm(cfg =>
                          cfg
                            ? {
                                ...cfg,
                                bybit: { ...cfg.bybit, perp_symbol_token0: e.target.value },
                              }
                            : cfg
                        )
                      }
                    />
                  </label>
                  <label className="flex flex-col gap-1 text-sm">
                    <span style={{ color: colors.text.muted }}>Perp Symbol Token1</span>
                    <input
                      className="rounded-md border px-2 py-1"
                      style={{
                        borderColor: colors.border.DEFAULT,
                        backgroundColor: colors.bg.surface,
                        color: colors.text.primary,
                      }}
                      value={configForm.bybit?.perp_symbol_token1 ?? ''}
                      onChange={e =>
                        setConfigForm(cfg =>
                          cfg
                            ? {
                                ...cfg,
                                bybit: { ...cfg.bybit, perp_symbol_token1: e.target.value },
                              }
                            : cfg
                        )
                      }
                    />
                    <span className="text-[10px]" style={{ color: colors.text.muted }}>
                      Optional – leave blank when not hedging Token1 (e.g. USDT).
                    </span>
                  </label>
                </>
              )}
              {isEthPlume && (
                <>
                  <label className="flex flex-col gap-1 text-sm">
                    <span style={{ color: colors.text.muted }}>
                      Target Net Token0
                      <span className="ml-1 text-[10px]" style={{ color: colors.text.muted }}>
                        (Derived from initial LP today; future versions will allow overriding this target.)
                      </span>
                    </span>
                    <input
                      className="rounded-md border px-2 py-1"
                      style={{
                        borderColor: colors.border.DEFAULT,
                        backgroundColor: colors.bg.surface,
                        color: colors.text.primary,
                      }}
                      value={configForm.target.target_net_token0 ?? ''}
                      readOnly
                      disabled
                    />
                  </label>
                  <label className="flex flex-col gap-1 text-sm">
                    <span style={{ color: colors.text.muted }}>
                      Target Net Token1
                      <span className="ml-1 text-[10px]" style={{ color: colors.text.muted }}>
                        (Derived from initial LP when token1 hedging is enabled; typically leave token1 hedging off for stables.)
                      </span>
                    </span>
                    <input
                      className="rounded-md border px-2 py-1"
                      style={{
                        borderColor: colors.border.DEFAULT,
                        backgroundColor: colors.bg.surface,
                        color: colors.text.primary,
                      }}
                      value={configForm.target.target_net_token1 ?? ''}
                      readOnly
                      disabled
                    />
                  </label>
                </>
              )}
            </div>
            {configError ? (
              <p className="text-sm" style={{ color: colors.semantic.danger.DEFAULT }}>
                {configError}
              </p>
            ) : null}
            <DialogFooter>
              <Button type="button" variant="secondary" onClick={() => setConfigEditorOpen(false)}>
                Cancel
              </Button>
              <Button type="submit" variant="success" disabled={configSaving}>
                Save & Restart
              </Button>
            </DialogFooter>
          </form>
        ) : (
          <p className="text-sm" style={{ color: colors.text.secondary }}>
            No config loaded.
          </p>
        )}
      </Dialog>
    </>
  );
}
