import type { SignalLeg, SignalStrategy } from '../types';

export type SignalLegSlot = 'A' | 'B';
export type SignalLegEntry = { key: string; leg: SignalLeg | null };

type SignalLegMap = SignalStrategy['legs'];

const SLOT_INDEX: Record<SignalLegSlot, number> = {
  A: 0,
  B: 1,
};

function isObjectRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === 'object';
}

function asLegMap(legs: SignalLegMap | null | undefined): SignalLegMap {
  if (!isObjectRecord(legs)) return {};
  return legs as SignalLegMap;
}

export function getOrderedLegKeys(
  legs: SignalLegMap | null | undefined,
  legsOrder?: readonly string[] | null,
): string[] {
  const legMap = asLegMap(legs);
  const keys = Object.keys(legMap).filter((key) => legMap[key] !== null && legMap[key] !== undefined);
  if (keys.length === 0) return [];

  const ordered: string[] = [];
  const seen = new Set<string>();

  if (Array.isArray(legsOrder)) {
    for (const key of legsOrder) {
      if (typeof key !== 'string') continue;
      if (seen.has(key)) continue;
      if (!keys.includes(key)) continue;
      seen.add(key);
      ordered.push(key);
    }
  }

  const remaining = keys
    .filter((key) => !seen.has(key))
    .sort((a, b) => a.localeCompare(b));

  return ordered.concat(remaining);
}

export function getOrderedLegEntries(
  strategy: Pick<SignalStrategy, 'legs' | 'legs_order'> | null | undefined,
): SignalLegEntry[] {
  if (!strategy) return [];
  const legMap = asLegMap(strategy.legs);
  return getOrderedLegKeys(legMap, strategy.legs_order).map((key) => ({
    key,
    leg: legMap[key] ?? null,
  }));
}

export function getLegKeyForSlot(
  strategy: Pick<SignalStrategy, 'legs' | 'legs_order'> | null | undefined,
  slot: SignalLegSlot,
): string | undefined {
  const keys = getOrderedLegKeys(strategy?.legs, strategy?.legs_order);
  return keys[SLOT_INDEX[slot]];
}

export function getLegForSlot(
  strategy: Pick<SignalStrategy, 'legs' | 'legs_order'> | null | undefined,
  slot: SignalLegSlot,
): SignalLeg | null {
  const key = getLegKeyForSlot(strategy, slot);
  if (!key) return null;
  const legMap = asLegMap(strategy?.legs);
  return legMap[key] ?? null;
}

export function resolveRoleSlot(
  roleKey: string | undefined,
  strategy: Pick<SignalStrategy, 'legs' | 'legs_order'> | null | undefined,
): SignalLegSlot | undefined {
  if (!roleKey) return undefined;
  if (roleKey === 'A' || roleKey === 'B') return roleKey;
  const legKeyA = getLegKeyForSlot(strategy, 'A');
  const legKeyB = getLegKeyForSlot(strategy, 'B');
  if (roleKey === legKeyA) return 'A';
  if (roleKey === legKeyB) return 'B';
  return undefined;
}

export function buildLegDeltaPatch(legsDelta: unknown): SignalLegMap | undefined {
  if (!isObjectRecord(legsDelta)) return undefined;
  const patch: SignalLegMap = {};
  for (const key of Object.keys(legsDelta)) {
    const value = (legsDelta as Record<string, unknown>)[key];
    patch[key] = (value as SignalLeg | null | undefined) ?? null;
  }
  return Object.keys(patch).length > 0 ? patch : undefined;
}

export function mergeSignalLegMaps(
  previousLegs: SignalLegMap | null | undefined,
  incomingLegs: SignalLegMap | null | undefined,
): SignalLegMap {
  const prev = asLegMap(previousLegs);
  if (!isObjectRecord(incomingLegs)) {
    return prev;
  }

  const merged: SignalLegMap = { ...prev };
  for (const key of Object.keys(incomingLegs)) {
    const incoming = incomingLegs[key];
    if (incoming === null) {
      delete merged[key];
      continue;
    }
    if (incoming === undefined) {
      continue;
    }
    const prevLeg = prev[key];
    if (prevLeg && typeof prevLeg === 'object') {
      merged[key] = { ...prevLeg, ...incoming };
    } else {
      merged[key] = incoming;
    }
  }

  return merged;
}
