//! Market Radar — Extreme Zone detection engine.
//!
//! Aggregates three pillars of sentiment data to classify the current market
//! regime into one of four zones:
//!
//! | Zone           | Description                    | Action                          |
//! |----------------|--------------------------------|---------------------------------|
//! | `Normal`       | No stress detected             | Base allocation only (Hi5)      |
//! | `Caution`      | One pillar flashing            | Moderate cash reserve           |
//! | `Panic`        | Two pillars in extreme zone    | Deploy 2× buffer pool cash      |
//! | `ExtremePanic` | All three pillars extreme      | Deploy 3× buffer pool cash      |
//!
//! ## Three Pillars
//!
//! 1. **AAII Sentiment** — Bulls / Bears ratio. Bearish extreme when
//!    bears ≥ 55% or bulls ≤ 25%.
//! 2. **NAAIM Exposure** — Active manager equity exposure. Bearish extreme
//!    when exposure ≤ 40%.
//! 3. **Market Breadth** — % of S&P 500 stocks above 200-day MA. Bearish
//!    extreme when ≤ 30%.

use chrono::NaiveDate;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

// ---- Zone classification ------------------------------------------------

/// The four market regime zones.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExtremeZone {
    Normal,
    Caution,
    Panic,
    ExtremePanic,
}

impl ExtremeZone {
    pub fn multiplier(self) -> f64 {
        match self {
            ExtremeZone::Normal | ExtremeZone::Caution => 0.5,
            ExtremeZone::Panic => 2.0,
            ExtremeZone::ExtremePanic => 3.0,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ExtremeZone::Normal => "NORMAL",
            ExtremeZone::Caution => "CAUTION",
            ExtremeZone::Panic => "PANIC",
            ExtremeZone::ExtremePanic => "EXTREME_BUY_NOW",
        }
    }

    pub fn css_class(self) -> &'static str {
        match self {
            ExtremeZone::Normal => "zone-normal",
            ExtremeZone::Caution => "zone-caution",
            ExtremeZone::Panic => "zone-panic",
            ExtremeZone::ExtremePanic => "zone-extreme",
        }
    }
}

impl std::fmt::Display for ExtremeZone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

// ---- Pillar indicators --------------------------------------------------

/// Each pillar reports its raw value and whether it's in the extreme zone.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PillarStatus {
    pub name: String,
    pub value: f64,
    pub is_extreme: bool,
    pub extreme_threshold: f64,
    pub direction: String, // "above" | "below"
}

// ---- Radar snapshot -----------------------------------------------------

/// A full snapshot of the radar at a point in time.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RadarSnapshot {
    pub date: NaiveDate,
    pub zone: ExtremeZone,
    pub pillars: Vec<PillarStatus>,
    pub vix: Option<f64>,
    pub rsp_daily_return: Option<f64>,
    pub rsp_monthly_drawdown: Option<f64>,
    /// Number of pillars currently flashing extreme.
    pub extreme_pillar_count: u8,
}

// ---- Engine --------------------------------------------------------------

/// Compute the extreme zone from the three pillar values + supporting market data.
pub fn classify_zone(
    aaii_bulls: Option<f64>,
    aaii_bears: Option<f64>,
    naaim_exposure: Option<f64>,
    sp500_pct_above_200ma: Option<f64>,
    vix: Option<f64>,
    rsp_daily_return: Option<f64>,
    rsp_monthly_drawdown: Option<f64>,
) -> RadarSnapshot {
    let mut pillars = Vec::new();
    let mut extreme_count: u8 = 0;

    // Pillar 1: AAII Bears extreme (≥ 55%) or Bulls extreme (≤ 25%).
    let aaii_extreme = match (aaii_bulls, aaii_bears) {
        (Some(bulls), Some(bears)) => {
            let bear_extreme = bears >= 55.0;
            let bull_extreme = bulls <= 25.0;
            bear_extreme || bull_extreme
        }
        (Some(bulls), None) => bulls <= 25.0,
        (None, Some(bears)) => bears >= 55.0,
        (None, None) => false,
    };
    pillars.push(PillarStatus {
        name: "AAII Bears > 55%".into(),
        value: aaii_bears.unwrap_or(0.0),
        is_extreme: aaii_extreme,
        extreme_threshold: 55.0,
        direction: "above".into(),
    });
    if aaii_extreme {
        extreme_count += 1;
    }

    // Pillar 2: NAAIM Exposure ≤ 40%.
    let naaim_val = naaim_exposure.unwrap_or(100.0);
    let naaim_extreme = naaim_val <= 40.0;
    pillars.push(PillarStatus {
        name: "NAAIM Exposure ≤ 40%".into(),
        value: naaim_val,
        is_extreme: naaim_extreme,
        extreme_threshold: 40.0,
        direction: "below".into(),
    });
    if naaim_extreme {
        extreme_count += 1;
    }

    // Pillar 3: S&P 500 % above 200MA ≤ 30%.
    let breadth_val = sp500_pct_above_200ma.unwrap_or(100.0);
    let breadth_extreme = breadth_val <= 30.0;
    pillars.push(PillarStatus {
        name: "S&P 500 above 200MA ≤ 30%".into(),
        value: breadth_val,
        is_extreme: breadth_extreme,
        extreme_threshold: 30.0,
        direction: "below".into(),
    });
    if breadth_extreme {
        extreme_count += 1;
    }

    // Classify zone from extreme pillar count, refined by VIX / RSP action.
    let zone = match extreme_count {
        0 => ExtremeZone::Normal,
        1 => {
            // Single pillar may be noise; escalate to Caution but not Panic.
            // However, if VIX ≥ 35, escalate to Panic anyway.
            if vix.unwrap_or(0.0) >= 35.0 {
                ExtremeZone::Panic
            } else {
                ExtremeZone::Caution
            }
        }
        2 => ExtremeZone::Panic,
        _ => {
            // All three pillars extreme + VIX ≥ 35 + RSP daily ≤ -3% → ExtremePanic
            if vix.unwrap_or(0.0) >= 35.0
                && rsp_daily_return.unwrap_or(0.0) <= -0.03
            {
                ExtremeZone::ExtremePanic
            } else {
                ExtremeZone::Panic
            }
        }
    };

    RadarSnapshot {
        date: chrono::Utc::now().date_naive(),
        zone,
        pillars,
        vix,
        rsp_daily_return,
        rsp_monthly_drawdown,
        extreme_pillar_count: extreme_count,
    }
}

/// Synthesize Hi5e dynamic deployment budget from the original Hi5 budget
/// and the current extreme zone.
///
/// - Normal/Caution: 50% deployed, 50% goes to SGOV cash reserve
/// - Panic: 200% deployed (unlock reserve)
/// - ExtremePanic: 300% deployed (max aggression)
pub fn hi5e_dynamic_budget(hi5_base_budget: Decimal, zone: ExtremeZone) -> Decimal {
    match zone {
        ExtremeZone::Normal => hi5_base_budget * Decimal::new(5, 1), // 0.5×
        ExtremeZone::Caution => {
            hi5_base_budget * Decimal::new(5, 1) // 0.5× (be cautious)
        }
        ExtremeZone::Panic => hi5_base_budget * Decimal::new(2, 0),        // 2×
        ExtremeZone::ExtremePanic => hi5_base_budget * Decimal::new(3, 0), // 3×
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_when_no_extreme_pillars() {
        let snap = classify_zone(
            Some(40.0), Some(30.0), // AAII balanced
            Some(80.0),             // NAAIM high
            Some(70.0),             // Breadth healthy
            None, None, None,
        );
        assert_eq!(snap.zone, ExtremeZone::Normal);
        assert_eq!(snap.extreme_pillar_count, 0);
    }

    #[test]
    fn caution_when_one_pillar_extreme() {
        let snap = classify_zone(
            Some(20.0), Some(60.0), // AAII extreme (bulls low, bears high)
            Some(80.0),             // NAAIM healthy
            Some(70.0),             // Breadth healthy
            None, None, None,
        );
        assert_eq!(snap.zone, ExtremeZone::Caution);
        assert_eq!(snap.extreme_pillar_count, 1);
    }

    #[test]
    fn panic_when_two_pillars_extreme() {
        let snap = classify_zone(
            Some(20.0), Some(60.0), // AAII extreme
            Some(30.0),             // NAAIM extreme
            Some(70.0),             // Breadth healthy
            None, None, None,
        );
        assert_eq!(snap.zone, ExtremeZone::Panic);
        assert_eq!(snap.extreme_pillar_count, 2);
    }

    #[test]
    fn extreme_panic_all_three_plus_vix() {
        let snap = classify_zone(
            Some(15.0), Some(65.0), // AAII extreme
            Some(25.0),             // NAAIM extreme
            Some(20.0),             // Breadth extreme
            Some(40.0),             // VIX ≥ 35
            Some(-0.04),            // RSP daily ≤ -3%
            None,
        );
        assert_eq!(snap.zone, ExtremeZone::ExtremePanic);
        assert_eq!(snap.extreme_pillar_count, 3);
    }

    #[test]
    fn multiplier_values() {
        assert_eq!(ExtremeZone::Normal.multiplier(), 0.5);
        assert_eq!(ExtremeZone::Caution.multiplier(), 0.5);
        assert_eq!(ExtremeZone::Panic.multiplier(), 2.0);
        assert_eq!(ExtremeZone::ExtremePanic.multiplier(), 3.0);
    }

    #[test]
    fn hi5e_dynamic_budget_scales() {
        let base = Decimal::new(1000, 0);
        assert_eq!(
            hi5e_dynamic_budget(base, ExtremeZone::Normal),
            Decimal::new(500, 0)
        );
        assert_eq!(
            hi5e_dynamic_budget(base, ExtremeZone::Panic),
            Decimal::new(2000, 0)
        );
        assert_eq!(
            hi5e_dynamic_budget(base, ExtremeZone::ExtremePanic),
            Decimal::new(3000, 0)
        );
    }
}
