import type { GoalPlan } from "@/lib/types";
import type { HomePurchase } from "../../retirement-planner/types";

/** Keep in sync with `HomePurchase::default` in crates/core/src/planning/home_purchase.rs. */
export const DEFAULT_HOME_PURCHASE: HomePurchase = {
  price: 0,
  downPaymentRate: 0.2,
  closingCostRate: 0.03,
  mortgageRate: 0.065,
  termYears: 30,
  propertyTaxRate: 0.01,
  insurance: 1500,
  maintenanceRate: 0.01,
  hoaMonthly: 0,
  appreciation: 0.03,
  purchaseAge: null,
};

/** A goal plan's settings JSON as an object (empty when missing or invalid). */
export function rawPlanSettings(plan: GoalPlan | null | undefined): Record<string, unknown> {
  if (!plan?.settingsJson) return {};
  try {
    const parsed: unknown = JSON.parse(plan.settingsJson);
    return parsed && typeof parsed === "object" && !Array.isArray(parsed)
      ? (parsed as Record<string, unknown>)
      : {};
  } catch {
    return {};
  }
}

/** The purchase saved with a home goal's savings plan, if any. */
export function parseHomePurchase(plan: GoalPlan | null | undefined): HomePurchase | null {
  const raw = rawPlanSettings(plan).homePurchase;
  if (!raw || typeof raw !== "object") return null;
  return { ...DEFAULT_HOME_PURCHASE, ...(raw as Partial<HomePurchase>) };
}

/** The first of the month someone born in `birthYearMonth` (YYYY-MM) turns `age`. */
export function purchaseDateAtAge(birthYearMonth: string, age: number): string | null {
  const [year, month] = birthYearMonth.split("-").map(Number);
  if (!year || !month) return null;
  return `${year + age}-${String(month).padStart(2, "0")}-01`;
}

export interface HomePurchaseCosts {
  /** Years from `today` to the purchase (0 when it is past). */
  years: number;
  priceAtPurchase: number;
  downPayment: number;
  closingCosts: number;
  /** Down payment plus closing costs: what the goal saves. */
  savingsTarget: number;
  loan: number;
  monthlyPayment: number;
  /** Property tax, insurance, maintenance and HOA per month, in the first year. */
  monthlyOwningCosts: number;
}

/** Monthly principal and interest; matches `HomePurchase::monthly_payment`. */
export function monthlyPayment(principal: number, rate: number, termYears: number): number {
  const months = termYears * 12;
  if (principal <= 0 || months <= 0) return 0;
  const r = rate / 12;
  return r <= 0 ? principal / months : (principal * r) / (1 - Math.pow(1 + r, -months));
}

/** What buying on `purchaseDate` costs, with the price growing by `appreciation`. */
export function homePurchaseCosts(
  home: HomePurchase,
  purchaseDate: string | null,
  today: Date,
): HomePurchaseCosts | null {
  if (!purchaseDate) return null;
  const date = new Date(`${purchaseDate}T00:00:00`);
  if (Number.isNaN(date.getTime())) return null;
  const years = Math.max(0, (date.getTime() - today.getTime()) / (365.25 * 24 * 3600 * 1000));
  const priceAtPurchase = home.price * Math.pow(1 + home.appreciation, years);
  const downPayment = priceAtPurchase * home.downPaymentRate;
  const closingCosts = priceAtPurchase * home.closingCostRate;
  const loan = priceAtPurchase - downPayment;
  return {
    years,
    priceAtPurchase,
    downPayment,
    closingCosts,
    savingsTarget: downPayment + closingCosts,
    loan,
    monthlyPayment: monthlyPayment(loan, home.mortgageRate, home.termYears),
    monthlyOwningCosts:
      (priceAtPurchase * (home.propertyTaxRate + home.maintenanceRate) + home.insurance) / 12 +
      home.hoaMonthly,
  };
}
