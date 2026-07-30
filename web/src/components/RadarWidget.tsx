'use client';

import { RadarResponse } from '@/lib/types';
import { ShieldAlert, ShieldCheck, Activity, TrendingUp, AlertTriangle } from 'lucide-react';

interface RadarWidgetProps {
  radar: RadarResponse | null;
}

export default function RadarWidget({ radar }: RadarWidgetProps) {
  const zone = radar?.zone || 'NORMAL';

  const getZoneStyle = (z: string) => {
    switch (z) {
      case 'EXTREME_PANIC':
        return {
          bg: 'bg-red-950/40 border-red-500/30 text-red-400',
          badge: 'bg-red-500/20 text-red-300 border-red-500/40',
          icon: AlertTriangle,
          multiplier: '3.0× Maximum Aggression',
          desc: 'All sentiment pillars extreme. Deploying 3× buffer pool cash.',
        };
      case 'PANIC':
        return {
          bg: 'bg-amber-950/40 border-amber-500/30 text-amber-400',
          badge: 'bg-amber-500/20 text-amber-300 border-amber-500/40',
          icon: ShieldAlert,
          multiplier: '2.0× Aggressive Deploy',
          desc: 'Two sentiment pillars in extreme panic zone. Deploying 2× buffer pool cash.',
        };
      case 'CAUTION':
        return {
          bg: 'bg-yellow-950/40 border-yellow-500/30 text-yellow-400',
          badge: 'bg-yellow-500/20 text-yellow-300 border-yellow-500/40',
          icon: Activity,
          multiplier: '0.5× Base Allocation',
          desc: 'Single pillar flashing. Single pillar treated as noise unless VIX ≥ 35.',
        };
      default:
        return {
          bg: 'bg-emerald-950/30 border-emerald-500/30 text-emerald-400',
          badge: 'bg-emerald-500/20 text-emerald-300 border-emerald-500/40',
          icon: ShieldCheck,
          multiplier: '0.5× Base Allocation',
          desc: 'No market stress detected. Standard conservative DCA execution.',
        };
    }
  };

  const currentStyle = getZoneStyle(zone);
  const IconComponent = currentStyle.icon;

  const pillars = [
    {
      name: 'AAII Sentiment (Bulls/Bears)',
      val: radar?.aaii_bulls ? `${radar.aaii_bulls}% / ${radar.aaii_bears}%` : 'N/A',
      status: radar?.aaii_bears && radar.aaii_bears >= 55 ? 'EXTREME' : 'NORMAL',
    },
    {
      name: 'NAAIM Exposure Index',
      val: radar?.naaim_exposure ? `${radar.naaim_exposure}%` : 'N/A',
      status: radar?.naaim_exposure && radar.naaim_exposure <= 40 ? 'EXTREME' : 'NORMAL',
    },
    {
      name: 'S&P 500 Market Breadth (>200MA)',
      val: radar?.sp500_pct_above_200ma ? `${radar.sp500_pct_above_200ma}%` : 'N/A',
      status: radar?.sp500_pct_above_200ma && radar.sp500_pct_above_200ma <= 30 ? 'EXTREME' : 'NORMAL',
    },
    {
      name: 'VIX Volatility Index',
      val: radar?.vix ? `${radar.vix}` : 'N/A',
      status: radar?.vix && radar.vix >= 35 ? 'EXTREME' : 'NORMAL',
    },
  ];

  return (
    <div className="glass-panel rounded-2xl p-6 relative overflow-hidden">
      <div className="flex items-center justify-between mb-6">
        <div className="flex items-center gap-3">
          <div className={`p-3 rounded-xl border ${currentStyle.bg}`}>
            <IconComponent className="w-6 h-6" />
          </div>
          <div>
            <h2 className="text-xl font-bold text-slate-100 flex items-center gap-2">
              Market Extreme Radar
            </h2>
            <p className="text-xs text-slate-400">Reading Date: {radar?.date || 'Today'}</p>
          </div>
        </div>
        <div className={`px-4 py-1.5 rounded-full text-xs font-semibold border ${currentStyle.badge}`}>
          {zone} ({radar?.extreme_pillar_count || 0} Pillars)
        </div>
      </div>

      <div className="mb-6 p-4 rounded-xl border bg-slate-900/60 border-slate-800 flex items-start justify-between">
        <div>
          <div className="text-xs text-slate-400 font-medium uppercase tracking-wider mb-1">
            Live Deployment Multiplier
          </div>
          <div className="text-lg font-bold text-slate-200">{currentStyle.multiplier}</div>
          <p className="text-xs text-slate-400 mt-1">{currentStyle.desc}</p>
        </div>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
        {pillars.map((p, idx) => (
          <div key={idx} className="glass-card p-3.5 rounded-xl flex items-center justify-between">
            <div>
              <div className="text-xs text-slate-400">{p.name}</div>
              <div className="text-sm font-semibold text-slate-200 mt-0.5">{p.val}</div>
            </div>
            <span
              className={`text-[10px] font-bold px-2 py-0.5 rounded border ${
                p.status === 'EXTREME'
                  ? 'bg-red-500/20 text-red-300 border-red-500/30'
                  : 'bg-slate-800 text-slate-400 border-slate-700'
              }`}
            >
              {p.status}
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}
