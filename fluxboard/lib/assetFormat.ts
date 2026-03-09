const STABLE_BASES = new Set([
  'USDT',
  'USDC',
  'PUSD',
  'PUSDT',
  'FDUSD',
  'BUSD',
  'DAI',
  'USD',
  'USDB',
  'WUSDC',
  'WUSDT',
  'EUROC',
  'PLUME_USD',
]);

const MAJOR_CRYPTOS = new Set([
  'BTC',
  'WBTC',
  'ETH',
  'WETH',
  'BNB',
  'SOL',
]);

const normalize = (symbol: string | null | undefined): string =>
  (symbol ?? '').toUpperCase().trim().replace(/_LOGICAL$/, '');

const stripPerp = (symbol: string): string =>
  symbol.endsWith('_PERP') ? symbol.slice(0, -5) : symbol;

const baseToken = (symbol: string): string => {
  const normalized = normalize(symbol);
  const withoutPerp = stripPerp(normalized);
  const [base] = withoutPerp.split(/[._]/);
  return base;
};

export function isStable(symbol: string): boolean {
  const normalized = normalize(symbol);
  if (!normalized) return false;
  if (normalized.endsWith('_PERP')) return false;

  if (STABLE_BASES.has(normalized)) return true;

  const separatorIndex = Math.max(normalized.indexOf('.'), normalized.indexOf('_'));
  if (separatorIndex > 0) {
    const base = normalized.slice(0, separatorIndex);
    const suffix = normalized.slice(separatorIndex + 1);
    if (STABLE_BASES.has(base) && suffix && suffix !== 'PERP') {
      return true;
    }
  }

  return false;
}

export function isPerp(symbol: string): boolean {
  const normalized = normalize(symbol);
  return normalized.endsWith('_PERP');
}

export function isMajorCrypto(symbol: string): boolean {
  const base = baseToken(symbol);
  return MAJOR_CRYPTOS.has(base);
}

const formatNumber = (
  value: number,
  minimumFractionDigits: number,
  maximumFractionDigits: number,
): string => {
  return new Intl.NumberFormat('en-US', {
    minimumFractionDigits,
    maximumFractionDigits,
  }).format(value);
};

const MISSING = '—';

export function formatMark(symbol: string, markUsd: number | null | undefined): string {
  if (markUsd === null || markUsd === undefined || Number.isNaN(markUsd)) return MISSING;

  const price = Number(markUsd);

  if (isStable(symbol) || isPerp(symbol)) {
    return formatNumber(price, 2, 2);
  }

  const absPrice = Math.abs(price);
  const maxFractionDigits = (() => {
    if (absPrice >= 1000) return 2;
    if (absPrice >= 1) return 3;
    if (absPrice >= 0.01) return 4;
    return 6;
  })();

  return formatNumber(price, 0, maxFractionDigits);
}

export function formatQty(
  symbol: string,
  qty: number | null | undefined,
  markUsd: number | null | undefined,
): string {
  if (qty === null || qty === undefined || Number.isNaN(qty)) return MISSING;

  const amount = Number(qty);
  const absQty = Math.abs(amount);
  const price = Number(markUsd ?? 0);
  const mvUsd = Math.abs(amount * price);

  if (isStable(symbol)) {
    return formatNumber(amount, 2, 2);
  }

  if (isPerp(symbol)) {
    return formatNumber(amount, 0, 3);
  }

  if (isMajorCrypto(symbol)) {
    const maxFractionDigits = absQty < 1 ? 6 : 4;
    return formatNumber(amount, 0, maxFractionDigits);
  }

  const maxFractionDigits = (() => {
    if (mvUsd >= 10_000) return 2;
    if (mvUsd >= 1_000) return 3;
    if (mvUsd >= 1) return 4;
    return 6;
  })();

  return formatNumber(amount, 0, maxFractionDigits);
}
