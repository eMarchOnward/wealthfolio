//! Success probability: re-solve a plan over many return and inflation paths
//! (Owl's historical back-test and bootstrap Monte Carlo) and count the paths
//! on which its spending can still be funded.
//!
//! Each account earns its own asset mix's return on the path's market history
//! (see [`crate::markets`]).

use serde::{Deserialize, Serialize};

use crate::error::{EngineError, Result};
use crate::markets::{account_returns, history, AccountMixes, ClassRates};
use crate::planner::optimize;
use crate::scenario::{Objective, Scenario};

/// Yearly account returns and inflation for one simulated lifetime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReturnPath {
    /// Label: the first historical year, or the bootstrap sample number.
    pub label: String,
    /// `[taxable, tax-deferred, Roth, HSA]` returns per year.
    pub returns: Vec<[f64; 4]>,
    pub inflation: Vec<f64>,
}

/// The path through `years` (one per plan year), each account earning its mix
/// for that plan year (past the end of `mixes`, the last one) less `fee`.
fn path(label: String, years: &[ClassRates], mixes: &[AccountMixes], fee: f64) -> ReturnPath {
    let last = mixes.last().copied().unwrap_or_default();
    ReturnPath {
        label,
        returns: years
            .iter()
            .enumerate()
            .map(|(n, y)| account_returns(mixes.get(n).unwrap_or(&last), y, fee))
            .collect(),
        inflation: years.iter().map(|y| y.inflation).collect(),
    }
}

/// Every historical window of `years` consecutive years.
pub fn historical_paths(years: usize, mixes: &[AccountMixes], fee: f64) -> Vec<ReturnPath> {
    let data = history();
    if years == 0 || years > data.len() || mixes.is_empty() {
        return Vec::new();
    }
    data.windows(years)
        .map(|window| {
            let picks: Vec<ClassRates> = window.iter().map(ClassRates::from).collect();
            path(window[0].year.to_string(), &picks, mixes, fee)
        })
        .collect()
}

/// `count` paths drawing whole historical years with replacement (keeping each
/// year's asset-class returns and inflation together), from a fixed `seed`.
pub fn bootstrap_paths(
    years: usize,
    mixes: &[AccountMixes],
    fee: f64,
    count: usize,
    seed: u64,
) -> Vec<ReturnPath> {
    let data = history();
    if mixes.is_empty() {
        return Vec::new();
    }
    let mut rng = SplitMix64(seed);
    (0..count)
        .map(|k| {
            let picks: Vec<ClassRates> = (0..years)
                .map(|_| ClassRates::from(&data[(rng.next() % data.len() as u64) as usize]))
                .collect();
            path(format!("sample {}", k + 1), &picks, mixes, fee)
        })
        .collect()
}

/// `count` paths drawing each year's asset-class returns and inflation from a
/// correlated lognormal fitted to the whole history (Owl's historical
/// lognormal), from a fixed `seed`. Unlike resampling, years can be worse (or
/// better) than any on record.
pub fn lognormal_paths(
    years: usize,
    mixes: &[AccountMixes],
    fee: f64,
    count: usize,
    seed: u64,
) -> Vec<ReturnPath> {
    let data = history();
    if mixes.is_empty() || data.len() < 2 {
        return Vec::new();
    }
    let logs: Vec<[f64; 4]> = data
        .iter()
        .map(|y| [y.stocks, y.bonds, y.notes, y.inflation].map(|r| (1.0 + r).max(1e-10).ln()))
        .collect();
    let n = logs.len() as f64;
    let mean: [f64; 4] = std::array::from_fn(|i| logs.iter().map(|x| x[i]).sum::<f64>() / n);
    let cov: [[f64; 4]; 4] = std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            logs.iter()
                .map(|x| (x[i] - mean[i]) * (x[j] - mean[j]))
                .sum::<f64>()
                / (n - 1.0)
        })
    });
    let chol = cholesky(&cov);
    let mut rng = SplitMix64(seed);
    (0..count)
        .map(|k| {
            let picks: Vec<ClassRates> = (0..years)
                .map(|_| {
                    let z: [f64; 4] = std::array::from_fn(|_| rng.normal());
                    let x: [f64; 4] = std::array::from_fn(|i| {
                        mean[i] + (0..=i).map(|j| chol[i][j] * z[j]).sum::<f64>()
                    });
                    ClassRates {
                        stocks: x[0].exp() - 1.0,
                        bonds: x[1].exp() - 1.0,
                        notes: x[2].exp() - 1.0,
                        inflation: x[3].exp() - 1.0,
                    }
                })
                .collect();
            path(format!("sample {}", k + 1), &picks, mixes, fee)
        })
        .collect()
}

/// Lower-triangular `l` with `l * l^T = a` for a covariance matrix; a
/// dimension with no variance left gets none.
fn cholesky(a: &[[f64; 4]; 4]) -> [[f64; 4]; 4] {
    let mut l = [[0.0; 4]; 4];
    for i in 0..4 {
        for j in 0..=i {
            let sum: f64 = (0..j).map(|k| l[i][k] * l[j][k]).sum();
            if i == j {
                l[i][j] = (a[i][i] - sum).max(0.0).sqrt();
            } else if l[j][j] > 0.0 {
                l[i][j] = (a[i][j] - sum) / l[j][j];
            }
        }
    }
    l
}

/// Small, dependency-free PRNG (SplitMix64); enough for resampling years.
pub(crate) struct SplitMix64(pub(crate) u64);

impl SplitMix64 {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in (0, 1].
    pub(crate) fn unit(&mut self) -> f64 {
        ((self.next() >> 11) as f64 + 1.0) / (1u64 << 53) as f64
    }

    /// Standard normal (Box-Muller).
    fn normal(&mut self) -> f64 {
        let (u, v) = (self.unit(), self.unit());
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos()
    }
}

/// Outcome of one path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathOutcome {
    pub label: String,
    pub funded: bool,
    /// After-tax estate in today's dollars (deflated by the path's own
    /// inflation), when funded.
    pub bequest_today: Option<f64>,
}

/// Success probability of spending `net_spending` (today's dollars, following
/// the scenario's spending profile) across `paths`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SuccessReport {
    pub net_spending: f64,
    pub paths: usize,
    pub funded: usize,
    /// `funded / paths`.
    pub success_rate: f64,
    /// 10th, 50th and 90th percentile estates among all paths (unfunded paths
    /// count as 0).
    pub bequest_percentiles: [f64; 3],
    pub outcomes: Vec<PathOutcome>,
}

/// One simulated lifetime: the plan re-solved on one market path. Runs can
/// differ in plan length (a random lifespan); each path covers its plan.
#[derive(Debug, Clone, Copy)]
pub struct Run<'a> {
    pub scenario: &'a Scenario,
    pub path: &'a ReturnPath,
}

/// `runs` for `scenario` on every path.
fn same_plan<'a>(scenario: &'a Scenario, paths: &'a [ReturnPath]) -> Vec<Run<'a>> {
    paths.iter().map(|path| Run { scenario, path }).collect()
}

/// Checks every run's plan and that its path covers it.
fn validate_runs(runs: &[Run]) -> Result<()> {
    for run in runs {
        run.scenario.validate()?;
        let years = run.scenario.years();
        if run.path.returns.len() < years || run.path.inflation.len() < years {
            return Err(EngineError::InvalidInput(format!(
                "every path needs {years} years of returns and inflation"
            )));
        }
    }
    Ok(())
}

/// `run`'s plan for a quick solve with `objective` on its path's markets.
fn on_path(run: &Run, objective: Objective) -> Scenario {
    let mut s = run.scenario.clone();
    let years = s.years();
    s.objective = objective;
    s.options.polish = false;
    s.options.max_iterations = s.options.max_iterations.min(12);
    s.rates.account_returns = run.path.returns[..years].to_vec();
    s.rates.annual_inflation = run.path.inflation[..years].to_vec();
    s
}

/// Re-solves `scenario` on every path at fixed spending, on up to `threads`
/// worker threads. Each solve is the fast self-consistent loop without the
/// polish; a path is funded when that plan exists.
pub fn success_probability(
    scenario: &Scenario,
    net_spending: f64,
    paths: &[ReturnPath],
    threads: usize,
) -> Result<SuccessReport> {
    success_probability_of_runs(&same_plan(scenario, paths), net_spending, threads)
}

/// [`success_probability`] where each run brings its own plan. A run the
/// solver gives up on counts as unfunded (as Owl counts a failed solve), so one
/// extreme path does not sink the rest.
pub fn success_probability_of_runs(
    runs: &[Run],
    net_spending: f64,
    threads: usize,
) -> Result<SuccessReport> {
    validate_runs(runs)?;
    let outcomes = map_paths(runs, threads, |run| {
        let label = run.path.label.clone();
        match optimize(&on_path(run, Objective::MaxBequest { net_spending })) {
            Ok(schedule) => Ok(PathOutcome {
                label,
                funded: true,
                bequest_today: Some(schedule.bequest_after_tax_today),
            }),
            Err(EngineError::Infeasible(_)) | Err(EngineError::Solver(_)) => Ok(PathOutcome {
                label,
                funded: false,
                bequest_today: None,
            }),
            Err(other) => Err(other),
        }
    })?;

    let funded = outcomes.iter().filter(|o| o.funded).count();
    let mut estates: Vec<f64> = outcomes
        .iter()
        .map(|o| o.bequest_today.unwrap_or(0.0))
        .collect();
    estates.sort_by(f64::total_cmp);
    let percentile = |p: f64| -> f64 {
        if estates.is_empty() {
            return 0.0;
        }
        let i = ((estates.len() - 1) as f64 * p).round() as usize;
        estates[i]
    };
    Ok(SuccessReport {
        net_spending,
        paths: outcomes.len(),
        funded,
        success_rate: if outcomes.is_empty() {
            0.0
        } else {
            funded as f64 / outcomes.len() as f64
        },
        bequest_percentiles: [percentile(0.1), percentile(0.5), percentile(0.9)],
        outcomes,
    })
}

/// `run` on every item, on up to `threads` worker threads, in order.
fn map_paths<T: Sync, R: Send>(
    paths: &[T],
    threads: usize,
    run: impl Fn(&T) -> Result<R> + Sync,
) -> Result<Vec<R>> {
    let threads = threads.clamp(1, 16);
    let chunk = paths.len().div_ceil(threads).max(1);
    let run = &run;
    std::thread::scope(|scope| {
        let handles: Vec<_> = paths
            .chunks(chunk)
            .map(|batch| scope.spawn(move || batch.iter().map(run).collect::<Result<Vec<_>>>()))
            .collect();
        let mut all = Vec::with_capacity(paths.len());
        for handle in handles {
            let batch = handle
                .join()
                .map_err(|_| EngineError::Solver("a simulation thread panicked".into()))??;
            all.extend(batch);
        }
        Ok(all)
    })
}

/// Share taken off each path's most spending: at the very edge, the
/// self-consistent tax loop of a fixed-spending solve can settle on a slightly
/// different point and miss by a few dollars.
const CAPACITY_MARGIN: f64 = 0.01;

/// The most one path can spend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PathCapacity {
    pub label: String,
    /// First-year net spending in today's dollars (following the scenario's
    /// spending profile) the path funds, less [`CAPACITY_MARGIN`]; 0 when even
    /// the required spending cannot be met.
    pub max_spending: f64,
}

/// Solves `scenario` for the most spending on every path, leaving nothing
/// required at the end (as [`success_probability`] does). A path funds any
/// spending up to its capacity, so these give the success rate of every
/// spending level at once (Owl's spending frontier). With spending
/// flexibility, the first year may sit below the most the path could fund
/// in that year alone, so the capacities are slightly conservative.
pub fn spending_capacities(
    scenario: &Scenario,
    paths: &[ReturnPath],
    threads: usize,
) -> Result<Vec<PathCapacity>> {
    spending_capacities_of_runs(&same_plan(scenario, paths), threads)
}

/// [`spending_capacities`] where each run brings its own plan; a run the
/// solver gives up on funds nothing (as in [`success_probability_of_runs`]).
pub fn spending_capacities_of_runs(runs: &[Run], threads: usize) -> Result<Vec<PathCapacity>> {
    validate_runs(runs)?;
    map_paths(runs, threads, |run| {
        let s = on_path(run, Objective::MaxSpending { bequest_floor: 0.0 });
        let max_spending = match optimize(&s) {
            Ok(schedule) => schedule.first_year_spending().max(0.0) * (1.0 - CAPACITY_MARGIN),
            Err(EngineError::Infeasible(_)) | Err(EngineError::Solver(_)) => 0.0,
            Err(other) => return Err(other),
        };
        Ok(PathCapacity {
            label: run.path.label.clone(),
            max_spending,
        })
    })
}

/// The most first-year spending that at least `rate` of the paths fund.
pub fn spending_at_success_rate(capacities: &[PathCapacity], rate: f64) -> f64 {
    let mut caps: Vec<f64> = capacities.iter().map(|c| c.max_spending).collect();
    if caps.is_empty() {
        return 0.0;
    }
    caps.sort_by(|a, b| b.total_cmp(a));
    let needed = ((rate.clamp(0.0, 1.0) * caps.len() as f64).ceil() as usize).max(1);
    caps[needed.min(caps.len()) - 1]
}

/// Success rate at each path's capacity, from the lowest spending up: the
/// spending/success trade-off curve.
pub fn success_frontier(capacities: &[PathCapacity]) -> Vec<(f64, f64)> {
    let mut caps: Vec<f64> = capacities.iter().map(|c| c.max_spending).collect();
    caps.sort_by(f64::total_cmp);
    let n = caps.len() as f64;
    caps.iter()
        .enumerate()
        .map(|(k, spending)| (*spending, (caps.len() - k) as f64 / n))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markets::AssetMix;
    use crate::markets::HistoricalYear;

    #[test]
    fn paths_follow_each_accounts_mix() {
        let sixty_forty = [AssetMix::stocks_bonds(0.6); 4];
        let paths = historical_paths(30, &[sixty_forty], 0.005);
        assert_eq!(paths.len(), 98 - 30 + 1);
        assert_eq!(paths[0].label, "1928");
        let expected = 0.6 * 0.4381 + 0.4 * 0.0322 - 0.005;
        assert!((paths[0].returns[0][0] - expected).abs() < 1e-12);
        assert!(historical_paths(99, &[sixty_forty], 0.0).is_empty());
        assert!(historical_paths(30, &[], 0.0).is_empty());

        // Taxable all stocks, the rest in cash earning inflation; the glide
        // moves the taxable account to cash in the second year.
        let cash = AssetMix {
            cash: 1.0,
            ..AssetMix::default()
        };
        let stocks = AssetMix::stocks_bonds(1.0);
        let glide = [[stocks, cash, cash, cash], [cash; 4]];
        let p = &historical_paths(3, &glide, 0.0)[0];
        assert!((p.returns[0][0] - 0.4381).abs() < 1e-12);
        assert!((p.returns[0][3] + 0.0116).abs() < 1e-12);
        assert!((p.returns[1][0] - 0.0058).abs() < 1e-12);
        // The last mix carries on: all cash, earning 1930's inflation.
        assert_eq!(p.returns[2], [-0.064; 4]);

        let a = bootstrap_paths(40, &[sixty_forty], 0.0, 5, 7);
        let b = bootstrap_paths(40, &[sixty_forty], 0.0, 5, 7);
        assert_eq!(a, b, "a seed reproduces its samples");
        assert_eq!(a.len(), 5);
        assert!(a.iter().all(|p| p.returns.len() == 40));
    }

    #[test]
    fn lognormal_paths_match_the_history_on_average() {
        let stocks = [AssetMix::stocks_bonds(1.0); 4];
        let a = lognormal_paths(50, &[stocks], 0.0, 200, 11);
        assert_eq!(a, lognormal_paths(50, &[stocks], 0.0, 200, 11));
        assert_eq!(a.len(), 200);
        assert!(a
            .iter()
            .all(|p| p.returns.len() == 50 && p.inflation.len() == 50));

        let mean_log = |pick: fn(&HistoricalYear) -> f64| {
            let data = history();
            data.iter().map(|y| (1.0 + pick(y)).ln()).sum::<f64>() / data.len() as f64
        };
        let sample_mean = |values: &[f64]| {
            values.iter().map(|r| (1.0 + r).ln()).sum::<f64>() / values.len() as f64
        };
        let draws: Vec<f64> = a
            .iter()
            .flat_map(|p| p.returns.iter().map(|r| r[0]))
            .collect();
        assert!((sample_mean(&draws) - mean_log(|y| y.stocks)).abs() < 0.01);
        let inflation: Vec<f64> = a.iter().flat_map(|p| p.inflation.clone()).collect();
        assert!((sample_mean(&inflation) - mean_log(|y| y.inflation)).abs() < 0.005);
        // Draws are not limited to the years on record.
        let data = history();
        let worst = data.iter().map(|y| y.stocks).fold(f64::MAX, f64::min);
        let best = data.iter().map(|y| y.stocks).fold(f64::MIN, f64::max);
        assert!(draws.iter().any(|r| *r < worst || *r > best));
        assert!(lognormal_paths(5, &[], 0.0, 3, 1).is_empty());
    }

    #[test]
    fn cholesky_rebuilds_the_matrix() {
        let a = [
            [4.0, 2.0, 0.4, 0.0],
            [2.0, 5.0, 1.0, 0.0],
            [0.4, 1.0, 3.0, 0.5],
            [0.0, 0.0, 0.5, 1.0],
        ];
        let l = cholesky(&a);
        for i in 0..4 {
            for j in 0..4 {
                let v: f64 = (0..4).map(|k| l[i][k] * l[j][k]).sum();
                assert!((v - a[i][j]).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn success_rate_spending_picks_the_right_capacity() {
        let caps: Vec<PathCapacity> = [50.0, 10.0, 40.0, 20.0, 30.0]
            .iter()
            .map(|m| PathCapacity {
                label: String::new(),
                max_spending: *m,
            })
            .collect();
        // 80% of 5 paths is 4: the fourth highest capacity.
        assert_eq!(spending_at_success_rate(&caps, 0.8), 20.0);
        assert_eq!(spending_at_success_rate(&caps, 1.0), 10.0);
        assert_eq!(spending_at_success_rate(&caps, 0.1), 50.0);
        let frontier = success_frontier(&caps);
        assert_eq!(frontier.first(), Some(&(10.0, 1.0)));
        assert_eq!(frontier.last(), Some(&(50.0, 0.2)));
    }
}
