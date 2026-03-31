import type {
  BalanceChildRow,
  BalanceParentRow,
  TokenMMBalanceRowType,
  TokenMMBalanceSectionKey,
  TokenMMBalancesVenueOption,
} from '../../types';

function titleCase(value: string): string {
  if (!value) return '';
  return value.slice(0, 1).toUpperCase() + value.slice(1);
}

function coerceText(value: unknown): string {
  return typeof value === 'string' ? value.trim() : '';
}

function normalizeInstrumentFallback(row: BalanceChildRow): string {
  return coerceText(
    row.display_name_short
      ?? row.display_name_long
      ?? row.raw_symbol
      ?? row.instrument_id
      ?? row.contract
      ?? row.coin,
  );
}

export function classifyTokenMMBalanceRowType(row: BalanceChildRow): TokenMMBalanceRowType {
  const productType = coerceText(row.product_type).toLowerCase();
  const marketType = coerceText(row.market_type).toLowerCase();
  const contractType = coerceText(row.contract_type).toLowerCase();
  const venue = coerceText(row.venue).toLowerCase();

  if (
    productType === 'perp'
    || marketType === 'perp'
    || contractType === 'perp'
    || contractType === 'linear'
    || contractType === 'swap'
    || contractType === 'future'
  ) {
    return 'perp';
  }

  if (
    productType === 'spot'
    || marketType === 'spot'
  ) {
    return 'spot';
  }

  if (
    venue === 'wallet'
    || contractType === 'cash'
  ) {
    return 'cash';
  }

  return 'spot';
}

export function getTokenMMBalanceReadableLabel(row: BalanceChildRow): string {
  return normalizeInstrumentFallback(row);
}

export function buildTokenMMBalanceSearchTokens(row: BalanceChildRow): string {
  return [
    row.coin,
    row.venue,
    getTokenMMBalanceReadableLabel(row),
    row.display_name_long,
    row.wallet,
    row.label,
    row.address,
    row.raw_symbol,
    row.instrument_id,
    row.contract,
  ]
    .map((value) => coerceText(value))
    .filter(Boolean)
    .join(' ')
    .toLowerCase();
}

export function groupTokenMMBalanceSections(
  rows: BalanceParentRow[],
): Record<TokenMMBalanceSectionKey, BalanceParentRow[]> {
  return rows.reduce<Record<TokenMMBalanceSectionKey, BalanceParentRow[]>>(
    (acc, row) => {
      if (row.stable) {
        acc.stables.push(row);
      } else {
        acc.trading.push(row);
      }
      return acc;
    },
    { stables: [], trading: [] },
  );
}

export function deriveTokenMMVenueOptions(rows: BalanceParentRow[]): TokenMMBalancesVenueOption[] {
  const uniqueVenues = new Set<string>();

  rows.forEach((row) => {
    row.children.forEach((child) => {
      const venue = coerceText(child.venue).toLowerCase();
      if (venue) {
        uniqueVenues.add(venue);
      }
    });
  });

  return [
    { label: 'All venues', value: 'all' },
    ...Array.from(uniqueVenues)
      .sort((left, right) => left.localeCompare(right))
      .map((venue) => ({ label: titleCase(venue), value: venue })),
  ];
}
