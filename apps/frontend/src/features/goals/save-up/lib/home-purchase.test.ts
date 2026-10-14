import { describe, expect, it } from "vitest";
import type { GoalPlan } from "@/lib/types";
import {
  DEFAULT_HOME_PURCHASE,
  homePurchaseCosts,
  monthlyPayment,
  parseHomePurchase,
  purchaseDateAtAge,
} from "./home-purchase";

const plan = (settingsJson: string) => ({ settingsJson }) as GoalPlan;

describe("home purchase", () => {
  it("matches the backend's mortgage payment", () => {
    expect(monthlyPayment(300_000, 0.06, 30)).toBeCloseTo(1798.65, 2);
    expect(monthlyPayment(360_000, 0, 30)).toBeCloseTo(1000, 9);
  });

  it("saves for the down payment and closing costs at the purchase price", () => {
    const home = { ...DEFAULT_HOME_PURCHASE, price: 400_000, appreciation: 0 };
    const costs = homePurchaseCosts(home, "2030-01-01", new Date("2026-01-01T00:00:00"));
    expect(costs?.savingsTarget).toBeCloseTo(400_000 * 0.23, 6);
    expect(costs?.loan).toBeCloseTo(320_000, 6);
    expect(homePurchaseCosts(home, null, new Date())).toBeNull();
  });

  it("reads the purchase and dates an age from the birth month", () => {
    expect(parseHomePurchase(plan('{"homePurchase":{"price":5}}'))?.termYears).toBe(30);
    expect(parseHomePurchase(plan('{"targetDate":"2030-01-01"}'))).toBeNull();
    expect(purchaseDateAtAge("1981-04", 52)).toBe("2033-04-01");
  });
});
