export const STANDARD_CONTRACT_VERSION = 2 as const;

export type StandardContractVersion = typeof STANDARD_CONTRACT_VERSION;

export enum RealtimeSurfaceState {
  SYNCING = 'syncing',
  LIVE = 'live',
  LAGGING = 'lagging',
  STALE = 'stale',
  RECOVERING = 'recovering',
  MANUAL_REFRESH_REQUIRED = 'manual_refresh_required',
}

export type RealtimeStreamId = string;
export type RealtimeSequence = number;
export type RealtimeSnapshotRevision = number | string;

export interface RealtimeContractFields {
  contract_version: StandardContractVersion;
  stream_id: RealtimeStreamId;
  seq: RealtimeSequence;
  snapshot_revision: RealtimeSnapshotRevision;
}
