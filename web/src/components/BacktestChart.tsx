'use client';

import { BacktestResponse } from '@/lib/types';
import { AreaChart, Area, XAxis, YAxis, Tooltip, ResponsiveContainer } from 'recharts';

interface BacktestChartProps {
  backtest: BacktestResponse | null;
}

export default function BacktestChart({ backtest }: BacktestChartProps) {
  const hi5 = backtest?.hi5 || { cagr: 0, max_dd: 0, sharpe: 0 };
  const hi5e = backtest?.hi5e || { cagr: 0, max_dd: 0, sharpe: 0 };
  const navSeries = backtest?.nav_series || [];

  return (
    <div className="glass-panel rounded-2xl p-6">
      <div className="flex items-center justify-between mb-6">
        <div>
          <h2 className="text-xl font-bold text-slate-100">Hi5 vs Hi5e Backtest Comparison</h2>
          <p className="text-xs text-slate-400">Baseline Mechanical DCA vs Dynamic Extreme-Zone Strategy</p>
        </div>

        <div className="flex items-center gap-6">
          <div className="text-right">
            <div className="text-xs text-slate-400">Hi5 Benchmark</div>
            <div className="text-sm font-semibold text-blue-400">
              CAGR {(hi5.cagr * 100).toFixed(1)}% | MaxDD {(hi5.max_dd * 100).toFixed(1)}%
            </div>
          </div>
          <div className="text-right">
            <div className="text-xs text-slate-400">Hi5e Enhanced</div>
            <div className="text-sm font-bold text-emerald-400">
              CAGR {(hi5e.cagr * 100).toFixed(1)}% | MaxDD {(hi5e.max_dd * 100).toFixed(1)}%
            </div>
          </div>
        </div>
      </div>

      <div className="h-64 w-full">
        <ResponsiveContainer width="100%" height="100%">
          <AreaChart data={navSeries} margin={{ top: 10, right: 10, left: -20, bottom: 0 }}>
            <defs>
              <linearGradient id="colorHi5" x1="0" y1="0" x2="0" y2="1">
                <stop offset="5%" stopColor="#3b82f6" stopOpacity={0.4} />
                <stop offset="95%" stopColor="#3b82f6" stopOpacity={0} />
              </linearGradient>
              <linearGradient id="colorHi5e" x1="0" y1="0" x2="0" y2="1">
                <stop offset="5%" stopColor="#10b981" stopOpacity={0.5} />
                <stop offset="95%" stopColor="#10b981" stopOpacity={0} />
              </linearGradient>
            </defs>
            <XAxis dataKey="date" stroke="#64748b" fontSize={11} tickLine={false} />
            <YAxis stroke="#64748b" fontSize={11} tickLine={false} />
            <Tooltip
              contentStyle={{ background: '#0f172a', border: '1px solid #334155', borderRadius: '8px' }}
              itemStyle={{ fontSize: '12px' }}
            />
            <Area type="monotone" dataKey="hi5_nav" stroke="#3b82f6" fillOpacity={1} fill="url(#colorHi5)" name="Hi5 NAV" />
            <Area type="monotone" dataKey="hi5_e_nav" stroke="#10b981" strokeWidth={2} fillOpacity={1} fill="url(#colorHi5e)" name="Hi5e NAV" />
          </AreaChart>
        </ResponsiveContainer>
      </div>
    </div>
  );
}
