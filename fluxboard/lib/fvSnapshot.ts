import type { FvSnapshot } from '@/types';

export const DEFAULT_FV_PROFILE = 'fv1';

export const normalizeProfile = (value?: string): string =>
  ((value || DEFAULT_FV_PROFILE).trim().toLowerCase() || DEFAULT_FV_PROFILE);

const isSameStream = (previous: FvSnapshot | undefined, next: FvSnapshot): boolean => {
  if (!previous) return false;
  return (
    previous.symbol === next.symbol
    && normalizeProfile(previous.fv_profile) === normalizeProfile(next.fv_profile)
  );
};

export const mergeSnapshotWithStickyWhatMoved = (
  previous: FvSnapshot | undefined,
  next: FvSnapshot,
): FvSnapshot => {
  if (next.what_moved || !previous?.what_moved || !isSameStream(previous, next)) {
    return next;
  }

  return {
    ...next,
    what_moved: previous.what_moved,
  };
};
