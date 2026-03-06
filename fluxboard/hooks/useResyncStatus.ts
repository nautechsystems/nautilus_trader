import {
  shallow,
  useResyncStore,
  selectResyncId,
  selectResyncing,
  selectResyncLastReason,
  selectResyncLastBumpAt,
} from '../stores';

export type ResyncStatus = {
  resyncId: number;
  isResyncing: boolean;
  lastReason?: string;
  lastBumpAt?: number;
};

export const useResyncStatus = (): ResyncStatus =>
  useResyncStore(
    (state) => ({
      resyncId: selectResyncId(state),
      isResyncing: selectResyncing(state),
      lastReason: selectResyncLastReason(state),
      lastBumpAt: selectResyncLastBumpAt(state),
    }),
    shallow,
  );
