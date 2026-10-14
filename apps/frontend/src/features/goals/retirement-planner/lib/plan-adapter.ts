import type { ExpenseItem, RetirementPlan } from "../types";
import { defaultExpenseItems, normalizeExpenseBudget } from "./expense-items";

function formatYearMonth(year: number, monthIndex: number) {
  return `${year}-${String(monthIndex + 1).padStart(2, "0")}`;
}

export function inferBirthYearMonthFromAge(age: number, asOf = new Date()) {
  return formatYearMonth(asOf.getFullYear() - Math.max(0, Math.round(age)), asOf.getMonth());
}

/** Oldest age a birth month may give; older ones are taken as unfinished input. */
const MAX_BIRTH_AGE = 110;

/**
 * Age at `asOf` for a "YYYY-MM" birth month, or `undefined` when it is not a
 * plausible birth month. A month input reports each keystroke of the year
 * ("0001", "0019", "0197"), and those must not count as ages near 2000.
 */
export function ageFromBirthYearMonth(birthYearMonth: string, asOf = new Date()) {
  const [yearRaw, monthRaw] = birthYearMonth.split("-");
  const year = Number(yearRaw);
  const month = Number(monthRaw);
  if (!Number.isInteger(year) || !Number.isInteger(month) || month < 1 || month > 12) {
    return undefined;
  }
  let age = asOf.getFullYear() - year;
  if (asOf.getMonth() + 1 < month) age -= 1;
  return age >= 0 && age <= MAX_BIRTH_AGE ? age : undefined;
}

export function normalizePersonalProfile(
  personal: Partial<RetirementPlan["personal"]> | undefined,
  asOf = new Date(),
): RetirementPlan["personal"] {
  const fallback = DEFAULT_RETIREMENT_PLAN.personal;
  const currentAge = Math.round(personal?.currentAge ?? fallback.currentAge);
  const birthYearMonth = personal?.birthYearMonth ?? inferBirthYearMonthFromAge(currentAge, asOf);
  const derivedAge = ageFromBirthYearMonth(birthYearMonth, asOf) ?? currentAge;
  const targetRetirementAge = Math.max(
    derivedAge + 1,
    Math.round(personal?.targetRetirementAge ?? fallback.targetRetirementAge),
  );
  const planningHorizonAge = Math.max(
    targetRetirementAge + 1,
    Math.round(personal?.planningHorizonAge ?? fallback.planningHorizonAge),
  );

  return {
    ...fallback,
    ...personal,
    birthYearMonth,
    currentAge: derivedAge,
    targetRetirementAge,
    planningHorizonAge,
  };
}

/** Ids of the spending items rent and an owned home's costs became. */
export const RENT_EXPENSE_ID = "housing-rent";
export const HOME_COSTS_EXPENSE_ID = "housing-home-costs";

/**
 * Moves rent or the owned home's costs, kept in the tax-optimized settings
 * before spending items could end at a home purchase, into must-have spending
 * items. Mirrors `move_housing_to_expenses` in
 * crates/core/src/planning/retirement/model.rs, with the same ids, so the
 * plan reads the same on both sides before and after it is saved.
 */
export function moveHousingToExpenses(plan: RetirementPlan): RetirementPlan {
  const settings = plan.taxOptimizer;
  if (!settings) return plan;
  const housing = settings.housing ?? "own";
  const rent = settings.monthlyRent ?? 0;
  const homeCosts = settings.homeCosts ?? 0;
  const moved: ExpenseItem | null =
    housing === "rent" && rent > 0
      ? {
          id: RENT_EXPENSE_ID,
          label: "Rent",
          monthlyAmount: rent,
          essential: true,
          endsAtHomePurchase: true,
        }
      : housing === "own" && homeCosts > 0
        ? {
            id: HOME_COSTS_EXPENSE_ID,
            label: "Home costs (tax, insurance, upkeep)",
            monthlyAmount: homeCosts / 12,
            essential: true,
            yearly: true,
            endsAtHomePurchase: settings.sellCurrentHome || undefined,
          }
        : null;
  if (!moved) return plan;
  const items = plan.expenses?.items ?? [];
  return {
    ...plan,
    expenses: {
      ...plan.expenses,
      items: items.some((item) => item.id === moved.id) ? items : [...items, moved],
    },
    taxOptimizer: { ...settings, monthlyRent: 0, homeCosts: 0 },
  };
}

export function normalizeRetirementPlan(plan: RetirementPlan, asOf = new Date()): RetirementPlan {
  const moved = moveHousingToExpenses(plan);
  return {
    ...moved,
    personal: normalizePersonalProfile(moved.personal, asOf),
    expenses: normalizeExpenseBudget(moved.expenses),
    incomeStreams: moved.incomeStreams ?? [],
  };
}

export function normalizeDashboardRetirementPlan(
  plan: RetirementPlan,
  asOf = new Date(),
): RetirementPlan {
  return normalizeRetirementPlan(plan, asOf);
}

export const DEFAULT_RETIREMENT_PLAN: RetirementPlan = {
  version: "v3",
  personal: {
    currentAge: 30,
    targetRetirementAge: 50,
    planningHorizonAge: 90,
  },
  expenses: {
    items: defaultExpenseItems(),
  },
  incomeStreams: [],
  investment: {
    preRetirementAnnualReturn: 0.0577,
    retirementAnnualReturn: 0.0337,
    annualInvestmentFeeRate: 0.006,
    annualVolatility: 0.12,
    inflationRate: 0.02,
    monthlyContribution: 1000,
    contributionGrowthRate: 0.02,
  },
  tax: {
    taxableWithdrawalRate: 0.15,
    taxDeferredWithdrawalRate: 0.25,
    taxFreeWithdrawalRate: 0,
    earlyWithdrawalPenaltyRate: 0,
    earlyWithdrawalPenaltyAge: 65,
  },
  currency: "USD",
};

/** DEFAULT_RETIREMENT_PLAN with its currency placeholder set to the account's base currency. */
export function createDefaultRetirementPlan(
  baseCurrency: string,
  asOf = new Date(),
): RetirementPlan {
  return {
    ...DEFAULT_RETIREMENT_PLAN,
    personal: normalizePersonalProfile(DEFAULT_RETIREMENT_PLAN.personal, asOf),
    currency: baseCurrency,
  };
}

export function parseSettingsJson(json: string, asOf = new Date()): RetirementPlan {
  try {
    const raw = JSON.parse(json);
    // Saved Tax Plan results live beside the plan; see `parseTaxPlanStore`.
    const {
      withdrawal: _legacyWithdrawal,
      taxPlanStore: _taxPlanStore,
      ...rawWithoutWithdrawal
    } = raw ?? {};
    const rawInvestment = raw.investment ?? {};
    const {
      expectedAnnualReturn,
      expectedReturnStdDev,
      targetAllocations,
      ...investmentWithoutLegacy
    } = rawInvestment;
    void targetAllocations;
    return {
      ...DEFAULT_RETIREMENT_PLAN,
      ...rawWithoutWithdrawal,
      version: "v3",
      personal: normalizePersonalProfile(raw.personal, asOf),
      expenses: normalizeExpenseBudget(raw.expenses ?? DEFAULT_RETIREMENT_PLAN.expenses),
      incomeStreams: raw.incomeStreams ?? DEFAULT_RETIREMENT_PLAN.incomeStreams,
      investment: {
        ...DEFAULT_RETIREMENT_PLAN.investment,
        ...investmentWithoutLegacy,
        preRetirementAnnualReturn:
          rawInvestment.preRetirementAnnualReturn ??
          expectedAnnualReturn ??
          DEFAULT_RETIREMENT_PLAN.investment.preRetirementAnnualReturn,
        retirementAnnualReturn:
          rawInvestment.retirementAnnualReturn ??
          DEFAULT_RETIREMENT_PLAN.investment.retirementAnnualReturn,
        annualInvestmentFeeRate:
          rawInvestment.annualInvestmentFeeRate ??
          DEFAULT_RETIREMENT_PLAN.investment.annualInvestmentFeeRate,
        annualVolatility:
          rawInvestment.annualVolatility ??
          expectedReturnStdDev ??
          DEFAULT_RETIREMENT_PLAN.investment.annualVolatility,
      },
      tax: raw.tax ?? DEFAULT_RETIREMENT_PLAN.tax,
    };
  } catch {
    return createDefaultRetirementPlan(DEFAULT_RETIREMENT_PLAN.currency, asOf);
  }
}
