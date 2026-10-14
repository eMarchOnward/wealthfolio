//! Asset classes and market history, as in Owl: the S&P 500 (with dividends),
//! Baa corporate bonds, 10-year Treasury notes, and cash earning inflation.
//!
//! The annual series, 1928–2025, are Owl's `data/rates.csv` (Damodaran).

use serde::{Deserialize, Serialize};

const HISTORY_CSV: &str = include_str!("data/historical_returns.csv");

/// One year of market history, as decimals.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HistoricalYear {
    pub year: i32,
    pub stocks: f64,
    pub bonds: f64,
    pub notes: f64,
    pub inflation: f64,
}

/// The embedded 1928–2025 series.
pub fn history() -> Vec<HistoricalYear> {
    HISTORY_CSV
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split(',').map(str::trim);
            let year = fields.next()?.parse().ok()?;
            let mut pct = || fields.next()?.parse::<f64>().ok().map(|v| v / 100.0);
            Some(HistoricalYear {
                year,
                stocks: pct()?,
                bonds: pct()?,
                notes: pct()?,
                inflation: pct()?,
            })
        })
        .collect()
}

/// One year's rates of the asset classes and inflation, as decimals.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassRates {
    pub stocks: f64,
    pub bonds: f64,
    pub notes: f64,
    pub inflation: f64,
}

impl From<&HistoricalYear> for ClassRates {
    fn from(y: &HistoricalYear) -> Self {
        Self {
            stocks: y.stocks,
            bonds: y.bonds,
            notes: y.notes,
            inflation: y.inflation,
        }
    }
}

/// How an account is invested: weights of stocks, bonds, T-notes and cash.
/// Weights need not sum to 1; they are normalized when used.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetMix {
    pub stocks: f64,
    pub bonds: f64,
    pub notes: f64,
    pub cash: f64,
}

impl AssetMix {
    /// `equity_share` in stocks, the rest in bonds.
    pub fn stocks_bonds(equity_share: f64) -> Self {
        let equity = equity_share.clamp(0.0, 1.0);
        Self {
            stocks: equity,
            bonds: 1.0 - equity,
            ..Self::default()
        }
    }

    fn weights(&self) -> [f64; 4] {
        [self.stocks, self.bonds, self.notes, self.cash].map(|w| w.max(0.0))
    }

    /// The weights scaled to sum to 1, or `None` when they are all zero.
    pub fn normalized(&self) -> Option<Self> {
        let w = self.weights();
        let total: f64 = w.iter().sum();
        (total > 1e-12).then(|| Self {
            stocks: w[0] / total,
            bonds: w[1] / total,
            notes: w[2] / total,
            cash: w[3] / total,
        })
    }

    /// Return of the mix in a year with `rates`; cash earns inflation, as in Owl.
    pub fn return_on(&self, rates: &ClassRates) -> f64 {
        let m = self.normalized().unwrap_or_default();
        m.stocks * rates.stocks
            + m.bonds * rates.bonds
            + m.notes * rates.notes
            + m.cash * rates.inflation
    }

    /// The mix `t` of the way (0–1) from `self` to `target`.
    pub fn toward(&self, target: &Self, t: f64) -> Self {
        let (a, b) = (
            self.normalized().unwrap_or_default(),
            target.normalized().unwrap_or_default(),
        );
        let mix = |x: f64, y: f64| x + (y - x) * t;
        Self {
            stocks: mix(a.stocks, b.stocks),
            bonds: mix(a.bonds, b.bonds),
            notes: mix(a.notes, b.notes),
            cash: mix(a.cash, b.cash),
        }
    }
}

/// Mixes of the `[taxable, tax-deferred, Roth, HSA]` accounts in one year.
pub type AccountMixes = [AssetMix; 4];

/// Net returns of each account in a year: its mix's return less `fee`.
pub fn account_returns(mixes: &AccountMixes, rates: &ClassRates, fee: f64) -> [f64; 4] {
    mixes.map(|m| m.return_on(rates) - fee)
}

/// Geometric mean of each series over `from..=to` (Owl's historical average).
pub fn historical_average(from: i32, to: i32) -> Option<ClassRates> {
    let years: Vec<HistoricalYear> = history()
        .into_iter()
        .filter(|y| (from..=to).contains(&y.year))
        .collect();
    if years.len() < 2 {
        return None;
    }
    let geometric = |pick: fn(&HistoricalYear) -> f64| {
        let log_sum: f64 = years.iter().map(|y| (1.0 + pick(y)).max(1e-10).ln()).sum();
        (log_sum / years.len() as f64).exp() - 1.0
    };
    Some(ClassRates {
        stocks: geometric(|y| y.stocks),
        bonds: geometric(|y| y.bonds),
        notes: geometric(|y| y.notes),
        inflation: geometric(|y| y.inflation),
    })
}

/// `years` consecutive historical years from `start`, wrapping to the first
/// year after the last; `None` when `start` is outside the history.
pub fn historical_sequence(start: i32, years: usize) -> Option<Vec<ClassRates>> {
    let data = history();
    let first = data.iter().position(|y| y.year == start)?;
    Some(
        (0..years)
            .map(|k| ClassRates::from(&data[(first + k) % data.len()]))
            .collect(),
    )
}

/// How a mix moves toward its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum GlideShape {
    /// Equal steps every year.
    #[default]
    Linear,
    /// Slow, then fast around the middle, then slow (Owl's tanh interpolation).
    SCurve,
}

/// How far (0–1) each of `years` plan years is along a glide that arrives
/// after `span` years and stays there.
///
/// The S-curve follows Owl's `tanh` interpolation, its 15-year center and
/// 5-year width scaled to the span (exact for a 30-year glide).
pub fn glide_fractions(years: usize, span: usize, shape: GlideShape) -> Vec<f64> {
    if span == 0 {
        return vec![1.0; years];
    }
    let s = span as f64;
    let curve = |t: f64| match shape {
        GlideShape::Linear => t / s,
        GlideShape::SCurve => {
            let (center, width) = (s / 2.0, s / 6.0);
            (((t - center) / width).tanh() + 1.0) / 2.0
        }
    };
    let (start, end) = (curve(0.0), curve(s));
    (0..years)
        .map(|n| {
            if n >= span {
                1.0
            } else {
                ((curve(n as f64) - start) / (end - start)).clamp(0.0, 1.0)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_has_four_series_from_1928_to_2025() {
        let data = history();
        assert_eq!(data.len(), 98);
        assert_eq!(data.first().map(|y| y.year), Some(1928));
        assert_eq!(data.last().map(|y| y.year), Some(2025));
        // 2025: S&P 17.72%, Baa 6.96%, T-notes 7.8%, inflation 2.74%.
        let last = data[97];
        assert!((last.notes - 0.078).abs() < 1e-12);
        assert!((last.inflation - 0.0274).abs() < 1e-12);
    }

    #[test]
    fn mixes_weight_the_classes_and_cash_earns_inflation() {
        let rates = ClassRates {
            stocks: 0.10,
            bonds: 0.05,
            notes: 0.03,
            inflation: 0.02,
        };
        let mix = AssetMix {
            stocks: 6.0,
            bonds: 2.0,
            notes: 1.0,
            cash: 1.0,
        };
        let expected = 0.6 * 0.10 + 0.2 * 0.05 + 0.1 * 0.03 + 0.1 * 0.02;
        assert!((mix.return_on(&rates) - expected).abs() < 1e-12);
        assert_eq!(AssetMix::default().return_on(&rates), 0.0);

        let half = AssetMix::stocks_bonds(1.0).toward(&AssetMix::stocks_bonds(0.0), 0.5);
        assert!((half.stocks - 0.5).abs() < 1e-12 && (half.bonds - 0.5).abs() < 1e-12);
    }

    #[test]
    fn averages_are_geometric_and_sequences_wrap() {
        let two = historical_average(1928, 1929).expect("average");
        let expected = ((1.4381_f64 * 0.917).sqrt()) - 1.0;
        assert!((two.stocks - expected).abs() < 1e-12);
        assert!(historical_average(1800, 1801).is_none());

        let seq = historical_sequence(2024, 3).expect("sequence");
        assert!((seq[0].stocks - 0.2488).abs() < 1e-12);
        assert!((seq[2].stocks - 0.4381).abs() < 1e-12, "wraps to 1928");
        assert!(historical_sequence(1900, 3).is_none());
    }

    #[test]
    fn glides_reach_the_target_at_the_span_and_stay() {
        let linear = glide_fractions(6, 4, GlideShape::Linear);
        assert_eq!(linear, vec![0.0, 0.25, 0.5, 0.75, 1.0, 1.0]);

        let s = glide_fractions(32, 30, GlideShape::SCurve);
        assert_eq!(s[0], 0.0);
        assert!((s[15] - 0.5).abs() < 1e-9, "inflection at the middle");
        assert!(s[5] < 5.0 / 30.0, "starts slower than linear");
        assert_eq!(s[30], 1.0);
        assert_eq!(s[31], 1.0);
        assert!(s.windows(2).all(|w| w[1] >= w[0]));

        assert_eq!(glide_fractions(3, 0, GlideShape::SCurve), vec![1.0; 3]);
    }
}
