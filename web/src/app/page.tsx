'use client';

import { useEffect, useState } from 'react';
import { fetchOverview, fetchRadar, fetchOrders, fetchBacktest } from '@/lib/api';
import { OverviewResponse, RadarResponse, OrderLogResponse, BacktestResponse } from '@/lib/types';
import RadarWidget from '@/components/RadarWidget';
import AllocationChart from '@/components/AllocationChart';
import BacktestChart from '@/components/BacktestChart';
import OrderLogTable from '@/components/OrderLogTable';
import { Cpu, RefreshCw, Zap, Shield } from 'lucide-react';

export default function Home() {
  const [overview, setOverview] = useState<OverviewResponse | null>(null);
  const [radar, setRadar] = useState<RadarResponse | null>(null);
  const [orders, setOrders] = useState<OrderLogResponse | null>(null);
  const [backtest, setBacktest] = useState<BacktestResponse | null>(null);
  const [loading, setLoading] = useState(true);

  const loadData = async () => {
    setLoading(true);
    const [ov, rd, ord, bt] = await Promise.all([
      fetchOverview(),
      fetchRadar(),
      fetchOrders(),
      fetchBacktest(),
    ]);
    setOverview(ov);
    setRadar(rd);
    setOrders(ord);
    setBacktest(bt);
    setLoading(false);
  };

  useEffect(() => {
    loadData();
  }, []);

  return (
    <main className="min-h-screen bg-[#090d16] text-slate-100 p-6 md:p-10 max-w-7xl mx-auto space-y-8">
      {/* Top Header */}
      <header className="flex flex-col md:flex-row items-start md:items-center justify-between gap-4 border-b border-slate-800/80 pb-6">
        <div className="flex items-center gap-3">
          <div className="p-3 rounded-2xl bg-blue-600/20 border border-blue-500/30 text-blue-400">
            <Cpu className="w-7 h-7" />
          </div>
          <div>
            <h1 className="text-2xl font-extrabold tracking-tight text-slate-100 flex items-center gap-2">
              Hi5bot Dashboard
              <span className="text-xs bg-emerald-500/20 text-emerald-300 border border-emerald-500/40 px-2.5 py-0.5 rounded-full font-mono font-semibold">
                v0.1.0 Daemon Live
              </span>
            </h1>
            <p className="text-xs text-slate-400 mt-0.5">
              Quantitative ETF Asset Allocation & Market Extreme Radar System
            </p>
          </div>
        </div>

        <div className="flex items-center gap-3">
          <button
            onClick={loadData}
            disabled={loading}
            className="flex items-center gap-2 px-4 py-2 rounded-xl bg-slate-800 hover:bg-slate-700 text-slate-200 text-xs font-semibold border border-slate-700 transition-colors"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${loading ? 'animate-spin' : ''}`} />
            Refresh Data
          </button>
        </div>
      </header>

      {/* Grid Row 1: Radar & Allocation */}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-8">
        <RadarWidget radar={radar} />
        <AllocationChart overview={overview} />
      </div>

      {/* Grid Row 2: Backtest Laboratory */}
      <BacktestChart backtest={backtest} />

      {/* Grid Row 3: Audit Order Logs */}
      <OrderLogTable orders={orders?.orders || []} />

      {/* Footer */}
      <footer className="text-center text-xs text-slate-500 pt-6 border-t border-slate-800/60">
        Hi5bot Rust Automation Daemon — Connected via Axum Web REST API (Port 8080)
      </footer>
    </main>
  );
}
