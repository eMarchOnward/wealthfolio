import { describe, expect, it } from "vitest";
import { DEFAULT_RETIREMENT_PLAN, parseSettingsJson } from "./plan-adapter";
import {
  comparableItems,
  CURRENT_PLAN_ID,
  settingLabel,
  settingsDifferences,
  settingValue,
} from "./tax-plan-compare";
import type { RetirementPlan, SavedTaxPlanRun, TaxOptimizedPlan } from "../types";

const plan: RetirementPlan = parseSettingsJson(
  JSON.stringify({ ...DEFAULT_RETIREMENT_PLAN, currency: "USD" }),
);
const run: SavedTaxPlanRun = {
  inputsKey: "k",
  computedAt: "2026-10-10T00:00:00.000Z",
  result: { status: "optimal" } as TaxOptimizedPlan,
};

describe("tax plan comparison", () => {
  it("offers the current plan and cases saved with a result", () => {
    const store = {
      cases: [
        { id: "a", name: "A", savedAt: "", plan, run },
        { id: "b", name: "B", savedAt: "", plan },
      ],
    };
    expect(comparableItems(store, plan, run, "Current").map((i) => i.id)).toEqual([
      CURRENT_PLAN_ID,
      "a",
    ]);
    expect(comparableItems(store, plan, null, "Current").map((i) => i.id)).toEqual(["a"]);
  });

  it("lists only the settings that differ, ignoring tax rates and the current age", () => {
    const other: RetirementPlan = {
      ...plan,
      personal: { ...plan.personal, targetRetirementAge: 60, currentAge: 99 },
      tax: { taxableWithdrawalRate: 0.5, taxDeferredWithdrawalRate: 0, taxFreeWithdrawalRate: 0 },
      taxOptimizer: { bequestFloor: 50_000 },
    };
    const paths = settingsDifferences([plan, other]).map((d) => d.path);
    expect(paths).toContain("personal.targetRetirementAge");
    expect(paths).toContain("taxOptimizer.bequestFloor");
    expect(paths.some((p) => p.startsWith("tax.") || p === "personal.currentAge")).toBe(false);
    expect(settingsDifferences([plan, plan])).toEqual([]);
  });

  it("labels and shows settings readably", () => {
    expect(settingLabel("taxOptimizer.markets.startYear")).toBe("Tax Plan › Markets › Start year");
    expect(settingValue(undefined)).toBe("—");
    expect(settingValue([1, 2])).toBe("2 items");
    expect(settingValue(0.123456)).toBe("0.1235");
  });
});
