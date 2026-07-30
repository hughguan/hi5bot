export interface OverviewResponse {
  positions: {
    ticker: string;
    shares: number;
    price: number;
    value: number;
    weight: number;
    target_weight: number;
  }[];
  cash_usd: number;
  sgov_pool: number;
  total_value: number;
  extreme_zone: string;
}

export interface RadarResponse {
  date: string;
  zone: 'NORMAL' | 'CAUTION' | 'PANIC' | 'EXTREME_PANIC';
  vix: number | null;
  aaii_bulls: number | null;
  aaii_bears: number | null;
  naaim_exposure: number | null;
  sp500_pct_above_200ma: number | null;
  extreme_pillar_count: number;
}

export interface RadarHistoryResponse {
  count: number;
  snapshots: RadarResponse[];
}

export interface OrderLogItem {
  id: number;
  timestamp: string;
  account: string;
  ticker: string;
  side: 'BUY' | 'SELL';
  shares: number;
  limit_price: number;
  est_cost: number;
  signal: string;
  status: string;
}

export interface OrderLogResponse {
  count: number;
  orders: OrderLogItem[];
}

export interface BacktestResponse {
  hi5: {
    cagr: number;
    max_dd: number;
    sharpe: number;
  };
  hi5e: {
    cagr: number;
    max_dd: number;
    sharpe: number;
  };
  nav_series: {
    date: string;
    hi5_nav: number;
    hi5_e_nav: number;
  }[];
}
