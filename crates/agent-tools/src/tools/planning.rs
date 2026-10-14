//! Goal and retirement planning tools: goal details and overviews, the
//! retirement projections (FIRE/Traditional styles) and the tax-optimized
//! plan. Every run can take `planOverrides`, a JSON merge patch applied to
//! the goal's saved plan settings for that run only, so "what if" questions
//! never change the saved plan.

use std::sync::Arc;

use serde::Deserialize;
use wealthfolio_core::goals::{validate_retirement_plan, TaxPlanInput};
use wealthfolio_core::planning::retirement::{
    normalize_retirement_plan_ages, optimize_tax_plan, tax_plan_choice_costs,
    tax_plan_conversion_regret, tax_plan_safe_spending, tax_plan_stress_tests, tax_plan_success,
    RetirementPlan, RetirementTimingMode, SuccessPathMode,
};
use wealthfolio_core::planning::{compute_save_up_overview, validate_save_up_input, SaveUpInput};
use wealthfolio_core::portfolio::fire::{
    project_retirement_with_mode, run_decision_sensitivity_matrix_with_mode,
    run_monte_carlo_with_mode_and_seed, run_scenario_analysis_with_mode,
    run_stress_tests_with_mode, DecisionSensitivityMap,
};

use super::support::{
    account_values, agent_tool, blocking, failed, invalid, object_schema, ok, parse, patched,
    platform, tax_plan_maps, today,
};
use crate::env::AgentEnvironment;
use crate::tool::AgentToolError;

const MAX_SIMULATIONS: u32 = 50_000;
const DEFAULT_SIMULATIONS: u32 = 10_000;

const PLAN_OVERRIDES: &str = "Optional JSON merge patch (RFC 7386) applied to the goal's saved plan settings (the `plan.settings` object from get_goal_details) for this run only; nothing is saved. Example: {\"personal\":{\"targetRetirementAge\":60}}.";

fn goal_id_schema() -> serde_json::Value {
    serde_json::json!({ "type": "string", "description": "Goal id (from get_goals)." })
}

fn overrides_schema() -> serde_json::Value {
    serde_json::json!({ "type": "object", "description": PLAN_OVERRIDES })
}

fn retirement_age_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "integer",
        "description": "Retirement age to test instead of the plan's target age."
    })
}

fn path_mode_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "string",
        "enum": ["historical", "bootstrap", "lognormal"],
        "description": "historical: every historical window since 1928; bootstrap: resampled historical years; lognormal: years drawn from a correlated lognormal fitted to the history. The plan's taxOptimizer.simulationCount sets how many bootstrap or lognormal paths (default 100; regretSimulationCount, default 30, for conversion regret); taxOptimizer.randomLifespans ({sex: Male|Female, table: an Owl mortality table key such as SSA2025}) gives each simulated run a random age at death. Default historical."
    })
}

fn parse_json_text(text: &str) -> serde_json::Value {
    serde_json::from_str(text).unwrap_or_else(|_| serde_json::Value::String(text.to_string()))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoalArgs {
    goal_id: String,
}

agent_tool! {
    /// A goal with its funding rules and saved plan settings.
    GetGoalDetails {
        name: "get_goal_details",
        description: "Get one goal in full: the goal, its funding rules (which accounts fund it, what share, and each account's tax bucket) and its saved plan (planning style, settings and summary). The plan settings object is the shape that planOverrides patch and update_goal_plan changes.",
        scopes: [FinancialPlanningRead],
        access: Read,
        schema: object_schema(serde_json::json!({ "goalId": goal_id_schema() }), &["goalId"]),
        call: |env, args| {
            let args: GoalArgs = parse(args)?;
            let goals = env.goal_service();
            let goal = goals.get_goal(&args.goal_id).map_err(failed)?;
            let funding = goals.get_goal_funding(&args.goal_id).map_err(failed)?;
            let plan = goals.get_goal_plan(&args.goal_id).map_err(failed)?.map(|plan| {
                serde_json::json!({
                    "planKind": plan.plan_kind,
                    "plannerMode": plan.planner_mode,
                    "settings": parse_json_text(&plan.settings_json),
                    "summary": parse_json_text(&plan.summary_json),
                    "version": plan.version,
                    "updatedAt": plan.updated_at,
                })
            });
            ok(serde_json::json!({ "goal": goal, "fundingRules": funding, "plan": plan }))
        }
    }
}

agent_tool! {
    /// The goal's overview from current account values.
    GetGoalOverview {
        name: "get_goal_overview",
        description: "Get a goal's overview from current account values, as the goal page shows it: for a retirement goal the projected retirement (required capital, projected value, shortfall, funding status, year-by-year trajectory); for other goals the save-up progress and projection.",
        scopes: [FinancialPlanningRead],
        access: Read,
        schema: object_schema(serde_json::json!({ "goalId": goal_id_schema() }), &["goalId"]),
        call: |env, args| {
            let args: GoalArgs = parse(args)?;
            let platform = platform(&env)?;
            let goal = env.goal_service().get_goal(&args.goal_id).map_err(failed)?;
            let values = account_values(&env, &platform).await?;
            if goal.goal_type == "retirement" {
                ok(env
                    .goal_service()
                    .compute_retirement_overview(&args.goal_id, &values)
                    .await
                    .map_err(failed)?)
            } else {
                ok(env
                    .goal_service()
                    .compute_save_up_overview(&args.goal_id, &values)
                    .await
                    .map_err(failed)?)
            }
        }
    }
}

agent_tool! {
    /// What the tax-optimized plan starts from.
    GetTaxPlanAccounts {
        name: "get_tax_plan_accounts",
        description: "Get the starting point of a retirement goal's tax-optimized plan: balances by tax treatment (taxable with cost basis, tax-deferred, Roth, HSA) from the goal's funding rules, and each bucket's asset mix from its holdings.",
        scopes: [FinancialPlanningRead],
        access: Read,
        schema: object_schema(serde_json::json!({ "goalId": goal_id_schema() }), &["goalId"]),
        call: |env, args| {
            let args: GoalArgs = parse(args)?;
            let platform = platform(&env)?;
            let (values, cost_basis, assets) = tax_plan_maps(&env, &platform).await?;
            ok(env
                .goal_service()
                .retirement_tax_plan_accounts(&args.goal_id, &values, &cost_basis, &assets)
                .await
                .map_err(failed)?)
        }
    }
}

agent_tool! {
    /// Save-up projection for any inputs.
    PreviewSaveUpPlan {
        name: "preview_save_up_plan",
        description: "Project a savings goal from given inputs without saving anything: progress, months to target, projected value at the target date and the monthly contribution needed.",
        scopes: [FinancialPlanningRead],
        access: Read,
        schema: object_schema(
            serde_json::json!({
                "currentValue": { "type": "number" },
                "targetAmount": { "type": "number" },
                "targetDate": { "type": ["string", "null"], "description": "YYYY-MM-DD, or null for open-ended." },
                "monthlyContribution": { "type": "number" },
                "expectedAnnualReturn": { "type": "number", "description": "Decimal, e.g. 0.05 for 5%." }
            }),
            &["currentValue", "targetAmount", "monthlyContribution", "expectedAnnualReturn"],
        ),
        call: |env, args| {
            let input: SaveUpInput = parse(args)?;
            let as_of = today(&platform(&env)?);
            validate_save_up_input(&input, as_of).map_err(|err| invalid(err.to_string()))?;
            ok(compute_save_up_overview(&input, as_of))
        }
    }
}

// ─── Retirement projections ──────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RetirementArgs {
    goal_id: String,
    #[serde(default)]
    plan_overrides: Option<serde_json::Value>,
    #[serde(default)]
    planner_mode: Option<RetirementTimingMode>,
    #[serde(default)]
    current_portfolio: Option<f64>,
}

struct RetirementRun {
    plan: RetirementPlan,
    current_portfolio: f64,
    planner_mode: RetirementTimingMode,
    as_of: chrono::NaiveDate,
}

fn checked_plan(
    plan: &RetirementPlan,
    overrides: Option<&serde_json::Value>,
    as_of: chrono::NaiveDate,
) -> Result<RetirementPlan, AgentToolError> {
    let Some(overrides) = overrides else {
        return Ok(plan.clone());
    };
    let mut plan = patched(plan, Some(overrides))?;
    normalize_retirement_plan_ages(&mut plan, as_of);
    validate_retirement_plan(&plan).map_err(|err| invalid(err.to_string()))?;
    Ok(plan)
}

async fn retirement_run(
    env: &Arc<dyn AgentEnvironment>,
    args: &RetirementArgs,
) -> Result<RetirementRun, AgentToolError> {
    let platform = platform(env)?;
    let values = account_values(env, &platform).await?;
    let prepared = env
        .goal_service()
        .prepare_retirement_simulation_input(&args.goal_id, &values)
        .await
        .map_err(failed)?;
    let as_of = today(&platform);
    Ok(RetirementRun {
        plan: checked_plan(&prepared.plan, args.plan_overrides.as_ref(), as_of)?,
        current_portfolio: args.current_portfolio.unwrap_or(prepared.current_portfolio),
        planner_mode: args.planner_mode.unwrap_or(prepared.planner_mode),
        as_of,
    })
}

fn retirement_schema(extra: serde_json::Value) -> serde_json::Value {
    let mut properties = serde_json::json!({
        "goalId": goal_id_schema(),
        "planOverrides": overrides_schema(),
        "plannerMode": {
            "type": "string",
            "enum": ["fire", "traditional"],
            "description": "fire: retire as soon as the portfolio is enough; traditional: retire at the target age. Default: the goal's style."
        },
        "currentPortfolio": {
            "type": "number",
            "description": "Starting portfolio value to use instead of the goal's funded accounts."
        }
    });
    if let (Some(properties), Some(extra)) = (properties.as_object_mut(), extra.as_object()) {
        properties.extend(extra.clone());
    }
    object_schema(properties, &["goalId"])
}

agent_tool! {
    /// Deterministic retirement projection.
    RunRetirementProjection {
        name: "run_retirement_projection",
        description: "Project a retirement goal year by year with deterministic returns (FIRE/Traditional planning styles): retirement age reached, portfolio at retirement, required capital, and the yearly balance, contributions, withdrawals and income. Use planOverrides to test changes without saving them.",
        scopes: [FinancialPlanningRead, PlanningRun],
        access: Read,
        schema: retirement_schema(serde_json::json!({})),
        call: |env, args| {
            let args: RetirementArgs = parse(args)?;
            let run = retirement_run(&env, &args).await?;
            ok(project_retirement_with_mode(
                &run.plan,
                run.current_portfolio,
                run.planner_mode,
                run.as_of,
            ))
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MonteCarloArgs {
    #[serde(flatten)]
    retirement: RetirementArgs,
    #[serde(default)]
    simulations: Option<u32>,
    #[serde(default)]
    seed: Option<u64>,
}

agent_tool! {
    /// Monte Carlo success rate of the retirement plan.
    RunRetirementMonteCarlo {
        name: "run_retirement_monte_carlo",
        description: "Run a Monte Carlo simulation of a retirement goal: success probability, percentile portfolio paths and failure ages.",
        scopes: [FinancialPlanningRead, PlanningRun],
        access: Read,
        schema: retirement_schema(serde_json::json!({
            "simulations": { "type": "integer", "description": "Number of simulated paths (default 10000, max 50000)." },
            "seed": { "type": "integer", "description": "Random seed for reproducible results." }
        })),
        call: |env, args| {
            let args: MonteCarloArgs = parse(args)?;
            let run = retirement_run(&env, &args.retirement).await?;
            let simulations = args
                .simulations
                .unwrap_or(DEFAULT_SIMULATIONS)
                .clamp(1, MAX_SIMULATIONS);
            ok(blocking(move || {
                run_monte_carlo_with_mode_and_seed(
                    &run.plan,
                    run.current_portfolio,
                    simulations,
                    run.planner_mode,
                    args.seed,
                )
            })
            .await?)
        }
    }
}

agent_tool! {
    /// Retirement stress tests.
    RunRetirementStressTests {
        name: "run_retirement_stress_tests",
        description: "Stress-test a retirement goal: lower returns, an inflation shock, higher spending, retiring earlier, saving less and an early market crash, each with its effect on the outcome.",
        scopes: [FinancialPlanningRead, PlanningRun],
        access: Read,
        schema: retirement_schema(serde_json::json!({})),
        call: |env, args| {
            let args: RetirementArgs = parse(args)?;
            let run = retirement_run(&env, &args).await?;
            ok(blocking(move || {
                run_stress_tests_with_mode(&run.plan, run.current_portfolio, run.planner_mode, run.as_of)
            })
            .await?)
        }
    }
}

agent_tool! {
    /// Pessimistic/base/optimistic scenarios.
    RunRetirementScenarios {
        name: "run_retirement_scenarios",
        description: "Run a retirement goal under pessimistic, base and optimistic return scenarios.",
        scopes: [FinancialPlanningRead, PlanningRun],
        access: Read,
        schema: retirement_schema(serde_json::json!({})),
        call: |env, args| {
            let args: RetirementArgs = parse(args)?;
            let run = retirement_run(&env, &args).await?;
            ok(blocking(move || {
                run_scenario_analysis_with_mode(
                    &run.plan,
                    run.current_portfolio,
                    run.planner_mode,
                    run.as_of,
                )
            })
            .await?)
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SensitivityArgs {
    #[serde(flatten)]
    retirement: RetirementArgs,
    map: DecisionSensitivityMap,
}

agent_tool! {
    /// Two-way sensitivity grid.
    RunRetirementSensitivityMap {
        name: "run_retirement_sensitivity_map",
        description: "Compute a two-way sensitivity grid for a retirement goal: contribution vs return, or retirement age vs spending, with the outcome in each cell.",
        scopes: [FinancialPlanningRead, PlanningRun],
        access: Read,
        schema: {
            let mut schema = retirement_schema(serde_json::json!({
                "map": { "type": "string", "enum": ["contribution-return", "retirement-age-spending"] }
            }));
            schema["required"] = serde_json::json!(["goalId", "map"]);
            schema
        },
        call: |env, args| {
            let args: SensitivityArgs = parse(args)?;
            let run = retirement_run(&env, &args.retirement).await?;
            let map = args.map;
            ok(blocking(move || {
                run_decision_sensitivity_matrix_with_mode(
                    &run.plan,
                    run.current_portfolio,
                    run.planner_mode,
                    map,
                    run.as_of,
                )
            })
            .await?)
        }
    }
}

// ─── Tax-optimized plan ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaxPlanArgs {
    goal_id: String,
    #[serde(default)]
    plan_overrides: Option<serde_json::Value>,
    #[serde(default)]
    retirement_age: Option<u32>,
    #[serde(default)]
    mode: Option<SuccessPathMode>,
    #[serde(default)]
    target_rate: Option<f64>,
}

impl TaxPlanArgs {
    fn mode(&self) -> SuccessPathMode {
        self.mode.unwrap_or(SuccessPathMode::Historical)
    }
}

async fn tax_plan_input(
    env: &Arc<dyn AgentEnvironment>,
    args: &TaxPlanArgs,
) -> Result<TaxPlanInput, AgentToolError> {
    let platform = platform(env)?;
    let (values, cost_basis, assets) = tax_plan_maps(env, &platform).await?;
    let mut input = env
        .goal_service()
        .retirement_tax_plan_input(&args.goal_id, &values, &cost_basis, &assets)
        .await
        .map_err(failed)?;
    input.plan = checked_plan(&input.plan, args.plan_overrides.as_ref(), input.as_of)?;
    Ok(input)
}

fn tax_plan_schema(extra: serde_json::Value, required: &[&str]) -> serde_json::Value {
    let mut properties = serde_json::json!({
        "goalId": goal_id_schema(),
        "planOverrides": overrides_schema(),
    });
    if let (Some(properties), Some(extra)) = (properties.as_object_mut(), extra.as_object()) {
        properties.extend(extra.clone());
    }
    let mut keys = vec!["goalId"];
    keys.extend_from_slice(required);
    object_schema(properties, &keys)
}

agent_tool! {
    /// Solve the tax-optimized plan.
    OptimizeTaxPlan {
        name: "optimize_tax_plan",
        description: "Solve a retirement goal's tax-optimized plan (linear optimization, as the Plan tab's Optimize does): yearly withdrawals by account, Roth conversions, taxes, spending and balances, the playbook and the baseline comparison. The tax plan settings live under plan settings `taxOptimizer`. Takes up to a minute; nothing is saved.",
        scopes: [FinancialPlanningRead, PlanningRun],
        access: Read,
        schema: tax_plan_schema(serde_json::json!({}), &[]),
        call: |env, args| {
            let args: TaxPlanArgs = parse(args)?;
            let input = tax_plan_input(&env, &args).await?;
            ok(blocking(move || optimize_tax_plan(&input.plan, &input.accounts, input.as_of))
                .await?
                .map_err(failed)?)
        }
    }
}

agent_tool! {
    /// Success rate of the tax-optimized plan.
    RunTaxPlanSuccess {
        name: "run_tax_plan_success",
        description: "Chance a retirement goal's tax-optimized plan funds its expenses across historical or resampled return paths (re-solves the plan per path; can take minutes).",
        scopes: [FinancialPlanningRead, PlanningRun],
        access: Read,
        schema: tax_plan_schema(
            serde_json::json!({ "mode": path_mode_schema(), "retirementAge": retirement_age_schema() }),
            &[],
        ),
        call: |env, args| {
            let args: TaxPlanArgs = parse(args)?;
            let mode = args.mode();
            let input = tax_plan_input(&env, &args).await?;
            ok(blocking(move || {
                tax_plan_success(&input.plan, &input.accounts, input.as_of, mode, args.retirement_age)
            })
            .await?
            .map_err(failed)?)
        }
    }
}

agent_tool! {
    /// Most spending at a target success rate.
    RunTaxPlanSafeSpending {
        name: "run_tax_plan_safe_spending",
        description: "The most a retirement goal's tax-optimized plan can spend while succeeding on at least targetRate of the return paths, with the spending-vs-success frontier (re-solves the plan per path; can take minutes).",
        scopes: [FinancialPlanningRead, PlanningRun],
        access: Read,
        schema: tax_plan_schema(
            serde_json::json!({
                "mode": path_mode_schema(),
                "retirementAge": retirement_age_schema(),
                "targetRate": { "type": "number", "description": "Success rate to reach, 0-1 (e.g. 0.9)." }
            }),
            &["targetRate"],
        ),
        call: |env, args| {
            let args: TaxPlanArgs = parse(args)?;
            let target_rate = args
                .target_rate
                .filter(|rate| (0.0..=1.0).contains(rate))
                .ok_or_else(|| invalid("targetRate must be between 0 and 1"))?;
            let mode = args.mode();
            let input = tax_plan_input(&env, &args).await?;
            ok(blocking(move || {
                tax_plan_safe_spending(
                    &input.plan,
                    &input.accounts,
                    input.as_of,
                    mode,
                    args.retirement_age,
                    target_rate,
                )
            })
            .await?
            .map_err(failed)?)
        }
    }
}

agent_tool! {
    /// What each plan choice costs.
    RunTaxPlanChoiceCosts {
        name: "run_tax_plan_choice_costs",
        description: "What each choice of a retirement goal's tax-optimized plan (spending floor, bequest, conversion limits, retirement age...) costs its goal, by re-solving the plan with each choice relaxed (can take a minute or more).",
        scopes: [FinancialPlanningRead, PlanningRun],
        access: Read,
        schema: tax_plan_schema(serde_json::json!({ "retirementAge": retirement_age_schema() }), &[]),
        call: |env, args| {
            let args: TaxPlanArgs = parse(args)?;
            let input = tax_plan_input(&env, &args).await?;
            ok(blocking(move || {
                tax_plan_choice_costs(&input.plan, &input.accounts, input.as_of, args.retirement_age)
            })
            .await?
            .map_err(failed)?)
        }
    }
}

agent_tool! {
    /// The plan under shocks it does not control.
    RunTaxPlanStressTests {
        name: "run_tax_plan_stress_tests",
        description: "How a retirement goal's tax-optimized plan holds up under shocks it does not control: returns 2 points lower, inflation 1.5 points higher, must-have expenses 10% higher, Social Security paying 77%, living 5 years longer, and retiring into the markets of 1929, 1966 or 2000. Each is re-optimized; effects are changes in the goal in today's money (null: no solution). Can take a minute or more.",
        scopes: [FinancialPlanningRead, PlanningRun],
        access: Read,
        schema: tax_plan_schema(serde_json::json!({ "retirementAge": retirement_age_schema() }), &[]),
        call: |env, args| {
            let args: TaxPlanArgs = parse(args)?;
            let input = tax_plan_input(&env, &args).await?;
            ok(blocking(move || {
                tax_plan_stress_tests(&input.plan, &input.accounts, input.as_of, args.retirement_age)
            })
            .await?
            .map_err(failed)?)
        }
    }
}

agent_tool! {
    /// Regret of this year's Roth conversion.
    RunTaxPlanConversionRegret {
        name: "run_tax_plan_conversion_regret",
        description: "How this year's recommended Roth conversion fares across market paths compared with converting more or less (re-solves the plan per path and amount; can take minutes).",
        scopes: [FinancialPlanningRead, PlanningRun],
        access: Read,
        schema: tax_plan_schema(
            serde_json::json!({ "mode": path_mode_schema(), "retirementAge": retirement_age_schema() }),
            &[],
        ),
        call: |env, args| {
            let args: TaxPlanArgs = parse(args)?;
            let mode = args.mode();
            let input = tax_plan_input(&env, &args).await?;
            ok(blocking(move || {
                tax_plan_conversion_regret(
                    &input.plan,
                    &input.accounts,
                    input.as_of,
                    mode,
                    args.retirement_age,
                )
            })
            .await?
            .map_err(failed)?)
        }
    }
}
