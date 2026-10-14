// All monetary values in the user's base currency (from Wealthfolio settings)
// All rates as decimals (0.065 = 6.5%)

export interface GlidepathSettings {
  enabled: boolean;
  /** Expected annual return for the bond portion (e.g. 0.03 = 3%). */
  bondReturnRate: number;
  /** Fraction held in bonds at the FIRE date (e.g. 0.2 = 20%). */
  bondAllocationAtFire: number;
  /** Fraction held in bonds at the planning horizon (e.g. 0.5 = 50%). */
  bondAllocationAtHorizon: number;
}

export interface FireProjection {
  /** Age when portfolio first reached the FIRE target. null if target was never reached. */
  fireAge: number | null;
  fireYear: number | null;
  retirementStartAge: number | null;
  retirementStartReason?: RetirementStartReason | null;
  portfolioAtFire: number;
  /** True when withdrawal phase started with portfolio >= required capital. */
  fundedAtRetirement: boolean;
  coastFireAmount: number;
  coastFireReached: boolean;
  yearByYear: YearlySnapshot[];
}

export type RetirementStartReason = "funded" | "target_age_forced";

export interface YearlySnapshot {
  age: number;
  year: number;
  phase: "accumulation" | "fire";
  portfolioValue: number;
  portfolioEndValue: number;
  annualContribution: number;
  annualWithdrawal: number;
  annualIncome: number;
  netWithdrawalFromPortfolio: number;
  pensionAssets: number;
  annualTaxes?: number;
  grossWithdrawal?: number;
  plannedExpenses?: number;
  fundedExpenses?: number;
  annualShortfall?: number;
}

export interface MonteCarloResult {
  successRate: number;
  /** Median age at which the FI target was genuinely reached. null for underfunded plans
   *  where fewer than 50% of simulations hit the target before the horizon. */
  medianFireAge: number | null;
  percentiles: {
    p10: number[];
    p25: number[];
    p50: number[];
    p75: number[];
    p90: number[];
  };
  ageAxis: number[];
  finalPortfolioAtHorizon: {
    p10: number;
    p25: number;
    p50: number;
    p75: number;
    p90: number;
  };
  nSimulations: number;
}

export interface AllocationHealth {
  symbol: string;
  name: string;
  currentWeight: number;
  targetWeight: number;
  drift: number;
  status: "underweight" | "overweight" | "ok";
  currentValue: number;
  daysSinceLastBuy: number | null;
}

export interface ScenarioResult {
  label: string;
  annualReturn: number;
  fireAge: number | null;
  portfolioAtHorizon: number;
  fundedAtGoalAge: boolean;
  success: boolean;
  failureAge?: number | null;
  spendingShortfallAge?: number | null;
  yearByYear: YearlySnapshot[];
}

export interface SorrScenario {
  label: string;
  returns: number[];
  portfolioPath: number[];
  finalValue: number;
  survived: boolean;
  failureAge?: number | null;
  spendingShortfallAge?: number | null;
}

export type DecisionSensitivityMap = "contribution-return" | "retirement-age-spending";

export interface DecisionSensitivityMatrix {
  rowLabel: string;
  columnLabel: string;
  rowValues: number[];
  columnValues: number[];
  rowLabels: string[];
  columnLabels: string[];
  cells: DecisionSensitivityCell[][];
  baselineRow?: number | null;
  baselineColumn?: number | null;
}

export interface DecisionSensitivityCell {
  fiAge: number | null;
  retirementStartAge: number | null;
  fundedAtGoalAge: boolean;
  shortfallAtGoalAge: number;
  portfolioAtHorizon: number;
}

export type StressTestId =
  | "return-drag"
  | "inflation-shock"
  | "spending-shock"
  | "retire-earlier"
  | "save-less"
  | "early-crash";

export type StressCategory = "market" | "inflation" | "spending" | "timing" | "saving";

export type StressSeverity = "low" | "medium" | "high";

export interface StressOutcome {
  fiAge: number | null;
  retirementStartAge: number | null;
  fundedAtGoalAge: boolean;
  shortfallAtGoalAge: number;
  portfolioAtHorizon: number;
  failureAge?: number | null;
  spendingShortfallAge?: number | null;
}

export interface StressDelta {
  fiAgeYears: number | null;
  shortfallAtGoalAge: number;
  portfolioAtHorizon: number;
}

export interface StressTestResult {
  id: StressTestId;
  label: string;
  description: string;
  category: StressCategory;
  baseline: StressOutcome;
  stressed: StressOutcome;
  delta: StressDelta;
  severity: StressSeverity;
}

// ─── Retirement Plan Types ──────────────────────────────────────────────────

export interface RetirementPlan {
  version: "v3";
  personal: PersonalProfile;
  expenses: ExpenseBudget;
  incomeStreams: RetirementIncomeStream[];
  investment: InvestmentAssumptions;
  tax?: TaxProfile;
  currency: string;
  /** Inputs used only by the tax-optimized planning style. */
  taxOptimizer?: TaxOptimizerSettings;
}

export type TaxOptimizerObjective = "maxBequest" | "maxSpending";
/** Retire at the target age, or find the earliest age that covers the must-haves. */
export type TaxPlanGoal = "fixedAge" | "earliestAge";
export type Housing = "own" | "rent";

/** Inputs for the tax-optimized (MILP) planning style, in today's money per year. */
export interface TaxOptimizerSettings {
  /** Income stream that represents Social Security (start age = claim age). */
  socialSecurityStreamId?: string | null;
  /** Social Security entered directly (used when no stream is linked). */
  socialSecurityMonthly?: number;
  socialSecurityClaimAge?: number | null;
  /** Let the optimizer choose the claiming month instead of the planned age. */
  optimizeClaimAge?: boolean;
  goal?: TaxPlanGoal;
  /** No longer used: the must-have spending items are the floor. */
  minimumExpenses?: number;
  housing?: Housing;
  /** Rent per month while renting. */
  monthlyRent?: number;
  /** Yearly property tax, insurance, maintenance and HOA of an owned home. */
  homeCosts?: number;
  /** Active home goal whose purchase the plan includes. */
  homeGoalId?: string | null;
  /** Share of market paths the safe spending must succeed on (default 0.9). */
  successTarget?: number | null;
  /** Paths a Monte Carlo risk check draws (default 100). */
  simulationCount?: number | null;
  /** Paths the Monte Carlo conversion regret check draws (default 30). */
  regretSimulationCount?: number | null;
  /** Give each Monte Carlo run a random lifespan; unset ends every run at the horizon. */
  randomLifespans?: RandomLifespans | null;
  /** Sell the current home when the linked home is bought. */
  sellCurrentHome?: boolean;
  /** The current home's value today. */
  currentHomeValue?: number;
  /** Selling costs as a share of the sale price (default 6%). */
  sellingCostRate?: number;
  /** Label of the debt the sale repays. */
  currentHomeDebt?: string | null;
  objective?: TaxOptimizerObjective;
  /** After-tax estate to leave when maximizing spending. */
  bequestFloor?: number;
  heirsTaxRate?: number;
  maxRothConversion?: number | null;
  dividendYield?: number;
  /** Pre-tax 401(k) deferral, taken out of salary before tax. */
  preTaxContribution?: number;
  employerContribution?: number;
  rothContribution?: number;
  taxableContribution?: number;
  /** Most recent year first. */
  recentRothConversions?: number[];
  recentRothContributions?: number[];
  priorMagi?: number | null;
  spendingFlexibility?: number;
  /** Two-letter state; only "VA" is modelled. */
  state?: string | null;
  /** Yearly benchmark silver ACA premium for retired years before Medicare. */
  acaBenchmarkPremium?: number;
  hsaContribution?: number;
  otherMedicalExpenses?: number;
  ruleOf55Balance?: number | null;
  seppStartAge?: number | null;
  seppAnnualPayment?: number;
  harvestGains?: boolean;
  /** Let the optimizer choose 401(k)/Roth IRA/HSA contributions (salary is gross). */
  optimizeContributions?: boolean;
  /** Employer match: this share of deferrals, up to matchCap of salary. */
  matchRate?: number;
  matchCap?: number;
  hsaEligible?: boolean;
  /** Family (rather than self-only) HSA coverage, which sets the HSA limit. */
  hsaFamilyCoverage?: boolean;
  roth401kAvailable?: boolean;
  /** With optimizeContributions: share (0–1) of leftover pay invested; null invests all. */
  leftoverInvestRate?: number | null;
  debts?: TaxOptimizerDebt[];
  /** Stock share (the rest in bonds) of a bucket with no holdings and no mix entered. */
  equityShare?: number;
  markets?: MarketSettings;
}

/** Where yearly market returns come from (Owl's rate options). */
export type ReturnMethod = "plan" | "fixed" | "historical" | "historicalAverage";
export type InflationMethod = "fixed" | "historical";
export type GlideShape = "linear" | "sCurve";

/** Weights of stocks (S&P 500), Baa corporate bonds, 10-year T-notes and cash. */
export interface AssetMix {
  stocks: number;
  bonds: number;
  notes: number;
  cash: number;
}

export interface BucketMixes {
  taxable: AssetMix;
  taxDeferred: AssetMix;
  roth: AssetMix;
  hsa: AssetMix;
}

/** Market assumptions of the tax-optimized plan; rates are decimals. */
export interface MarketSettings {
  method?: ReturnMethod;
  fixed?: { stocks: number; bonds: number; notes: number };
  inflationMethod?: InflationMethod;
  /** Fixed inflation; unset uses the projection assumption. */
  inflation?: number | null;
  startYear?: number;
  averageFrom?: number;
  averageTo?: number;
  /** Each bucket's mix today; unset derives it from the holdings. */
  startingMix?: BucketMixes | null;
  /** The mix every bucket glides to; unset keeps the starting mix. */
  targetMix?: AssetMix | null;
  glide?: GlideShape;
  /** Age the target mix is reached; unset is the planning horizon. */
  targetAge?: number | null;
}

export interface TaxOptimizerDebt {
  label: string;
  balance: number;
  /** Yearly rate, 0.065 = 6.5%. */
  annualRate: number;
  annualPayment: number;
}

export type TaxSuccessMode = "historical" | "bootstrap" | "lognormal";

export type Sex = "Female" | "Male";
/** Owl's actuarial mortality tables. */
export type MortalityTable =
  | "SSA2025"
  | "VBT2015-SM"
  | "VBT2015-NS"
  | "RP2014"
  | "IAM2012"
  | "Pub2010-General"
  | "Pub2010-Safety"
  | "Pub2010-Teacher";

/** Where random lifespans are drawn from, given the person lives to retire. */
export interface RandomLifespans {
  sex: Sex;
  table: MortalityTable;
}

export interface TaxPlanSuccess {
  mode: TaxSuccessMode;
  netSpending: number;
  paths: number;
  funded: number;
  successRate: number;
  /** 10th, 50th, 90th percentile after-tax estates. */
  bequestPercentiles: [number, number, number];
  failedPaths: string[];
  /** 10th, 50th, 90th percentile ages at death, when each run drew a lifespan. */
  lifespans?: [number, number, number] | null;
}

export interface SuccessFrontierPoint {
  spending: number;
  successRate: number;
}

/** The most the plan can spend at a target success rate, and the whole trade-off. */
export interface TaxPlanSafeSpending {
  mode: TaxSuccessMode;
  targetRate: number;
  /** First-year spending (in the planned shape) at the target rate, today's money. */
  spending: number;
  paths: number;
  plannedSpending: number;
  plannedSuccessRate: number;
  /** From low to high spending. */
  frontier: SuccessFrontierPoint[];
  /** The most each path can spend, in path order (historical paths by start year). */
  pathSpending?: PathSpending[];
  /** As in TaxPlanSuccess. */
  lifespans?: [number, number, number] | null;
}

export interface PathSpending {
  /** The path's first historical year, or its sample number. */
  label: string;
  spending: number;
}

/** One choice nudged, and what that does to the plan's goal. */
export interface ChoiceCost {
  choice:
    | "minimumExpenses"
    | "spending"
    | "bequestFloor"
    | "retireLater"
    | "retireEarlier"
    | "homePurchase"
    | "conversionCap"
    | "claimAge";
  change: number;
  /** Change in first-year spending (max spending) or estate (max estate); null when unsolvable. */
  effect: number | null;
}

export interface TaxPlanChoiceCosts {
  objective: TaxOptimizerObjective;
  base: number;
  costs: ChoiceCost[];
}

/** One shock the plan does not control, and what it does to the plan's goal. */
export interface StressCase {
  stress:
    | "lowerReturns"
    | "higherInflation"
    | "higherExpenses"
    | "socialSecurityCut"
    | "liveLonger"
    | "retireInto";
  /** The historical year a `retireInto` stress replays from retirement on. */
  year: number | null;
  /** Change in first-year spending (max spending) or estate (max estate); null when unsolvable. */
  effect: number | null;
}

export interface TaxPlanStressTests {
  objective: TaxOptimizerObjective;
  base: number;
  stresses: StressCase[];
}

export interface ConversionOption {
  amount: number;
  meanRegret: number;
  worstRegret: number;
  bestShare: number;
  fundedShare: number;
}

/** How this year's recommended Roth conversion fares across market paths. */
export interface TaxPlanConversionRegret {
  mode: TaxSuccessMode;
  paths: number;
  recommended: number;
  netSpending: number;
  options: ConversionOption[];
}

/** A Tax Plan result kept with the goal so revisiting the page needs no re-run. */
export interface SavedTaxPlanRun {
  /** `planInputsKey` of the plan it was solved for. */
  inputsKey: string;
  /** ISO timestamp of the solve. */
  computedAt: string;
  result: TaxOptimizedPlan;
  /** The last success-rate check of this result. */
  success?: TaxPlanSuccess;
  /** The last safe-spending search of this result. */
  safeSpending?: TaxPlanSafeSpending;
  /** The last choice-cost analysis and conversion regret check of this result. */
  choiceCosts?: TaxPlanChoiceCosts;
  conversionRegret?: TaxPlanConversionRegret;
  /** The last stress tests of this result. */
  stressTests?: TaxPlanStressTests;
}

/** A named snapshot of the plan's inputs and its result ("Retire at 64"). */
export interface TaxPlanCase {
  id: string;
  name: string;
  savedAt: string;
  plan: RetirementPlan;
  run?: SavedTaxPlanRun;
}

/**
 * Tax Plan results and cases, stored under `taxPlanStore` in the goal plan's
 * settings JSON beside (not inside) the `RetirementPlan`, so simulations and
 * plan comparisons never carry them.
 */
export interface TaxPlanStore {
  lastRun?: SavedTaxPlanRun;
  cases: TaxPlanCase[];
}

export interface TaxOptimizerAccounts {
  taxable: number;
  taxableCostBasis: number;
  taxDeferred: number;
  roth: number;
  hsa: number;
  /** Each bucket's asset mix from its holdings; null when there are none. */
  mixes?: BucketMixes | null;
  /** Value of holdings without an asset class, counted as stocks. */
  unclassified?: number;
}

/** One year of the optimized playbook, in today's money. */
export interface TaxOptimizedYear {
  year: number;
  age: number;
  working: boolean;
  taxableBalance: number;
  taxDeferredBalance: number;
  rothBalance: number;
  hsaBalance: number;
  wages: number;
  taxableContribution: number;
  taxDeferredContribution: number;
  rothContribution: number;
  hsaContribution: number;
  taxableWithdrawal: number;
  taxDeferredWithdrawal: number;
  rothWithdrawal: number;
  hsaWithdrawal: number;
  rothConversion: number;
  taxableDeposit: number;
  /** Leftover pay spent instead of invested. */
  leftoverSpent?: number;
  gainHarvest: number;
  /** Employer contributions, given or the chosen match. */
  employerMatch: number;
  /** Employee pre-tax 401(k) deferrals (absent from older results). */
  preTaxDeferral?: number;
  /** Chosen Roth 401(k) and Roth IRA; a given Roth contribution is in neither. */
  roth401k?: number;
  rothIra?: number;
  debtPayment: number;
  debtBalance: number;
  socialSecurity: number;
  otherIncome: number;
  /** Untaxed money in (+) or out (-): a down payment, a home sale. */
  cashFlow?: number;
  requiredMinimumDistribution: number;
  netSpending: number;
  /** Must-have expenses the plan has to cover this year. */
  requiredSpending: number;
  ordinaryTaxableIncome: number;
  capitalGains: number;
  federalIncomeTax: number;
  capitalGainsTax: number;
  stateIncomeTax: number;
  earlyWithdrawalPenalty: number;
  medicare: number;
  acaPremium: number;
  irmaaTier: number | null;
  magi: number;
  inflationFactor: number;
}

export type TaxOptimizedStatus = "optimal" | "optimalWithinGap" | "timeLimit" | "infeasible";

/** The conventional strategy (taxable first, no Roth conversions) for comparison. */
export interface TaxPlanBaseline {
  status: TaxOptimizedStatus;
  firstYearSpending: number;
  bequestAfterTax: number;
  lifetimeFederalTax: number;
  lifetimeStateTax: number;
  lifetimeMedicare: number;
  lifetimeAca: number;
  lifetimeEarlyWithdrawalPenalty: number;
}

export interface TaxOptimizedPlan {
  objective: TaxOptimizerObjective;
  status: TaxOptimizedStatus;
  converged: boolean;
  iterations: number;
  infeasibleReason: string | null;
  startingAccounts: TaxOptimizerAccounts;
  socialSecurityClaimAge: number | null;
  firstYearSpending: number;
  bequestAfterTax: number;
  lifetimeSpending: number;
  lifetimeFederalTax: number;
  lifetimeStateTax: number;
  lifetimeMedicare: number;
  lifetimeAca: number;
  lifetimeEarlyWithdrawalPenalty: number;
  lifetimeRothConversions: number;
  years: TaxOptimizedYear[];
  baseline: TaxPlanBaseline | null;
  /** Retirement age the plan was solved for. */
  retirementAge: number;
  /** Every age compared by the earliest-age search (empty otherwise). */
  retirementAgeOptions: RetirementAgeOption[];
  /** The linked home purchase, when there is one. */
  home?: HomePlanSummary | null;
  /** Given contributions held to their IRS limits, and from which age. */
  contributionCaps?: ContributionCap[];
}

/** A given contribution growth would take past its IRS limit, held there from `age`. */
export interface ContributionCap {
  account: "preTax401k" | "roth" | "hsa";
  age: number;
}

/** A linked home purchase as the plan models it, in today's money. */
export interface HomePlanSummary {
  goalTitle: string;
  purchaseYear: number;
  purchaseAge: number;
  price: number;
  /** Down payment plus closing costs. */
  upfrontCost: number;
  mortgage: number;
  monthlyPayment: number;
  yearlyOwningCosts: number;
  saleProceeds: number;
  /** The home's value at the end of the plan, beside the after-tax estate. */
  valueAtEnd: number;
}

/** A home goal's planned purchase, under `homePurchase` in its save-up settings. */
export interface HomePurchase {
  price: number;
  downPaymentRate: number;
  closingCostRate: number;
  mortgageRate: number;
  termYears: number;
  propertyTaxRate: number;
  /** Yearly homeowner's insurance. */
  insurance: number;
  maintenanceRate: number;
  hoaMonthly: number;
  appreciation: number;
  /** Buy at this age instead of on the goal's target date. */
  purchaseAge?: number | null;
}

/** One candidate age of the earliest-retirement search, in today's money. */
export interface RetirementAgeOption {
  age: number;
  feasible: boolean;
  /** Average yearly spending sustained once retired (0 when not feasible). */
  retirementSpending: number;
  /** Average yearly must-have expenses once retired. */
  requiredSpending: number;
  bequestAfterTax: number;
}

export interface PersonalProfile {
  birthYearMonth?: string;
  currentAge: number;
  targetRetirementAge: number;
  planningHorizonAge: number;
  currentAnnualSalary?: number;
  salaryGrowthRate?: number;
}

export interface ExpenseBudget {
  items: ExpenseItem[];
}

export interface ExpenseItem {
  id: string;
  label: string;
  monthlyAmount: number;
  inflationRate?: number;
  startAge?: number;
  endAge?: number;
  essential?: boolean;
  /** Entered and shown per year; `monthlyAmount` still holds the monthly amount. */
  yearly?: boolean;
  /** Ends when the linked home is bought, such as rent. */
  endsAtHomePurchase?: boolean;
}

export interface RetirementIncomeStream {
  id: string;
  label: string;
  streamType: "db" | "dc";
  startAge: number;
  adjustForInflation: boolean;
  annualGrowthRate?: number;
  monthlyAmount?: number;
  linkedAccountId?: string;
  currentValue?: number;
  monthlyContribution?: number;
  accumulationReturn?: number;
  /** Annual draw rate applied to the projected fund balance once payouts start. */
  payoutRate?: number;
  /** What happens to the fund at startAge. Absent means "annuity". */
  payoutMode?: PayoutMode;
  /** Annual return the balance still invested earns during a drawdown payout phase. */
  postPayoutReturn?: number;
}

export type PayoutMode = "annuity" | "drawdown";

export interface InvestmentAssumptions {
  preRetirementAnnualReturn: number;
  retirementAnnualReturn: number;
  annualInvestmentFeeRate: number;
  annualVolatility: number;
  inflationRate: number;
  monthlyContribution: number;
  contributionGrowthRate: number;
  glidePath?: GlidepathSettings;
}

export interface TaxProfile {
  taxableWithdrawalRate: number;
  taxDeferredWithdrawalRate: number;
  taxFreeWithdrawalRate: number;
  earlyWithdrawalPenaltyRate?: number;
  earlyWithdrawalPenaltyAge?: number;
  countryCode?: string;
  withdrawalBuckets?: TaxBucketBalances;
}

export interface TaxBucketBalances {
  taxable: number;
  taxDeferred: number;
  taxFree: number;
}
