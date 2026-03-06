import { create } from 'zustand';
import type { MarketSnapshot } from '@/types';

export const MARKET_DATA_PAGE_SIZE = 50;

export type MarketDataState = {
  rows: MarketSnapshot[];
  loading: boolean;
  lastUpdate: number | null;
  setSnapshot: (rows: MarketSnapshot[], tsMs?: number | null) => void;
  replaceFromSocket: (rows: MarketSnapshot[], tsMs?: number | null) => void;
  setLoading: (loading: boolean) => void;
  setLastUpdate: (tsMs: number | null) => void;
};

const nowMs = () => Date.now();

export const useMarketDataStore = create<MarketDataState>((set) => ({
  rows: [],
  loading: false,
  lastUpdate: null,
  setSnapshot: (rows, tsMs) => set({ rows, lastUpdate: tsMs ?? nowMs() }),
  replaceFromSocket: (rows, tsMs) => set({ rows, lastUpdate: tsMs ?? nowMs() }),
  setLoading: (loading) => set({ loading }),
  setLastUpdate: (tsMs) => set({ lastUpdate: tsMs }),
}));
