import type { Goal, GoalPlan } from "@/lib/types";
import { parseHomePurchase, purchaseDateAtAge, rawPlanSettings } from "./home-purchase";

/**
 * The home goal a retirement plan includes. The link lives with the
 * tax-optimized settings (`taxOptimizer.homeGoalId`), where it started, and
 * every planning style reads it from there.
 */
export function linkedHomeGoalId(retirementPlan: GoalPlan | null | undefined): string | null {
  const settings = rawPlanSettings(retirementPlan).taxOptimizer as
    | { homeGoalId?: string | null }
    | undefined;
  return settings?.homeGoalId ?? null;
}

/** The retirement plan's settings JSON including `homeGoalId` (none with null), all else kept. */
export function settingsWithLinkedHome(
  retirementPlan: GoalPlan,
  homeGoalId: string | null,
): string {
  const settings = rawPlanSettings(retirementPlan);
  const taxOptimizer = (settings.taxOptimizer ?? {}) as Record<string, unknown>;
  return JSON.stringify({ ...settings, taxOptimizer: { ...taxOptimizer, homeGoalId } });
}

/** The purchase date of a home goal: at an age (needs a birth month) or on its target date. */
export function homePurchaseDate(
  goal: Goal,
  plan: GoalPlan | null | undefined,
  retirementPlan: GoalPlan | null | undefined,
): string | null {
  const purchase = parseHomePurchase(plan);
  if (purchase?.purchaseAge != null) {
    const personal = rawPlanSettings(retirementPlan).personal as
      | { birthYearMonth?: string }
      | undefined;
    return personal?.birthYearMonth
      ? purchaseDateAtAge(personal.birthYearMonth, purchase.purchaseAge)
      : null;
  }
  return goal.targetDate ?? null;
}
