import { OverviewResponse, RadarResponse, OrderLogResponse, BacktestResponse } from './types';

const API_BASE = process.env.NEXT_PUBLIC_API_BASE || 'http://localhost:8080/api';

export async function fetchOverview(): Promise<OverviewResponse> {
  try {
    const res = await fetch(`${API_BASE}/overview`, { cache: 'no-store' });
    if (!res.ok) throw new Error('Failed to fetch overview');
    return await res.json();
  } catch (e) {
    console.warn('Backend unavailable, using fallback mock overview:', e);
    return {
      positions: [
        { ticker: 'IWY', shares: 45, price: 210.5, market_value: 9472.5, current_weight_pct: 19.8, target_weight_pct: 20.0 },
        { ticker: 'SPMO', shares: 82, price: 115.2, market_value: 9446.4, current_weight_pct: 19.7, target_weight_pct: 20.0 },
        { ticker: 'RSP', shares: 58, price: 165.0, market_value: 9570.0, current_weight_pct: 20.0, target_weight_pct: 20.0 },
        { ticker: 'PFF', shares: 310, price: 30.5, market_value: 9455.0, current_weight_pct: 19.7, target_weight_pct: 20.0 },
        { ticker: 'VNQ', shares: 115, price: 82.0, market_value: 9430.0, current_weight_pct: 19.7, target_weight_pct: 20.0 },
      ],
      cash_usd: 520.0,
      sgov_pool: 0.0,
      total_value: 47893.9,
      extreme_zone: 'NORMAL',
    };
  }
}

export async function fetchRadar(): Promise<RadarResponse> {
  try {
    const res = await fetch(`${API_BASE}/radar`, { cache: 'no-store' });
    if (!res.ok) throw new Error('Failed to fetch radar');
    return await res.json();
  } catch (e) {
    console.warn('Backend unavailable, using fallback mock radar:', e);
    return {
      date: new Date().toISOString().split('T')[0],
      zone: 'NORMAL',
      vix: 16.5,
      aaii_bulls: 38.5,
      aaii_bears: 28.2,
      naaim_exposure: 75.4,
      sp500_pct_above_200ma: 68.2,
      extreme_pillar_count: 0,
    };
  }
}

export async function fetchOrders(): Promise<OrderLogResponse> {
  try {
    const res = await fetch(`${API_BASE}/orders/log`, { cache: 'no-store' });
    if (!res.ok) throw new Error('Failed to fetch order logs');
    return await res.json();
  } catch (e) {
    console.warn('Backend unavailable, using fallback mock orders:', e);
    return {
      count: 3,
      orders: [
        {
          id: 101,
          placed_at: new Date().toISOString(),
          account: 'RESP-789213',
          ticker: 'VNQ',
          side: 'BUY',
          shares: 6,
          limit_price: 81.95,
          est_cost: 491.7,
          signal: 'Signal1_RegularLowSlip',
          status: 'submitted',
        },
        {
          id: 100,
          placed_at: new Date(Date.now() - 86400000 * 5).toISOString(),
          account: 'TFSA-441209',
          ticker: 'PFF',
          side: 'BUY',
          shares: 16,
          limit_price: 30.45,
          est_cost: 487.2,
          signal: 'Signal2_ThirdFridayFallback',
          status: 'submitted',
        },
        {
          id: 99,
          placed_at: new Date(Date.now() - 86400000 * 12).toISOString(),
          account: 'RESP-789213',
          ticker: 'SPMO',
          side: 'BUY',
          shares: 4,
          limit_price: 114.8,
          est_cost: 459.2,
          signal: 'Signal1_RegularLowSlip',
          status: 'submitted',
        },
      ],
    };
  }
}

export async function fetchBacktest(): Promise<BacktestResponse> {
  try {
    const res = await fetch(`${API_BASE}/backtest/cached`, { cache: 'no-store' });
    if (!res.ok) throw new Error('Failed to fetch backtest');
    const json = await res.json();
    return json.result || json;
  } catch (e) {
    console.warn('Backend unavailable, using fallback mock backtest:', e);
    const mockNav = [];
    let hi5Nav = 10000;
    let hi5eNav = 10000;
    const startDate = new Date('2024-01-01');

    for (let i = 0; i < 90; i++) {
      const d = new Date(startDate.getTime() + i * 86400000);
      hi5Nav *= 1 + (Math.random() * 0.015 - 0.006);
      hi5eNav *= 1 + (Math.random() * 0.017 - 0.0055);
      mockNav.push({
        date: d.toISOString().split('T')[0],
        hi5_nav: parseFloat(hi5Nav.toFixed(2)),
        hi5_e_nav: parseFloat(hi5eNav.toFixed(2)),
      });
    }

    return {
      hi5: { cagr: 0.142, max_dd: -0.118, sharpe: 1.35 },
      hi5e: { cagr: 0.188, max_dd: -0.092, sharpe: 1.72 },
      nav_series: mockNav,
    };
  }
}
