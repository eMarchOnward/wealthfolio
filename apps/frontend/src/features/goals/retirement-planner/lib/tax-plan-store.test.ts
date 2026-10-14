import { describe, expect, it } from "vitest";
import { DEFAULT_RETIREMENT_PLAN, parseSettingsJson } from "./plan-adapter";
import {
  balanceChange,
  caseMatchingPlan,
  parseTaxPlanStore,
  planInputsKey,
  settingsJsonWithStore,
} from "./tax-plan-store";
import type { RetirementPlan, TaxOptimizedPlan, TaxPlanStore } from "../types";

// Parsed, as the page sees it (parsing fills in the birth month).
const plan: RetirementPlan = parseSettingsJson(
  JSON.stringify({ ...DEFAULT_RETIREMENT_PLAN, currency: "USD" }),
);
const accounts = { taxable: 100, taxableCostBasis: 80, taxDeferred: 300, roth: 50, hsa: 50 };
const store: TaxPlanStore = {
  lastRun: {
    inputsKey: planInputsKey(plan),
    computedAt: "2026-10-09T12:00:00.000Z",
    result: { startingAccounts: accounts } as TaxOptimizedPlan,
  },
  cases: [{ id: "a", name: "Retire at 64", savedAt: "2026-10-09T12:00:00.000Z", plan }],
};

describe("tax plan store", () => {
  it("round-trips beside the plan without leaking into it", () => {
    const json = settingsJsonWithStore(plan, store);
    expect(parseTaxPlanStore(json)).toEqual(store);
    const parsed = parseSettingsJson(json);
    expect(parsed).not.toHaveProperty("taxPlanStore");
    expect(planInputsKey(parsed)).toBe(planInputsKey(plan));
  });

  it("reads an empty store from plans saved before it existed", () => {
    expect(parseTaxPlanStore(JSON.stringify(plan))).toEqual({ cases: [] });
    expect(parseTaxPlanStore(undefined)).toEqual({ cases: [] });
    expect(parseTaxPlanStore("not json")).toEqual({ cases: [] });
  });

  it("keys inputs independent of key order and the refreshed current age", () => {
    const reordered = Object.fromEntries(
      Object.entries(plan).reverse(),
    ) as unknown as RetirementPlan;
    expect(planInputsKey(reordered)).toBe(planInputsKey(plan));
    const older = { ...plan, personal: { ...plan.personal, currentAge: 31 } };
    expect(planInputsKey(older)).toBe(planInputsKey(plan));
    const later = { ...plan, personal: { ...plan.personal, targetRetirementAge: 67 } };
    expect(planInputsKey(later)).not.toBe(planInputsKey(plan));
  });

  it("finds the case saved from the same inputs", () => {
    expect(caseMatchingPlan(store.cases, plan)?.id).toBe("a");
    const other = { ...plan, personal: { ...plan.personal, targetRetirementAge: 67 } };
    expect(caseMatchingPlan(store.cases, other)).toBeUndefined();
  });

  it("flags balances only beyond the tolerance", () => {
    expect(balanceChange(accounts, 505)).toBeNull();
    expect(balanceChange(accounts, 550)).toBeCloseTo(0.1);
    expect(balanceChange(accounts, 450)).toBeCloseTo(-0.1);
    expect(balanceChange(accounts, undefined)).toBeNull();
  });
});
