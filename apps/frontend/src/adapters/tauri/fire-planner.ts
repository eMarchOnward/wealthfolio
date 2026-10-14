import { invoke } from "./core";
import type {
  DecisionSensitivityMap,
  DecisionSensitivityMatrix,
  FireProjection,
  MonteCarloResult,
  RetirementPlan,
  ScenarioResult,
  SorrScenario,
  StressTestResult,
  TaxOptimizedPlan,
  TaxOptimizerAccounts,
  TaxPlanChoiceCosts,
  TaxPlanConversionRegret,
  TaxPlanSafeSpending,
  TaxPlanStressTests,
  TaxPlanSuccess,
  TaxSuccessMode,
} from "@/features/goals/retirement-planner/types";
import type { PlannerMode } from "@/lib/types";

export const calculateRetirementProjection = async (
  plan: RetirementPlan,
  currentPortfolio: number,
  plannerMode?: PlannerMode,
  goalId?: string,
): Promise<FireProjection> => {
  return invoke<FireProjection>("calculate_retirement_projection", {
    plan,
    currentPortfolio,
    plannerMode,
    goalId,
  });
};

export const runRetirementMonteCarlo = async (
  plan: RetirementPlan,
  currentPortfolio: number,
  nSims = 100_000,
  plannerMode?: PlannerMode,
  goalId?: string,
  seed?: number,
): Promise<MonteCarloResult> => {
  return invoke<MonteCarloResult>("run_retirement_monte_carlo", {
    plan,
    currentPortfolio,
    nSims,
    plannerMode,
    goalId,
    seed,
  });
};

export const runRetirementStressTests = async (
  plan: RetirementPlan,
  currentPortfolio: number,
  plannerMode?: PlannerMode,
  goalId?: string,
): Promise<StressTestResult[]> => {
  return invoke<StressTestResult[]>("run_retirement_stress_tests", {
    plan,
    currentPortfolio,
    plannerMode,
    goalId,
  });
};

export const runRetirementScenarioAnalysis = async (
  plan: RetirementPlan,
  currentPortfolio: number,
  plannerMode?: PlannerMode,
  goalId?: string,
): Promise<ScenarioResult[]> => {
  return invoke<ScenarioResult[]>("run_retirement_scenario_analysis", {
    plan,
    currentPortfolio,
    plannerMode,
    goalId,
  });
};

export const runRetirementSorr = async (
  plan: RetirementPlan,
  portfolioAtFire: number,
  retirementStartAge: number,
  goalId?: string,
): Promise<SorrScenario[]> => {
  return invoke<SorrScenario[]>("run_retirement_sorr", {
    plan,
    portfolioAtFire,
    retirementStartAge,
    goalId,
  });
};

export const runRetirementDecisionSensitivityMap = async (
  plan: RetirementPlan,
  currentPortfolio: number,
  map: DecisionSensitivityMap,
  plannerMode?: PlannerMode,
  goalId?: string,
): Promise<DecisionSensitivityMatrix> => {
  return invoke<DecisionSensitivityMatrix>("run_retirement_decision_sensitivity_map", {
    plan,
    currentPortfolio,
    map,
    plannerMode,
    goalId,
  });
};

/** Starting balances and asset mixes the tax-optimized plan of a retirement goal begins from. */
export const getRetirementTaxPlanAccounts = async (
  goalId: string,
): Promise<TaxOptimizerAccounts> => {
  return invoke<TaxOptimizerAccounts>("get_retirement_tax_plan_accounts", { goalId });
};

/** Solves the tax-optimized (MILP) plan of a retirement goal; can take a minute. */
export const optimizeRetirementTaxPlan = async (goalId: string): Promise<TaxOptimizedPlan> => {
  return invoke<TaxOptimizedPlan>("optimize_retirement_tax_plan", { goalId });
};

/** Chance the tax-optimized plan's expenses are funded across return paths; can take minutes. */
export const runRetirementTaxSuccess = async (
  goalId: string,
  mode: TaxSuccessMode,
  retirementAge?: number,
): Promise<TaxPlanSuccess> => {
  return invoke<TaxPlanSuccess>("run_retirement_tax_success", { goalId, mode, retirementAge });
};

/** The most the tax-optimized plan can spend at a target success rate; can take minutes. */
export const runRetirementTaxSafeSpending = async (
  goalId: string,
  mode: TaxSuccessMode,
  targetRate: number,
  retirementAge?: number,
): Promise<TaxPlanSafeSpending> => {
  return invoke<TaxPlanSafeSpending>("run_retirement_tax_safe_spending", {
    goalId,
    mode,
    targetRate,
    retirementAge,
  });
};

/** What each choice of the tax-optimized plan costs its goal; can take a minute or more. */
export const runRetirementTaxChoiceCosts = async (
  goalId: string,
  retirementAge?: number,
): Promise<TaxPlanChoiceCosts> => {
  return invoke<TaxPlanChoiceCosts>("run_retirement_tax_choice_costs", { goalId, retirementAge });
};

/** The tax-optimized plan re-solved under shocks it does not control; can take a minute or more. */
export const runRetirementTaxStressTests = async (
  goalId: string,
  retirementAge?: number,
): Promise<TaxPlanStressTests> => {
  return invoke<TaxPlanStressTests>("run_retirement_tax_stress_tests", { goalId, retirementAge });
};

/** How this year's recommended Roth conversion fares across market paths; can take minutes. */
export const runRetirementTaxConversionRegret = async (
  goalId: string,
  mode: TaxSuccessMode,
  retirementAge?: number,
): Promise<TaxPlanConversionRegret> => {
  return invoke<TaxPlanConversionRegret>("run_retirement_tax_conversion_regret", {
    goalId,
    mode,
    retirementAge,
  });
};
