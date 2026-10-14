import { describe, expect, it } from "vitest";
import {
  DEFAULT_RETIREMENT_PLAN,
  ageFromBirthYearMonth,
  createDefaultRetirementPlan,
  HOME_COSTS_EXPENSE_ID,
  normalizeDashboardRetirementPlan,
  normalizeRetirementPlan,
  parseSettingsJson,
  RENT_EXPENSE_ID,
} from "./plan-adapter";

describe("retirement plan adapter", () => {
  it("ignores a birth year that is still being typed", () => {
    const asOf = new Date(2026, 9, 12);
    expect(ageFromBirthYearMonth("1975-03", asOf)).toBe(51);
    expect(ageFromBirthYearMonth("1975-11", asOf)).toBe(50);
    // A month input reports "0001", "0019" and "0197" on the way to "1975".
    expect(ageFromBirthYearMonth("0019-03", asOf)).toBeUndefined();
    expect(ageFromBirthYearMonth("0197-03", asOf)).toBeUndefined();
    expect(ageFromBirthYearMonth("2030-03", asOf)).toBeUndefined();
  });

  it("default plans do not include legacy withdrawal-rule fields", () => {
    const plan = parseSettingsJson("{}");

    expect(plan).not.toHaveProperty("withdrawal");
  });

  it("parses old withdrawal-rule JSON and strips it from the normalized plan", () => {
    const plan = parseSettingsJson(
      JSON.stringify({
        withdrawal: {
          safeWithdrawalRate: 0.04,
          strategy: "guardrails",
          guardrails: {
            ceilingRate: 0.06,
          },
        },
      }),
    );

    expect(plan).not.toHaveProperty("withdrawal");
  });

  it("saving a plan strips any legacy withdrawal-rule fields", () => {
    const plan = parseSettingsJson(
      JSON.stringify({
        withdrawal: {
          safeWithdrawalRate: 0.041,
          strategy: "constant-percentage",
        },
      }),
    );

    const normalized = normalizeDashboardRetirementPlan(plan);

    expect(normalized).not.toHaveProperty("withdrawal");
  });
});

describe("createDefaultRetirementPlan", () => {
  it("sets the currency to the given base currency without altering amounts", () => {
    const plan = createDefaultRetirementPlan("IDR");

    expect(plan.currency).toBe("IDR");
    expect(plan.investment.monthlyContribution).toBe(
      DEFAULT_RETIREMENT_PLAN.investment.monthlyContribution,
    );
    expect(plan.expenses.items).toEqual(DEFAULT_RETIREMENT_PLAN.expenses.items);
  });
});

describe("retirement calendar date", () => {
  it("uses the supplied month for age and newly inferred birth dates", () => {
    const json = JSON.stringify({ personal: { birthYearMonth: "1990-01", currentAge: 35 } });
    expect(parseSettingsJson(json, new Date(2025, 11, 31)).personal.currentAge).toBe(35);
    expect(parseSettingsJson(json, new Date(2026, 0, 1)).personal.currentAge).toBe(36);
    expect(createDefaultRetirementPlan("USD", new Date(2026, 0, 1)).personal.birthYearMonth).toBe(
      "1996-01",
    );
    expect(createDefaultRetirementPlan("USD", new Date(2025, 11, 31)).personal.birthYearMonth).toBe(
      "1995-12",
    );
  });

  it("moves rent from the tax plan settings into a must-have item, once", () => {
    const plan = {
      ...DEFAULT_RETIREMENT_PLAN,
      taxOptimizer: { housing: "rent" as const, monthlyRent: 2_000, homeCosts: 9_000 },
    };
    const moved = normalizeRetirementPlan(plan);
    const rent = moved.expenses.items.find((item) => item.id === RENT_EXPENSE_ID);
    expect(rent).toMatchObject({ monthlyAmount: 2_000, essential: true, endsAtHomePurchase: true });
    expect(moved.taxOptimizer).toMatchObject({ monthlyRent: 0, homeCosts: 0 });
    expect(normalizeRetirementPlan(moved).expenses.items).toHaveLength(moved.expenses.items.length);
  });

  it("moves an owned home's costs into a yearly item that ends only when it is sold", () => {
    for (const sell of [false, true]) {
      const plan = {
        ...DEFAULT_RETIREMENT_PLAN,
        taxOptimizer: { housing: "own" as const, homeCosts: 9_000, sellCurrentHome: sell },
      };
      const costs = normalizeRetirementPlan(plan).expenses.items.find(
        (item) => item.id === HOME_COSTS_EXPENSE_ID,
      );
      expect(costs?.monthlyAmount).toBeCloseTo(750);
      expect(costs?.yearly).toBe(true);
      expect(costs?.endsAtHomePurchase ?? false).toBe(sell);
    }
  });
});
