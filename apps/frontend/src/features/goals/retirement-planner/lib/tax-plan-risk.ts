import type { RetirementPlan, TaxSuccessMode } from "../types";

/** Monte Carlo paths drawn unless the plan sets how many, and the range allowed (as in the backend). */
export const DEFAULT_SIMULATIONS = 100;
export const DEFAULT_REGRET_SIMULATIONS = 30;
export const MIN_SIMULATIONS = 10;
export const MAX_SIMULATIONS = 2_000;

/** The ways to draw market paths: history, then the two Monte Carlo methods. */
export const SUCCESS_MODES: readonly TaxSuccessMode[] = ["historical", "bootstrap", "lognormal"];

export function simulationCount(plan: RetirementPlan): number {
  return plan.taxOptimizer?.simulationCount ?? DEFAULT_SIMULATIONS;
}

export function regretSimulationCount(plan: RetirementPlan): number {
  return plan.taxOptimizer?.regretSimulationCount ?? DEFAULT_REGRET_SIMULATIONS;
}

/** Histogram bins of `values` (Owl's results histogram): about √n bins, at most 20. */
export function histogramBins(values: number[]): { from: number; to: number; count: number }[] {
  if (values.length === 0) return [];
  const min = Math.min(...values);
  const max = Math.max(...values);
  const count = Math.min(20, Math.max(1, Math.ceil(Math.sqrt(values.length))));
  const width = (max - min) / count;
  if (width <= 0) return [{ from: min, to: max, count: values.length }];
  const bins = Array.from({ length: count }, (_, k) => ({
    from: min + k * width,
    to: min + (k + 1) * width,
    count: 0,
  }));
  for (const value of values) {
    bins[Math.min(count - 1, Math.floor((value - min) / width))].count += 1;
  }
  return bins;
}
