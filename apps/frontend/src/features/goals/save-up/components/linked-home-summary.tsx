import { useBalancePrivacy } from "@/hooks/use-balance-privacy";
import type { Goal, GoalPlan } from "@/lib/types";
import { AmountDisplay, Button, useDateFormatting } from "@wealthfolio/ui";
import { Card, CardContent, CardHeader, CardTitle } from "@wealthfolio/ui/components/ui/card";
import { Icons } from "@wealthfolio/ui/components/ui/icons";
import { useMemo } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router-dom";
import { useRetirementOverview } from "../../hooks/use-goal-detail";
import { parseSettingsJson } from "../../retirement-planner/lib/plan-adapter";
import { parseTaxPlanStore, planInputsKey } from "../../retirement-planner/lib/tax-plan-store";
import { homePurchaseDate } from "../lib/home-link";
import { homePurchaseCosts, parseHomePurchase } from "../lib/home-purchase";

interface Props {
  goal: Goal;
  plan: GoalPlan | null | undefined;
  retirementGoal: Goal;
  retirementPlan: GoalPlan;
}

/**
 * A home goal the retirement plan includes: what the purchase costs, and how
 * the retirement plan pays for it, in place of the savings progress.
 */
export function LinkedHomeSummary({ goal, plan, retirementGoal, retirementPlan }: Props) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { isBalanceHidden } = useBalancePrivacy();
  const { formatCalendarDate } = useDateFormatting();
  const currency = goal.currency ?? "USD";
  const optimized = retirementPlan.plannerMode === "optimized";
  const overview = useRetirementOverview(optimized ? undefined : retirementGoal.id);

  const purchase = parseHomePurchase(plan);
  const purchaseDate = homePurchaseDate(goal, plan, retirementPlan);
  const costs = purchase ? homePurchaseCosts(purchase, purchaseDate, new Date()) : null;
  const purchaseYear = purchaseDate ? Number(purchaseDate.slice(0, 4)) : null;
  const money = (value: number, className?: string) => (
    <AmountDisplay
      value={Math.round(value)}
      currency={currency}
      isHidden={isBalanceHidden}
      className={className}
    />
  );

  // Tax-optimized: the saved Tax Plan run's withdrawals in the purchase year.
  const taxPlan = useMemo(() => {
    if (!optimized) return null;
    const run = parseTaxPlanStore(retirementPlan.settingsJson).lastRun;
    if (!run) return { run: null, stale: false, year: null };
    // Parsed as the goal page parses it, so the key matches the page's.
    const settings = {
      ...parseSettingsJson(retirementPlan.settingsJson, new Date()),
      currency,
    };
    const stale = planInputsKey(settings) !== run.inputsKey;
    const home = run.result.home;
    const year = home ? run.result.years.find((row) => row.year === home.purchaseYear) : undefined;
    return { run, stale, year: year ?? null };
  }, [currency, optimized, retirementPlan]);

  // FIRE and Traditional: the projection's purchase year.
  const projectedYear =
    !optimized && purchaseYear
      ? overview.data?.trajectory.find((point) => point.year === purchaseYear)
      : undefined;

  const dateLabel = purchaseDate
    ? formatCalendarDate(
        {
          year: Number(purchaseDate.slice(0, 4)),
          month: Number(purchaseDate.slice(5, 7)) || 1,
          day: 1,
        },
        { year: "numeric", month: "short" },
      )
    : t("goals:linked_home.no_date");

  return (
    <Card className="overflow-hidden">
      <CardHeader className="pb-3">
        <div className="text-muted-foreground mb-1 text-xs font-medium uppercase tracking-[0.15em]">
          {t("goals:linked_home.kicker")}
        </div>
        <CardTitle className="font-serif text-2xl font-normal leading-tight tracking-tight">
          {t("goals:linked_home.headline", { retirement: retirementGoal.title })}
        </CardTitle>
        <p className="text-muted-foreground text-sm">
          {optimized ? t("goals:linked_home.how_optimized") : t("goals:linked_home.how_projection")}
        </p>
      </CardHeader>
      <CardContent className="space-y-5 p-0">
        <div className="border-border grid grid-cols-2 divide-x divide-y border-y md:grid-cols-4 md:divide-y-0">
          <Metric label={t("goals:linked_home.purchase")}>{dateLabel}</Metric>
          <Metric label={t("goals:linked_home.price_then")}>
            {costs ? money(costs.priceAtPurchase) : "—"}
          </Metric>
          <Metric label={t("goals:linked_home.cash_needed")}>
            {costs ? money(costs.savingsTarget) : "—"}
          </Metric>
          <Metric label={t("goals:linked_home.monthly_housing")}>
            {costs ? money(costs.monthlyPayment + costs.monthlyOwningCosts) : "—"}
          </Metric>
        </div>

        <div className="space-y-3 px-6 pb-6">
          {!purchase || !purchaseDate ? (
            <p className="text-muted-foreground text-sm">{t("goals:linked_home.incomplete")}</p>
          ) : optimized ? (
            !taxPlan?.run ? (
              <p className="text-muted-foreground text-sm">{t("goals:linked_home.run_tax_plan")}</p>
            ) : taxPlan.year ? (
              <div className="space-y-2">
                <p className="text-sm">
                  {t("goals:linked_home.tax_plan_year", { year: taxPlan.year.year })}
                </p>
                <dl className="bg-muted/40 grid grid-cols-2 gap-x-6 gap-y-2 rounded-md p-4 text-sm md:grid-cols-4">
                  {(
                    [
                      ["taxable", taxPlan.year.taxableWithdrawal],
                      ["tax_deferred", taxPlan.year.taxDeferredWithdrawal],
                      ["roth", taxPlan.year.rothWithdrawal],
                      ["hsa", taxPlan.year.hsaWithdrawal],
                    ] as const
                  ).map(([key, value]) => (
                    <div key={key}>
                      <dt className="text-muted-foreground text-xs">
                        {t(`goals:linked_home.bucket_${key}`)}
                      </dt>
                      <dd className="font-medium tabular-nums">{money(value)}</dd>
                    </div>
                  ))}
                </dl>
                <p className="text-muted-foreground text-xs">
                  {taxPlan.stale
                    ? t("goals:linked_home.tax_plan_stale")
                    : t("goals:linked_home.tax_plan_note")}
                </p>
              </div>
            ) : (
              <p className="text-muted-foreground text-sm">
                {t("goals:linked_home.tax_plan_missing")}
              </p>
            )
          ) : projectedYear ? (
            <div className="space-y-2">
              <p className="text-sm">
                {t("goals:linked_home.projection_year_prefix", { year: projectedYear.year })}{" "}
                {money(projectedYear.netWithdrawalFromPortfolio, "font-semibold")}{" "}
                {t("goals:linked_home.projection_year_suffix")}
              </p>
              <p className="text-muted-foreground text-xs">
                {t("goals:linked_home.projection_note")}
              </p>
            </div>
          ) : (
            <p className="text-muted-foreground text-sm">
              {overview.isLoading
                ? t("goals:linked_home.loading")
                : t("goals:linked_home.projection_missing")}
            </p>
          )}

          <Button
            variant="outline"
            size="sm"
            onClick={() => navigate(`/goals/${retirementGoal.id}`)}
          >
            <Icons.ArrowRight className="mr-1.5 h-3.5 w-3.5" />
            {t("goals:linked_home.open_retirement")}
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}

function Metric({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="min-w-0 px-5 py-4">
      <div className="text-muted-foreground text-[10px] font-semibold uppercase tracking-wider">
        {label}
      </div>
      <div className="mt-2 truncate text-lg font-semibold tabular-nums">{children}</div>
    </div>
  );
}
