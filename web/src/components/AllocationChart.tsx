'use client';

import { OverviewResponse } from '@/lib/types';
import { PieChart, Pie, Cell, ResponsiveContainer, Tooltip } from 'recharts';

interface AllocationChartProps {
  overview: OverviewResponse | null;
}

const COLORS = ['#3b82f6', '#10b981', '#f59e0b', '#8b5cf6', '#ec4899'];

export default function AllocationChart({ overview }: AllocationChartProps) {
  const positions = overview?.positions || [];
  const totalVal = overview?.total_value || 0;
  const cashUsd = overview?.cash_usd || 0;

  const chartData = positions.map((p) => {
    const val = p.market_value ?? p.value ?? 0;
    const w = p.current_weight_pct ?? (p.weight ? p.weight * 100 : 20.0);
    return {
      name: p.ticker,
      value: val,
      weight: typeof w === 'number' ? w.toFixed(1) : w,
    };
  });

  return (
    <div className="glass-panel rounded-2xl p-6 flex flex-col justify-between">
      <div className="flex items-center justify-between mb-4">
        <div>
          <h2 className="text-xl font-bold text-slate-100">Portfolio Target vs Allocation</h2>
          <p className="text-xs text-slate-400">Target Weight: 20.0% per ETF holding</p>
        </div>
        <div className="text-right">
          <div className="text-xs text-slate-400">Total Account Value</div>
          <div className="text-lg font-bold text-emerald-400">${totalVal.toLocaleString()}</div>
        </div>
      </div>

      <div className="h-48 w-full relative flex items-center justify-center">
        <ResponsiveContainer width="100%" height="100%">
          <PieChart>
            <Pie
              data={chartData}
              cx="50%"
              cy="50%"
              innerRadius={55}
              outerRadius={75}
              paddingAngle={4}
              dataKey="value"
            >
              {chartData.map((entry, index) => (
                <Cell key={`cell-${index}`} fill={COLORS[index % COLORS.length]} stroke="rgba(0,0,0,0.5)" />
              ))}
            </Pie>
            <Tooltip
              contentStyle={{ background: '#0f172a', border: '1px solid #334155', borderRadius: '8px' }}
              itemStyle={{ color: '#f8fafc', fontSize: '12px' }}
            />
          </PieChart>
        </ResponsiveContainer>
        <div className="absolute text-center pointer-events-none">
          <span className="text-xs text-slate-400 block">USD Cash</span>
          <span className="text-sm font-bold text-slate-200">${cashUsd.toFixed(2)}</span>
        </div>
      </div>

      <div className="grid grid-cols-5 gap-2 mt-4">
        {positions.map((p, i) => {
          const w = p.current_weight_pct ?? (p.weight ? p.weight * 100 : 20.0);
          return (
            <div key={p.ticker} className="glass-card p-2 rounded-lg text-center">
              <div className="flex items-center justify-center gap-1 text-xs font-bold text-slate-200">
                <span className="w-2 h-2 rounded-full inline-block" style={{ background: COLORS[i % COLORS.length] }} />
                {p.ticker}
              </div>
              <div className="text-[11px] text-slate-400 mt-1">{typeof w === 'number' ? w.toFixed(1) : w}%</div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
