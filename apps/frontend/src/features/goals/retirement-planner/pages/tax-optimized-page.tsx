import {
  Badge,
  Button,
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
  TabsContent,
  useAmountFormatting,
  useNumberFormatting,
} from "@wealthfolio/ui";
import { getRetirementTaxPlanAccounts } from "@/adapters";
import { QueryKeys } from "@/lib/query-keys";
import { useQuery } from "@tanstack/react-query";
import { Icons } from "@wealthfolio/ui/components/ui/icons";
import { type ReactNode, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  RetirementAgeChart,
  TaxPlanBalancesChart,
  TaxPlanConversionsChart,
  TaxPlanSourcesChart,
} from "../components/tax-optimized-charts";
import { TaxOptimizerSettingsCard } from "../components/tax-optimizer-settings";
import { TaxPlanCaseBar } from "../components/tax-plan-cases";
import { TaxPlanCompare } from "../components/tax-plan-compare";
import { TaxPlanContributionsTable } from "../components/tax-plan-contributions";
import { comparableItems } from "../lib/tax-plan-compare";
import {
  TaxPlanChoiceCostsCard,
  TaxPlanConversionRegretCard,
  TaxPlanStressTestsCard,
} from "../components/tax-plan-insights";
import { TaxPlanSafeSpendingCard } from "../components/tax-plan-safe-spending";
import { TaxPlanSimulationsCard } from "../components/tax-plan-simulations";
import { TaxPlanSuccessCard } from "../components/tax-plan-success";
import { useTaxOptimizedPlan } from "../hooks/use-tax-optimized-plan";
import { balanceChange, planInputsKey, type TaxPlanTab } from "../lib/tax-plan-store";
import type {
  HomePlanSummary,
  RetirementPlan,
  SavedTaxPlanRun,
  TaxOptimizedPlan,
  TaxOptimizedYear,
  TaxOptimizerSettings,
  TaxPlanChoiceCosts,
  TaxPlanConversionRegret,
  TaxPlanSafeSpending,
  TaxPlanStore,
  TaxPlanStressTests,
  TaxPlanSuccess,
} from "../types";

interface Props {
  goalId: string;
  plan: RetirementPlan;
  onSavePlan: (plan: RetirementPlan) => void;
  /** Saved results and cases. */
  store: TaxPlanStore;
  /** Saves results and cases; `updated` also replaces the plan (loading a case). */
  onSaveStore: (store: TaxPlanStore, updated?: RetirementPlan) => void;
  /** Today's funding balances, to flag a saved result solved on other balances. */
  currentBalanceTotal?: number;
  /** The plan's ages, spending, income, assumptions and accounts, shown on the Plan tab. */
  planInputs: ReactNode;
  onTabChange: (tab: TaxPlanTab) => void;
}

function Metric({ label, value, hint }: { label: string; value: string; hint?: string }) {
  return (
    <Card>
      <CardContent className="space-y-1 py-4">
        <p className="text-muted-foreground text-xs">{label}</p>
        <p className="text-xl font-semibold tabular-nums">{value}</p>
        {hint && <p className="text-muted-foreground text-xs">{hint}</p>}
      </CardContent>
    </Card>
  );
}

function Summary({ result, currency }: { result: TaxOptimizedPlan; currency: string }) {
  const { t } = useTranslation();
  const { formatAmount } = useAmountFormatting();
  const money = (value: number) => formatAmount(value, currency);
  return (
    <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
      {result.objective === "maxSpending" ? (
        <Metric
          label={t("goals:tax_plan.summary.sustainable_spending")}
          value={money(result.firstYearSpending)}
          hint={t("goals:tax_plan.summary.per_year_today")}
        />
      ) : (
        <Metric
          label={t("goals:tax_plan.summary.estate")}
          value={money(result.bequestAfterTax)}
          hint={t("goals:tax_plan.summary.estate_hint")}
        />
      )}
      <Metric
        label={t("goals:tax_plan.summary.lifetime_tax")}
        value={money(
          result.lifetimeFederalTax +
            result.lifetimeStateTax +
            result.lifetimeEarlyWithdrawalPenalty,
        )}
        hint={t("goals:tax_plan.summary.lifetime_medicare", {
          amount: money(result.lifetimeMedicare + result.lifetimeAca),
        })}
      />
      <Metric
        label={t("goals:tax_plan.summary.roth_conversions")}
        value={money(result.lifetimeRothConversions)}
      />
      <Metric
        label={t("goals:tax_plan.summary.claim_age")}
        value={
          result.socialSecurityClaimAge == null
            ? "—"
            : t("goals:tax_plan.summary.claim_age_value", {
                age: Math.floor(result.socialSecurityClaimAge),
                months: Math.round((result.socialSecurityClaimAge % 1) * 12),
              })
        }
      />
    </div>
  );
}

/** The linked home purchase as solved, with the home's value beside the estate. */
function HomeCard({ home, currency }: { home: HomePlanSummary; currency: string }) {
  const { t } = useTranslation();
  const { formatAmount } = useAmountFormatting();
  const money = (value: number) => formatAmount(Math.round(value), currency);
  const rows: [string, string][] = [
    [t("goals:tax_plan.home.price"), money(home.price)],
    [t("goals:tax_plan.home.upfront"), money(home.upfrontCost)],
    [
      t("goals:tax_plan.home.mortgage"),
      t("goals:tax_plan.home.mortgage_value", {
        amount: money(home.mortgage),
        payment: money(home.monthlyPayment),
      }),
    ],
    [t("goals:tax_plan.home.owning_costs"), money(home.yearlyOwningCosts)],
  ];
  if (home.saleProceeds > 0) {
    rows.push([t("goals:tax_plan.home.sale_proceeds"), money(home.saleProceeds)]);
  }
  rows.push([t("goals:tax_plan.home.value_at_end"), money(home.valueAtEnd)]);
  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">
          {t("goals:tax_plan.home.title", {
            title: home.goalTitle,
            year: home.purchaseYear,
            age: home.purchaseAge,
          })}
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-3 text-sm">
        <dl className="grid gap-x-6 gap-y-2 sm:grid-cols-2 lg:grid-cols-3">
          {rows.map(([label, value]) => (
            <div key={label} className="flex justify-between gap-3 sm:block">
              <dt className="text-muted-foreground text-xs">{label}</dt>
              <dd className="font-medium tabular-nums">{value}</dd>
            </div>
          ))}
        </dl>
        <p className="text-muted-foreground text-xs">{t("goals:tax_plan.home.note")}</p>
      </CardContent>
    </Card>
  );
}

/** The earliest-age search: every candidate age, and the age the plan uses. */
function RetirementAgeCard({
  result,
  plan,
  onUseAge,
}: {
  result: TaxOptimizedPlan;
  plan: RetirementPlan;
  /** Absent for a saved case, which is read-only. */
  onUseAge?: (age: number) => void;
}) {
  const { t } = useTranslation();
  const { formatAmount } = useAmountFormatting();
  const options = result.retirementAgeOptions;
  const earliest = options.find((option) => option.feasible) ?? null;
  const latest = options[options.length - 1];
  const target = plan.personal.targetRetirementAge;
  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">{t("goals:tax_plan.age_search.title")}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3 text-sm">
        {earliest ? (
          <div className="flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
            <div>
              <p className="text-2xl font-semibold tabular-nums">
                {t("goals:tax_plan.age_search.earliest", { age: earliest.age })}
              </p>
              <p className="text-muted-foreground">
                {t("goals:tax_plan.age_search.earliest_detail", {
                  spending: formatAmount(Math.round(earliest.retirementSpending), plan.currency),
                  required: formatAmount(Math.round(earliest.requiredSpending), plan.currency),
                })}
              </p>
            </div>
            {onUseAge && earliest.age !== target && (
              <Button variant="outline" size="sm" onClick={() => onUseAge(earliest.age)}>
                {t("goals:tax_plan.age_search.use_age", { age: earliest.age })}
              </Button>
            )}
          </div>
        ) : (
          <p className="text-destructive font-medium">
            {t("goals:tax_plan.age_search.none", { age: latest?.age })}
          </p>
        )}
        <p className="text-muted-foreground">{t("goals:tax_plan.age_search.description")}</p>
        <RetirementAgeChart
          options={options}
          earliestAge={earliest?.age ?? null}
          currency={plan.currency}
        />
      </CardContent>
    </Card>
  );
}

/** What the optimized plan gains over the conventional strategy. */
function BaselineComparison({ result, currency }: { result: TaxOptimizedPlan; currency: string }) {
  const { t } = useTranslation();
  const { formatAmount } = useAmountFormatting();
  const baseline = result.baseline;
  if (!baseline) return null;
  const money = (value: number) => formatAmount(Math.round(value), currency);
  const spending = result.objective === "maxSpending";
  const optimizedTax =
    result.lifetimeFederalTax + result.lifetimeStateTax + result.lifetimeEarlyWithdrawalPenalty;
  const baselineTax =
    baseline.lifetimeFederalTax +
    baseline.lifetimeStateTax +
    baseline.lifetimeEarlyWithdrawalPenalty;
  const taxSaved = baselineTax - optimizedTax;
  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">{t("goals:tax_plan.baseline.title")}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-2 text-sm">
        <p className="text-muted-foreground">{t("goals:tax_plan.baseline.description")}</p>
        {baseline.status === "infeasible" ? (
          <p className="font-medium">{t("goals:tax_plan.baseline.infeasible")}</p>
        ) : (
          <div className="grid gap-3 sm:grid-cols-2">
            <div>
              <p className="text-muted-foreground text-xs">
                {spending
                  ? t("goals:tax_plan.baseline.extra_spending")
                  : t("goals:tax_plan.baseline.extra_estate")}
              </p>
              <p className="text-xl font-semibold tabular-nums">
                {spending
                  ? money(result.firstYearSpending - baseline.firstYearSpending)
                  : money(result.bequestAfterTax - baseline.bequestAfterTax)}
              </p>
              <p className="text-muted-foreground text-xs">
                {spending
                  ? t("goals:tax_plan.baseline.versus", {
                      amount: money(baseline.firstYearSpending),
                    })
                  : t("goals:tax_plan.baseline.versus", {
                      amount: money(baseline.bequestAfterTax),
                    })}
              </p>
            </div>
            <div>
              <p className="text-muted-foreground text-xs">
                {taxSaved >= 0
                  ? t("goals:tax_plan.baseline.tax_saved")
                  : t("goals:tax_plan.baseline.tax_extra")}
              </p>
              <p className="text-xl font-semibold tabular-nums">{money(Math.abs(taxSaved))}</p>
              <p className="text-muted-foreground text-xs">
                {t("goals:tax_plan.baseline.versus", { amount: money(baselineTax) })}
              </p>
              {taxSaved < 0 && (
                <p className="text-muted-foreground text-xs">
                  {spending
                    ? t("goals:tax_plan.baseline.tax_extra_spending_note")
                    : t("goals:tax_plan.baseline.tax_extra_estate_note")}
                </p>
              )}
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  );
}

function PlaybookTable({ years, currency }: { years: TaxOptimizedYear[]; currency: string }) {
  const { t } = useTranslation();
  const { formatAmount } = useAmountFormatting();
  const money = (value: number) =>
    Math.abs(value) < 0.5 ? "—" : formatAmount(Math.round(value), currency);
  const columns: [string, (y: TaxOptimizedYear) => number][] = [
    ["wages", (y) => y.wages],
    ["social_security", (y) => y.socialSecurity],
    ["taxable", (y) => y.taxableWithdrawal],
    ["tax_deferred", (y) => y.taxDeferredWithdrawal],
    ["roth", (y) => y.rothWithdrawal],
    ["roth_conversion", (y) => y.rothConversion],
    ["hsa", (y) => y.hsaWithdrawal],
    ["gain_harvest", (y) => y.gainHarvest],
    ["debt_payment", (y) => y.debtPayment],
    ["taxes", (y) => y.federalIncomeTax + y.capitalGainsTax + y.earlyWithdrawalPenalty],
    ["state_tax", (y) => y.stateIncomeTax],
    ["medicare", (y) => y.medicare + y.acaPremium],
    ["spending", (y) => y.netSpending],
    ["saved", (y) => y.taxableDeposit],
    ["leftover_spent", (y) => y.leftoverSpent ?? 0],
  ];
  return (
    <div className="overflow-x-auto">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>{t("goals:tax_plan.table.year")}</TableHead>
            <TableHead>{t("goals:tax_plan.table.age")}</TableHead>
            {columns.map(([key]) => (
              <TableHead key={key} className="text-right">
                {t(`goals:tax_plan.series.${key}`)}
              </TableHead>
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          {years.map((y) => (
            <TableRow key={y.year}>
              <TableCell className="tabular-nums">{y.year}</TableCell>
              <TableCell className="tabular-nums">
                {y.age}
                {y.working && (
                  <Badge variant="secondary" className="ml-2 text-[10px]">
                    {t("goals:tax_plan.table.working")}
                  </Badge>
                )}
              </TableCell>
              {columns.map(([key, value]) => (
                <TableCell key={key} className="text-right tabular-nums">
                  {money(value(y))}
                </TableCell>
              ))}
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}

/** When a result was solved, and whether the funding balances have moved since. */
function RunStatus({
  run,
  stale,
  currentBalanceTotal,
}: {
  run: SavedTaxPlanRun;
  stale: boolean;
  currentBalanceTotal?: number;
}) {
  const { t } = useTranslation();
  const { formatPercent } = useNumberFormatting();
  const change = balanceChange(run.result.startingAccounts, currentBalanceTotal);
  const when = new Date(run.computedAt).toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
  return (
    <div className="space-y-1 text-sm">
      <p className="text-muted-foreground">{t("goals:tax_plan.calculated_at", { when })}</p>
      {stale && <p className="text-muted-foreground">{t("goals:tax_plan.stale")}</p>}
      {change !== null && (
        <p className="flex items-center gap-1.5 text-amber-600 dark:text-amber-500">
          <Icons.AlertTriangle className="h-4 w-4 shrink-0" />
          {t(
            change > 0
              ? "goals:tax_plan.balances_changed_up"
              : "goals:tax_plan.balances_changed_down",
            { pct: formatPercent(Math.abs(change)) },
          )}
        </p>
      )}
    </div>
  );
}

/** The optimized plan: the age search, the summary, the charts and the playbook. */
function OverviewResult({
  plan,
  run,
  onUseAge,
}: {
  plan: RetirementPlan;
  run: SavedTaxPlanRun;
  onUseAge?: (age: number) => void;
}) {
  const { t } = useTranslation();
  const result = run.result;
  const currency = plan.currency;
  const searched = result.retirementAgeOptions.length > 0;
  return (
    <>
      {searched && <RetirementAgeCard result={result} plan={plan} onUseAge={onUseAge} />}

      {result.status === "infeasible" && (
        <Card className="border-destructive/30 bg-destructive/5">
          <CardContent className="space-y-2 py-4 text-sm">
            <p className="text-destructive font-medium">{t("goals:tax_plan.infeasible")}</p>
            {!searched && (
              <p className="text-muted-foreground">{t("goals:tax_plan.infeasible_hint")}</p>
            )}
            <pre className="text-muted-foreground whitespace-pre-wrap text-xs">
              {result.infeasibleReason}
            </pre>
          </CardContent>
        </Card>
      )}

      {result.status !== "infeasible" && (
        <>
          {(result.status === "timeLimit" || !result.converged) && (
            <p className="text-muted-foreground text-sm">{t("goals:tax_plan.approximate")}</p>
          )}
          <Summary result={result} currency={currency} />
          {result.home && <HomeCard home={result.home} currency={currency} />}
          <BaselineComparison result={result} currency={currency} />
          <div className="grid gap-4 lg:grid-cols-2">
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm">{t("goals:tax_plan.chart.sources")}</CardTitle>
              </CardHeader>
              <CardContent>
                <TaxPlanSourcesChart years={result.years} currency={currency} />
              </CardContent>
            </Card>
            <Card>
              <CardHeader className="pb-2">
                <CardTitle className="text-sm">{t("goals:tax_plan.chart.balances")}</CardTitle>
              </CardHeader>
              <CardContent>
                <TaxPlanBalancesChart years={result.years} currency={currency} />
              </CardContent>
            </Card>
          </div>
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">{t("goals:tax_plan.chart.conversions")}</CardTitle>
            </CardHeader>
            <CardContent>
              <TaxPlanConversionsChart years={result.years} currency={currency} />
            </CardContent>
          </Card>
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">{t("goals:tax_plan.contributions.title")}</CardTitle>
            </CardHeader>
            <CardContent>
              <TaxPlanContributionsTable
                years={result.years}
                caps={result.contributionCaps ?? []}
                currency={currency}
              />
            </CardContent>
          </Card>
          <Card>
            <CardHeader className="pb-2">
              <CardTitle className="text-sm">{t("goals:tax_plan.table.title")}</CardTitle>
            </CardHeader>
            <CardContent>
              <PlaybookTable years={result.years} currency={currency} />
            </CardContent>
          </Card>
        </>
      )}
    </>
  );
}

/**
 * How the optimized plan holds up: its chance of success, the spending it can
 * count on, what each choice costs, and whether this year's Roth conversion
 * holds up.
 */
function RiskResult({
  goalId,
  plan,
  run,
  onSuccess,
  onSafeSpending,
  onChoiceCosts,
  onStressTests,
  onConversionRegret,
  onTargetChange,
  onRiskSettingsChange,
  readOnly,
}: {
  goalId: string;
  plan: RetirementPlan;
  run: SavedTaxPlanRun;
  onSuccess?: (success: TaxPlanSuccess) => void;
  onSafeSpending?: (result: TaxPlanSafeSpending) => void;
  onChoiceCosts?: (result: TaxPlanChoiceCosts) => void;
  onStressTests?: (result: TaxPlanStressTests) => void;
  onConversionRegret?: (result: TaxPlanConversionRegret) => void;
  onTargetChange?: (target: number) => void;
  onRiskSettingsChange?: (patch: Partial<TaxOptimizerSettings>) => void;
  readOnly: boolean;
}) {
  const { t } = useTranslation();
  const result = run.result;
  if (result.status === "infeasible") {
    return (
      <Card className="border-destructive/30 bg-destructive/5">
        <CardContent className="text-muted-foreground py-4 text-sm">
          {t("goals:tax_plan.empty.risk_infeasible")}
        </CardContent>
      </Card>
    );
  }
  const retirementAge = result.retirementAgeOptions.length > 0 ? result.retirementAge : undefined;
  return (
    <>
      {!readOnly && onRiskSettingsChange && (
        <TaxPlanSimulationsCard plan={plan} onChange={onRiskSettingsChange} />
      )}
      <TaxPlanSuccessCard
        key={run.computedAt}
        goalId={goalId}
        plan={plan}
        retirementAge={retirementAge}
        saved={run.success}
        onResult={onSuccess}
        readOnly={readOnly}
      />
      <TaxPlanSafeSpendingCard
        key={`safe-${run.computedAt}`}
        goalId={goalId}
        plan={plan}
        retirementAge={retirementAge}
        saved={run.safeSpending}
        onResult={onSafeSpending}
        onTargetChange={onTargetChange}
        readOnly={readOnly}
      />
      <TaxPlanChoiceCostsCard
        key={`choices-${run.computedAt}`}
        goalId={goalId}
        plan={plan}
        retirementAge={retirementAge}
        saved={run.choiceCosts}
        onResult={onChoiceCosts}
        readOnly={readOnly}
      />
      <TaxPlanStressTestsCard
        key={`stress-${run.computedAt}`}
        goalId={goalId}
        plan={plan}
        retirementAge={retirementAge}
        saved={run.stressTests}
        onResult={onStressTests}
        readOnly={readOnly}
      />
      {result.lifetimeRothConversions > 0 && (
        <TaxPlanConversionRegretCard
          key={`regret-${run.computedAt}`}
          goalId={goalId}
          plan={plan}
          retirementAge={retirementAge}
          saved={run.conversionRegret}
          onResult={onConversionRegret}
          readOnly={readOnly}
        />
      )}
    </>
  );
}

/** Shown on a results tab before the plan has been optimized. */
function NotOptimizedYet({
  body,
  runButton,
  onOpenPlan,
}: {
  body: string;
  runButton: ReactNode;
  onOpenPlan: () => void;
}) {
  const { t } = useTranslation();
  return (
    <Card className="border-dashed">
      <CardContent className="flex flex-col items-center gap-4 py-12 text-center">
        <Icons.ReceiptText className="text-muted-foreground size-8" />
        <div className="max-w-lg space-y-1">
          <p className="font-medium">{t("goals:tax_plan.empty.title")}</p>
          <p className="text-muted-foreground text-sm">{body}</p>
        </div>
        <div className="flex flex-wrap justify-center gap-2">
          <Button variant="outline" onClick={onOpenPlan}>
            {t("goals:tax_plan.empty.open_plan")}
          </Button>
          {runButton}
        </div>
      </CardContent>
    </Card>
  );
}

/**
 * The tax-optimized style: set the plan up (Plan), optimize it, read the
 * result (Overview), test it (Risk) and compare saved cases (Compare). Results
 * appear only after a run; nothing here is projected from the FIRE or
 * Traditional model. Renders the tabs' contents inside the page's `Tabs`.
 */
export function TaxOptimizedTabs({
  goalId,
  plan,
  onSavePlan,
  store,
  onSaveStore,
  currentBalanceTotal,
  planInputs,
  onTabChange,
}: Props) {
  const { t } = useTranslation();
  const {
    savedRun,
    result,
    isStale,
    isRunning,
    error,
    run,
    saveSuccess,
    saveSafeSpending,
    saveChoiceCosts,
    saveStressTests,
    saveConversionRegret,
  } = useTaxOptimizedPlan(goalId, plan, store, onSaveStore);
  const [viewingId, setViewingId] = useState<string | null>(null);
  const viewing = store.cases.find((c) => c.id === viewingId) ?? null;
  const compareItems = comparableItems(
    store,
    plan,
    savedRun && !isStale ? savedRun : null,
    t("goals:tax_plan.cases.current"),
  );
  // The holdings' asset mix shown in the Markets settings.
  const { data: startingAccounts } = useQuery({
    queryKey: QueryKeys.retirementTaxAccounts(goalId),
    queryFn: () => getRetirementTaxPlanAccounts(goalId),
  });
  const searching = plan.taxOptimizer?.goal === "earliestAge";

  // Taking the found age keeps the search current: the target age does not change it.
  const takeAge = (age: number) => {
    const updated = {
      ...plan,
      personal: { ...plan.personal, targetRetirementAge: age },
    };
    if (savedRun && !isStale) {
      onSaveStore(
        { ...store, lastRun: { ...savedRun, inputsKey: planInputsKey(updated) } },
        updated,
      );
    } else {
      onSavePlan(updated);
    }
  };

  // A finished run opens its result.
  const optimize = () => {
    setViewingId(null);
    run(() => onTabChange("overview"));
  };
  const runButton = (
    <Button onClick={optimize} disabled={isRunning}>
      {isRunning ? (
        <Icons.Spinner className="mr-2 h-4 w-4 animate-spin" />
      ) : (
        <Icons.PlayCircle className="mr-2 h-4 w-4" />
      )}
      {isRunning
        ? searching
          ? t("goals:tax_plan.running_search")
          : t("goals:tax_plan.running")
        : result
          ? t("goals:tax_plan.rerun")
          : t("goals:tax_plan.run")}
    </Button>
  );
  const openPlan = () => onTabChange("plan");

  // The results tabs show the case being viewed, else the plan's last result.
  const shownRun = viewing ? (viewing.run ?? null) : savedRun;
  const shownPlan = viewing ? viewing.plan : plan;
  const resultHeader = viewing ? null : savedRun ? (
    <div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
      <RunStatus run={savedRun} stale={isStale} currentBalanceTotal={currentBalanceTotal} />
      {runButton}
    </div>
  ) : null;

  return (
    <div className="space-y-4">
      <TaxPlanCaseBar
        plan={plan}
        store={store}
        freshRun={savedRun && !isStale ? savedRun : null}
        viewingId={viewing?.id ?? null}
        onView={setViewingId}
        onSaveStore={onSaveStore}
        onCompare={compareItems.length >= 2 ? () => onTabChange("compare") : undefined}
      />

      {viewing && (
        <Card>
          <CardContent className="space-y-2 py-4">
            <p className="font-medium">{viewing.name}</p>
            <p className="text-muted-foreground text-sm">
              {t("goals:tax_plan.cases.viewing_description")}
            </p>
            {viewing.run ? (
              <RunStatus
                run={viewing.run}
                stale={false}
                currentBalanceTotal={currentBalanceTotal}
              />
            ) : (
              <p className="text-muted-foreground text-sm">{t("goals:tax_plan.cases.no_result")}</p>
            )}
          </CardContent>
        </Card>
      )}

      {error && (
        <Card className="border-destructive/30 bg-destructive/5">
          <CardContent className="text-destructive py-4 text-sm">
            {t("goals:tax_plan.failed", { message: String(error) })}
          </CardContent>
        </Card>
      )}

      <TabsContent value="plan" className="mt-0">
        <div className="grid grid-cols-1 gap-4 lg:grid-cols-3">
          <div className="space-y-4 lg:col-span-2">
            <Card>
              <CardContent className="flex flex-col gap-3 py-4 sm:flex-row sm:items-center sm:justify-between">
                <div className="space-y-1">
                  <p className="font-medium">{t("goals:tax_plan.title")}</p>
                  <p className="text-muted-foreground text-sm">{t("goals:tax_plan.description")}</p>
                  {savedRun && (
                    <RunStatus
                      run={savedRun}
                      stale={isStale}
                      currentBalanceTotal={currentBalanceTotal}
                    />
                  )}
                </div>
                {runButton}
              </CardContent>
            </Card>
            <TaxOptimizerSettingsCard
              plan={plan}
              startingAccounts={startingAccounts}
              onSavePlan={onSavePlan}
            />
          </div>
          <div className="space-y-4 lg:col-span-1 lg:self-start">{planInputs}</div>
        </div>
      </TabsContent>

      <TabsContent value="overview" className="mt-0 space-y-4">
        {resultHeader}
        {shownRun ? (
          <OverviewResult
            plan={shownPlan}
            run={shownRun}
            onUseAge={viewing ? undefined : takeAge}
          />
        ) : (
          !viewing && (
            <NotOptimizedYet
              body={t("goals:tax_plan.empty.overview")}
              runButton={runButton}
              onOpenPlan={openPlan}
            />
          )
        )}
      </TabsContent>

      <TabsContent value="risk" className="mt-0 space-y-4">
        {resultHeader}
        {shownRun ? (
          <RiskResult
            goalId={goalId}
            plan={shownPlan}
            run={shownRun}
            onSuccess={viewing || isStale ? undefined : saveSuccess}
            onSafeSpending={viewing || isStale ? undefined : saveSafeSpending}
            onChoiceCosts={viewing || isStale ? undefined : saveChoiceCosts}
            onStressTests={viewing || isStale ? undefined : saveStressTests}
            onConversionRegret={viewing || isStale ? undefined : saveConversionRegret}
            onTargetChange={
              viewing
                ? undefined
                : (successTarget) =>
                    onSavePlan({
                      ...plan,
                      taxOptimizer: { ...plan.taxOptimizer, successTarget },
                    })
            }
            onRiskSettingsChange={
              viewing
                ? undefined
                : (patch) =>
                    onSavePlan({
                      ...plan,
                      taxOptimizer: { ...plan.taxOptimizer, ...patch },
                    })
            }
            readOnly={Boolean(viewing)}
          />
        ) : (
          !viewing && (
            <NotOptimizedYet
              body={t("goals:tax_plan.empty.risk")}
              runButton={runButton}
              onOpenPlan={openPlan}
            />
          )
        )}
      </TabsContent>

      <TabsContent value="compare" className="mt-0">
        {compareItems.length >= 2 ? (
          <TaxPlanCompare items={compareItems} currency={plan.currency} />
        ) : (
          <Card className="border-dashed">
            <CardContent className="text-muted-foreground py-12 text-center text-sm">
              {t("goals:tax_plan.compare.empty")}
            </CardContent>
          </Card>
        )}
      </TabsContent>
    </div>
  );
}
