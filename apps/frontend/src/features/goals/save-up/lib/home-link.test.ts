import { describe, expect, it } from "vitest";
import type { Goal, GoalPlan } from "@/lib/types";
import { homePurchaseDate, linkedHomeGoalId, settingsWithLinkedHome } from "./home-link";

type Settings = Record<string, Record<string, unknown> | null>;

function parsed(json: string): Settings {
  return JSON.parse(json) as Settings;
}

function retirementPlan(settings: Record<string, unknown>): GoalPlan {
  return {
    goalId: "retirement",
    planKind: "retirement",
    plannerMode: "traditional",
    settingsJson: JSON.stringify(settings),
    summaryJson: "{}",
    version: 1,
    createdAt: "",
    updatedAt: "",
  };
}

describe("home link", () => {
  it("reads the linked home from the tax-optimized settings", () => {
    expect(linkedHomeGoalId(retirementPlan({ taxOptimizer: { homeGoalId: "home" } }))).toBe("home");
    expect(linkedHomeGoalId(retirementPlan({}))).toBeNull();
    expect(linkedHomeGoalId(null)).toBeNull();
  });

  it("links and unlinks a home, keeping every other setting and saved result", () => {
    const plan = retirementPlan({
      personal: { currentAge: 45 },
      taxOptimizer: { state: "VA", homeGoalId: "old" },
      taxPlanStore: { cases: [{ id: "case" }] },
    });

    const linked = parsed(settingsWithLinkedHome(plan, "new"));
    expect(linked.taxOptimizer).toEqual({ state: "VA", homeGoalId: "new" });
    expect(linked.personal).toEqual({ currentAge: 45 });
    expect(linked.taxPlanStore).toEqual({ cases: [{ id: "case" }] });

    const unlinked = parsed(settingsWithLinkedHome(plan, null));
    expect(unlinked.taxOptimizer?.homeGoalId).toBeNull();
  });

  it("starts the tax-optimized settings when a plan has none", () => {
    const linked = parsed(settingsWithLinkedHome(retirementPlan({}), "home"));
    expect(linked.taxOptimizer).toEqual({ homeGoalId: "home" });
  });

  it("dates a purchase at an age from the retirement goal's birth month", () => {
    const goal = { targetDate: "2030-05-01" } as Goal;
    const home = (purchase: Record<string, unknown>) =>
      ({ settingsJson: JSON.stringify({ homePurchase: purchase }) }) as GoalPlan;
    const birth = retirementPlan({ personal: { birthYearMonth: "1980-03" } });

    expect(homePurchaseDate(goal, home({ price: 1 }), birth)).toBe("2030-05-01");
    expect(homePurchaseDate(goal, home({ price: 1, purchaseAge: 50 }), birth)).toBe("2030-03-01");
    expect(homePurchaseDate(goal, home({ price: 1, purchaseAge: 50 }), null)).toBeNull();
  });
});
