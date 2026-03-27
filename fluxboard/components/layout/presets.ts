// Dashboard layout presets

export type LayoutConfig = {
  i: string;
  x: number;
  y: number;
  w: number;
  h: number;
};

export const PRESETS: Record<string, LayoutConfig[]> = {
  default: [
    { i: 'signal', x: 0, y: 0, w: 12, h: 6 },
    { i: 'balances', x: 0, y: 6, w: 12, h: 4 },
    { i: 'trades', x: 0, y: 10, w: 12, h: 5 },
    { i: 'params', x: 0, y: 15, w: 12, h: 4 },
    { i: 'fv', x: 0, y: 19, w: 12, h: 5 },
  ],
  signal: [
    { i: 'signal', x: 0, y: 0, w: 12, h: 6 },
    { i: 'trades', x: 0, y: 6, w: 6, h: 5 },
    { i: 'params', x: 6, y: 6, w: 6, h: 4 },
    { i: 'fv', x: 0, y: 11, w: 12, h: 5 },
  ],
};
