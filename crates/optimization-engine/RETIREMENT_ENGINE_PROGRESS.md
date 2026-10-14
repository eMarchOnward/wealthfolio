# Retirement Optimization Engine — Progress Summary

Headless Rust port of the mixed-integer linear programming (MILP) retirement
model from [Owl](https://github.com/mdlacasse/Owl) (`owlplanner`, version
2026.10.1, commit `a85ff769`), added as the workspace crate
`crates/optimization-engine` (package `wealthfolio-optimization-engine`).

**Status:** complete for a single filer. 46 tests pass (36 unit, 10
integration), `cargo fmt` and `cargo clippy -D warnings` are clean, and the
standard-retiree scenario solves to proven optimality in about one second.

## What it does

Given balances, rates, Social Security and a goal, it chooses year by year:
traditional IRA withdrawals, taxable liquidations, Roth withdrawals, Roth
conversions, surplus reinvestment and (optionally) the Social Security claiming
month, under federal tax, LTCG stacking, IRMAA cliffs and RMDs. It supports
both of Owl's objectives:

| Objective | Constraint | Maximizes |
|---|---|---|
| `MaxSpending { bequest_floor }` | after-heir-tax estate ≥ floor (today's $) | Σ net spending in today's $ |
| `MaxBequest { net_spending }` | spending fixed (today's $, follows profile) | after-heir-tax estate |

```rust
use wealthfolio_optimization_engine::{optimize, Scenario};
let schedule = optimize(&scenario)?; // Schedule { years: Vec<YearPlan>, .. }
```

## Module map

| Module | Contents | Owl source |
|---|---|---|
| `tax::federal` | OBBBA brackets, standard deduction (+65, senior bonus phase-out to 2028), LTCG stacking, SS taxability (Pub. 915, 85% cap), NIIT | `tax_federal.py` |
| `tax::irmaa` | Part B + D IRMAA tiers (CMS 2026), cumulative costs, two-year lookback helpers | `tax_federal.py` (`mediVals`, `mediCosts`) |
| `social_security` | FRA by birth date, 5/9%–5/12% reductions, 8% delayed credits, 97-point monthly grid 62–70 | `socialsecurity.py` |
| `mortality` | SSA 2025 period life table (generated from Owl's data), survival, life expectancy, longevity-percentile horizon | `data/mortality_tables.py` |
| `rmd` | SECURE 1.0/2.0 start ages, Uniform Lifetime divisors | `tax_federal.rho_in`, `data/irs_590b.py` |
| `scenario` | Inputs and validation | `plan.py` setters |
| `model` | `good_lp` formulation, tagged constraint registry | `plan.py` `_buildConstraints` |
| `planner` | Self-consistent loop, HiGHS solve, schedule extraction | `plan.py` `solve`/`_scSolve` |
| `diagnostics` | Elastic-filter infeasibility report | (new; Owl has none) |
| `schedule` | `Schedule`, `YearPlan`, `SolveStatus`, `Convergence`, `LoopResidual` | `plan.py` result frames |

## MILP formulation (per plan year *n*)

Decision variables, with the names the brief asked for:

- `traditional_ira_withdrawal_n`, `taxable_brokerage_liquidation_n`,
  `roth_withdrawal_n`, `roth_conversion_n` (continuous ≥ 0)
- `social_security_active_n` (binary), defined from the claiming-month binaries
  `claim_month_k` (exactly one of 97)
- balances (start of year) for taxable, tax-deferred and Roth accounts;
  `taxable_deposit_n`, `net_spending_n`
- ordinary bracket fill `bracket_t_n` (bounded by bracket width × inflation),
  `deduction_used_n` (≤ standard deduction), LTCG bands `ltcg_band_{0,1,2}_n`
- Medicare: `medicare_n`, IRMAA MAGI portions and tier binaries `irmaa_tier_q_n`
  (exactly one tier; MAGI two years back decides it)

Core rows:

- **Cash flow:** spending + deposits + Σ rateₜ·bracketₜ + 15%·band₁ + 20%·band₂
  + Medicare = withdrawals × (1 − early penalty) + Social Security + other income.
- **Taxable income:** deduction used + Σ bracket fill = IRA withdrawals +
  conversions + taxable SS + other income.
- **Carryover:** next balance = (1 + r)(balance − withdrawals ± conversions + deposits).
- **RMD:** IRA withdrawal ≥ divisor fraction × start-of-year balance.
- **Spending profile** (Owl `lambdha`), **bequest floor** or **fixed spending**.
- **LTCG:** bands cover realized gains (qualified dividends + gain fraction of
  liquidations); band room set by ordinary income (see loop below). An exact
  big-M variant is available with `optimize_ltcg: true`.
- **IRMAA:** one-hot tier selection with MAGI decomposition (Owl's
  `withMedicare="optimize"`); `optimize_medicare: false` uses loop mode.
- Tie-breakers (Owl ε = 5e-7): convert earlier, keep money in Roth, avoid
  withdraw-and-redeposit.

## Self-consistent loop

As in Owl, non-linear pieces are fixed parameters within one solve and updated
between solves: the taxable fraction of Social Security, the MAGI behind the
senior deduction, the capital-gain fraction (average-cost basis tracking), the
LTCG band room (ordinary income), and loop-mode Medicare premiums.

- Convergence follows Owl: the objective settles (≤ $100 or 5e-5 relative) **and**
  the residual is ≤ $50/year. The residual is measured here as the gap between
  the modelled federal tax and the statutory tax of the resulting plan
  (`YearPlan::statutory_federal_tax`), in today's dollars.
- Owl's damping of the SS fraction (30% new) is kept; the ordinary-income
  estimate is damped 50/50.
- Cycle detection (lengths 2–4) and a max of 29 iterations as in Owl; when not
  converged, the most self-consistent iterate (lowest residual) is returned.

## Solver stack

`good_lp` 1.15 with the `highs` feature (HiGHS compiled from source via
`highs-sys`). No solver was written from scratch.

Build prerequisites on this Windows machine (resolved during the port):

- **CMake:** satisfied by the CMake bundled with Strawberry Perl
  (`C:\Strawberry\c\bin`), already on PATH.
- **libclang** (for `bindgen`): taken from the `libclang` Python wheel, copied to
  `%LOCALAPPDATA%\libclang`; set `LIBCLANG_PATH` to that folder before building.
- **MSVC:** build from the VS 2022 Build Tools developer environment, as for the
  rest of the workspace.

## Tests (`cargo test -p wealthfolio-optimization-engine`)

Unit tests cover every module: tax math against hand calculations, IRMAA cliffs,
the SS claiming curve endpoints (70%/100%/124%), mortality tables, RMD cohorts,
input validation, the benefit table and claim eligibility, model structure
(row counts per family), loop damping, cycle detection, and report formatting.

Integration tests (`tests/standard_retiree.rs`): $1M traditional, $500k taxable
($350k basis), single male born June 1964, retiring at 62 in 2026, PIA
$2,800/month, 5% returns, 2.5% inflation, horizon to age 95. Every schedule is
re-verified independently: cash flow balances to $0.001, carryover identities,
non-negative balances, RMDs met, IRMAA tiers match the two-year-lookback MAGI,
ordinary tax exact, and modelled vs statutory tax (with SS taxability and the
deduction re-derived by the test) within $250 in any year and $50/year on average.

| Scenario | Result |
|---|---|
| MaxBequest, $80k/yr (today's $) | Optimal (0% gap), converged in 11 iterations; estate $77,357 (today's $) |
| MaxSpending, no floor | $81,420/yr sustainable |
| MaxSpending, $250k floor | $76,725/yr; estate exactly $250k |
| Duality check | MaxBequest at $76,725/yr leaves $249,197 (within $803 of the floor) |
| Optimized claiming age | Converges; at least as good as a fixed age-67 claim |
| Roth cap, loop-mode Medicare, longevity horizon (p10 = SSA table) | Pass |
| $400k/yr spending | Infeasible: "fixed net spending short by at least $…" |
| $5M bequest floor | Infeasible: "bequest floor short by at least $1,385,397 ($598,360 today)" |

The plan the optimizer finds is the textbook one: from ages 62–65 it converts
exactly up to the top of the 12% bracket while living off the taxable account,
then draws the IRA, and late in life liquidates taxable shares inside the 0% band.

## Infeasibility debugging

`diagnose_infeasibility(&scenario)` (also run automatically when `optimize`
fails) re-solves with weighted slack on goal and money-flow rows only: goals
(spending, profile, bequest) weight 1, RMD weight 10, cash flow weight 100.
Starting balances, account dynamics and tax definitions stay hard, so the report
names the goal that cannot be met and by how much, plus the binding constraints
in that year. It runs on the LP relaxation for speed (milliseconds), so amounts
are lower bounds on the true gap (the test checks it is within 25%). An
infeasible LP relaxation also short-circuits the MILP, so infeasible scenarios
fail fast instead of running branch-and-bound to the time limit.

## Assumptions and deviations from Owl

- **Single owner only.** Owl's two-spouse model (survivor benefits, MFJ→single
  at first death, Joint Life RMD table, spousal benefits) is deferred. `FilingStatus`
  and the IRMAA/tax tables already carry the MFJ columns.
- **LTCG loop mode by default** (Owl's default too). The exact big-M MILP mode is
  implemented but took over 120 s per solve on the standard case (0.37% gap), so
  it is opt-in.
- **SS taxability uses the loop** (Owl's default `loop` mode), not Owl's optional
  MILP mode.
- **Loop stabilizer (new):** a proximal penalty (default 0.05 per dollar) on
  moving ordinary income between iterations, only in years that realize gains.
  Without it the loop cycled indefinitely; it vanishes at a fixed point, so
  converged plans are genuinely self-consistent. Weights of 0.05–0.10 gave the
  best plans; 0.2 and above locked in worse ones (`examples/duality.rs`).
- **NIIT** is implemented as a function but not modelled in the MILP (it only
  applies above $200k MAGI single).
- Not ported: ACA premium credits, state income tax, HSA, fixed assets, debts,
  SPIAs, QCDs, Roth 5-year maturation, pre-TCJA sunset scenarios, Monte Carlo
  rate models, and the time-preference discount.
- Investment returns are deterministic per account; the taxable return includes
  a qualified-dividend yield taxed each year.
- The 10% early-withdrawal penalty applies through age 59 (exact for mid-year
  births).

## Architecture impact

- New crate only; no existing crate, migration, Tauri command or Axum route was
  changed, and nothing depends on it yet.
- New native dependency (HiGHS via `good_lp`): any workspace-wide build
  (`cargo clippy --workspace`, `cargo test --workspace`) now needs CMake and
  libclang. CI would need both. The desktop and web apps don't link the crate, so
  `pnpm tauri dev` and `pnpm dev:web` are unaffected.
- `Cargo.lock` gained `good_lp`, `highs`, `highs-sys` and `bindgen` entries.
- Pure computation: no network, storage, or financial-data persistence.

## How to run

```powershell
# In the VS 2022 developer shell:
$env:LIBCLANG_PATH = "$env:LOCALAPPDATA\libclang"
$env:OPENSSL_SRC_PERL = 'C:\Strawberry\perl\bin\perl.exe'
cargo test -p wealthfolio-optimization-engine
cargo run -p wealthfolio-optimization-engine --example time_solve -- --years
cargo run -p wealthfolio-optimization-engine --example duality
cargo run -p wealthfolio-optimization-engine --example infeasible
```

## Suggested next steps

1. Two-person households (survivor benefit table, filing-status switch).
2. Wire into Wealthfolio: a core service that builds a `Scenario` from account
   balances, plus Tauri/Axum commands.
3. SS-taxability MILP mode and NIIT, if loop residuals matter for high earners.
4. Sweep the loop stabilizer over more scenarios before relying on its default.
