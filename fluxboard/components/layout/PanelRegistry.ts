// Panel registry for dashboard

import { ParamsPanel } from '../panels/ParamsPanel';
import { TradesPanel } from '../panels/TradesPanel';
import { SignalPanel } from '../panels/SignalPanel';
import { BalancesPanel } from '../panels/BalancesPanel';
import { AlertsPanel } from '../panels/AlertsPanel';
import { FVPanel } from '../panels/FVPanel';

export const PANEL_REGISTRY = {
  params: ParamsPanel,
  trades: TradesPanel,
  signal: SignalPanel,
  fv: FVPanel,
  balances: BalancesPanel,
  alerts: AlertsPanel,
} as const;

export type PanelId = keyof typeof PANEL_REGISTRY;
