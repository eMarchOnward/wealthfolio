import type { RetirementPlan, TaxOptimizerAccounts, TaxPlanCase, TaxPlanStore } from "../types";

/** Settings JSON key holding the `TaxPlanStore`. */
export const TAX_PLAN_STORE_KEY = "taxPlanStore";

/** Balances may drift this much (as a share) before a saved result is flagged. */
export const BALANCE_CHANGE_TOLERANCE = 0.02;

export const EMPTY_TAX_PLAN_STORE: TaxPlanStore = { cases: [] };

/** The tabs of the tax-optimized style. */
export type TaxPlanTab = "plan" | "overview" | "risk" | "compare";
export const TAX_PLAN_TABS: TaxPlanTab[] = ["plan", "overview", "risk", "compare"];

/** Reads the stored Tax Plan results and cases from a goal plan's settings JSON. */
export function parseTaxPlanStore(settingsJson: string | undefined): TaxPlanStore {
  if (!settingsJson) return EMPTY_TAX_PLAN_STORE;
  try {
    const raw: unknown = (JSON.parse(settingsJson) as Record<string, unknown>)?.[
      TAX_PLAN_STORE_KEY
    ];
    if (!raw || typeof raw !== "object") return EMPTY_TAX_PLAN_STORE;
    const store = raw as Partial<TaxPlanStore>;
    return {
      lastRun: store.lastRun,
      cases: Array.isArray(store.cases) ? store.cases : [],
    };
  } catch {
    return EMPTY_TAX_PLAN_STORE;
  }
}

/** Settings JSON for `plan` with `store` kept beside it. */
export function settingsJsonWithStore(plan: RetirementPlan, store: TaxPlanStore): string {
  return JSON.stringify({ ...plan, [TAX_PLAN_STORE_KEY]: store });
}

/** JSON with object keys sorted, so equal plans give equal strings whatever their key order. */
function stableStringify(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  if (value && typeof value === "object") {
    const entries = Object.entries(value as Record<string, unknown>)
      .filter(([, v]) => v !== undefined)
      .sort(([a], [b]) => (a < b ? -1 : a > b ? 1 : 0));
    return `{${entries.map(([k, v]) => `${JSON.stringify(k)}:${stableStringify(v)}`).join(",")}}`;
  }
  return JSON.stringify(value) ?? "null";
}

/**
 * Identifies the inputs a result was solved for. `currentAge` is left out: the
 * backend refreshes it on every save, and it only moves with the birth date.
 * So are the withdrawal tax rates, which the tax-optimized plan does not use
 * (it works taxes out itself), and the success target and simulation count,
 * which only steer the risk checks.
 */
export function planInputsKey(plan: RetirementPlan): string {
  const { personal, tax: _tax, taxOptimizer, ...rest } = plan;
  const { currentAge: _currentAge, ...personalInputs } = personal;
  const {
    successTarget: _target,
    simulationCount: _simulations,
    regretSimulationCount: _regretSimulations,
    randomLifespans: _lifespans,
    ...optimizerInputs
  } = taxOptimizer ?? {};
  return stableStringify({
    ...rest,
    personal: personalInputs,
    taxOptimizer: taxOptimizer ? optimizerInputs : undefined,
  });
}

/** Today's funding balances as the overview reports them (HSA is in `taxFree`). */
export function taxBucketTotal(buckets: {
  taxable: number;
  taxDeferred: number;
  taxFree: number;
}): number {
  return buckets.taxable + buckets.taxDeferred + buckets.taxFree;
}

export function accountsTotal(accounts: TaxOptimizerAccounts): number {
  return accounts.taxable + accounts.taxDeferred + accounts.roth + accounts.hsa;
}

/**
 * How much the funding accounts moved since a result was solved, as a share of
 * the balances it used, or `null` while it is within tolerance or unknown.
 */
export function balanceChange(
  solvedWith: TaxOptimizerAccounts,
  currentTotal: number | undefined,
): number | null {
  const solvedTotal = accountsTotal(solvedWith);
  if (currentTotal == null || solvedTotal <= 0) return null;
  const change = (currentTotal - solvedTotal) / solvedTotal;
  return Math.abs(change) > BALANCE_CHANGE_TOLERANCE ? change : null;
}

/** The case whose inputs match `plan`, if any. */
export function caseMatchingPlan(
  cases: TaxPlanCase[],
  plan: RetirementPlan,
): TaxPlanCase | undefined {
  const key = planInputsKey(plan);
  return cases.find((c) => planInputsKey(c.plan) === key);
}
