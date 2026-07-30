'use client';

import { OrderLogItem } from '@/lib/types';
import { ShoppingCart, CheckCircle2, Clock } from 'lucide-react';

interface OrderLogTableProps {
  orders: OrderLogItem[];
}

export default function OrderLogTable({ orders }: OrderLogTableProps) {
  return (
    <div className="glass-panel rounded-2xl p-6">
      <div className="flex items-center justify-between mb-4">
        <div>
          <h2 className="text-xl font-bold text-slate-100">Audit & Order Logs</h2>
          <p className="text-xs text-slate-400">Real-time Questrade Execution History</p>
        </div>
        <span className="text-xs text-slate-400 font-mono">Total Logs: {orders.length}</span>
      </div>

      <div className="overflow-x-auto">
        <table className="w-full text-left text-xs">
          <thead>
            <tr className="border-b border-slate-800 text-slate-400 uppercase tracking-wider font-semibold">
              <th className="pb-3">Time</th>
              <th className="pb-3">Account</th>
              <th className="pb-3">Ticker</th>
              <th className="pb-3">Side</th>
              <th className="pb-3">Shares</th>
              <th className="pb-3">Limit Price</th>
              <th className="pb-3">Est. Cost</th>
              <th className="pb-3">Signal</th>
              <th className="pb-3">Status</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-slate-800/60">
            {orders.map((o) => {
              const timeStr = o.placed_at ?? o.timestamp ?? '';
              const dateObj = timeStr ? new Date(timeStr) : new Date();
              return (
                <tr key={o.id} className="hover:bg-slate-800/40 transition-colors">
                  <td className="py-3 text-slate-300 font-mono">
                    {dateObj.toLocaleDateString()} {dateObj.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
                  </td>
                <td className="py-3 text-slate-300 font-semibold">{o.account}</td>
                <td className="py-3 font-bold text-blue-400">{o.ticker}</td>
                <td className="py-3">
                  <span className="px-2 py-0.5 rounded bg-emerald-500/20 text-emerald-300 border border-emerald-500/30 font-bold">
                    {o.side}
                  </span>
                </td>
                <td className="py-3 font-semibold text-slate-200">{o.shares}</td>
                <td className="py-3 text-slate-300 font-mono">${o.limit_price.toFixed(2)}</td>
                <td className="py-3 text-emerald-400 font-mono font-semibold">${o.est_cost.toFixed(2)}</td>
                <td className="py-3 text-slate-400">{o.signal}</td>
                <td className="py-3">
                  <span className="inline-flex items-center gap-1 text-[11px] text-amber-400 font-medium bg-amber-500/10 px-2 py-0.5 rounded border border-amber-500/20">
                    <Clock className="w-3 h-3" />
                    {o.status}
                  </span>
                </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
