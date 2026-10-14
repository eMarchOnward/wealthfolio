import {
  runRetirementTaxChoiceCosts,
  runRetirementTaxConversionRegret,
  runRetirementTaxStressTests,
} from "@/adapters";
import { useMutation } from "@tanstack/react-query";
import {
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
  useAmountFormatting,
} from "@wealthfolio/ui";
import { Icons } from "@wealthfolio/ui/components/ui/icons";
import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";
import type {
  ChoiceCost,
  ConversionOption,
  RetirementPlan,
  TaxPlanChoiceCosts,
  TaxPlanConversionRegret,
  TaxPlanStressTests,
  TaxSuccessMode,
} from "../types";
import { regretSimulationCount, SUCCESS_MODES } from "../lib/tax-plan-risk";

function RunButton({
  running,
  label,
  onClick,
}: {
  running: boolean;
  label: string;
  onClick: () => void;
}) {
  return (
    <Button variant="outline" size="sm" disabled={running} onClick={onClick}>
      {running && <Icons.Spinner className="mr-2 h-4 w-4 animate-spin" />}
      {label}
    </Button>
  );
}

/** A change in the plan's goal: spending a year, or the estate. */
function goalEffect(
  t: TFunction,
  spendingGoal: boolean,
  effect: number | null,
  money: (value: number) => string,
) {
  if (effect == null) return t("goals:tax_plan.choices.no_solution");
  if (Math.abs(effect) < 1) return t("goals:tax_plan.choices.no_change");
  const key = spendingGoal
    ? effect > 0
      ? "more_spending"
      : "less_spending"
    : effect > 0
      ? "more_estate"
      : "less_estate";
  return t(`goals:tax_plan.choices.${key}`, { amount: money(effect) });
}

/** Red for a loss in the goal (or no solution), else plain. */
function effectClass(effect: number | null) {
  return effect == null || effect < -1
    ? "text-destructive text-right tabular-nums"
    : "text-right tabular-nums";
}

/**
 * What each choice costs the plan's goal: the plan re-solved with one choice
 * nudged at a time (Owl explains the same with the optimizer's shadow prices).
 */
export function TaxPlanChoiceCostsCard({
  goalId,
  plan,
  retirementAge,
  saved,
  onResult,
  readOnly = false,
}: {
  goalId: string;
  plan: RetirementPlan;
  retirementAge?: number;
  saved?: TaxPlanChoiceCosts;
  onResult?: (result: TaxPlanChoiceCosts) => void;
  readOnly?: boolean;
}) {
  const { t } = useTranslation();
  const { formatAmount } = useAmountFormatting();
  const money = (value: number) => formatAmount(Math.round(Math.abs(value)), plan.currency);
  const mutation = useMutation({
    mutationFn: () => runRetirementTaxChoiceCosts(goalId, retirementAge),
    onSuccess: (data) => onResult?.(data),
  });
  const result = mutation.data ?? saved;
  const running = mutation.isPending;
  const spendingGoal = result?.objective === "maxSpending";

  const describe = (cost: ChoiceCost) =>
    t(`goals:tax_plan.choices.${cost.choice}`, {
      amount: money(cost.change),
      age: Math.floor(cost.change),
    });
  const effect = (cost: ChoiceCost) => goalEffect(t, spendingGoal, cost.effect, money);

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">{t("goals:tax_plan.choices.title")}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3 text-sm">
        <p className="text-muted-foreground">{t("goals:tax_plan.choices.description")}</p>
        {readOnly && !result && (
          <p className="text-muted-foreground">{t("goals:tax_plan.choices.not_run")}</p>
        )}
        {!readOnly && (
          <RunButton
            running={running}
            label={t("goals:tax_plan.choices.run")}
            onClick={() => mutation.mutate()}
          />
        )}
        {running && <p className="text-muted-foreground">{t("goals:tax_plan.choices.running")}</p>}
        {mutation.error && (
          <p className="text-destructive">
            {t("goals:tax_plan.failed", { message: String(mutation.error) })}
          </p>
        )}
        {result && !running && (
          <>
            <p className="text-muted-foreground">
              {spendingGoal
                ? t("goals:tax_plan.choices.base_spending", { amount: money(result.base) })
                : t("goals:tax_plan.choices.base_estate", { amount: money(result.base) })}
            </p>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t("goals:tax_plan.choices.choice")}</TableHead>
                  <TableHead className="text-right">{t("goals:tax_plan.choices.effect")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {result.costs.map((cost) => (
                  <TableRow key={cost.choice}>
                    <TableCell>{describe(cost)}</TableCell>
                    <TableCell
                      className={
                        cost.effect != null && cost.effect < -1
                          ? "text-destructive text-right tabular-nums"
                          : "text-right tabular-nums"
                      }
                    >
                      {effect(cost)}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
            <p className="text-muted-foreground text-xs">{t("goals:tax_plan.choices.note")}</p>
          </>
        )}
      </CardContent>
    </Card>
  );
}

/**
 * The plan re-optimized under shocks it does not control: weaker markets,
 * higher inflation and expenses, a Social Security cut, a longer life, and the
 * worst historical times to retire. Unlike a fixed simulation, the optimizer
 * adapts conversions, withdrawals and spending to each one.
 */
export function TaxPlanStressTestsCard({
  goalId,
  plan,
  retirementAge,
  saved,
  onResult,
  readOnly = false,
}: {
  goalId: string;
  plan: RetirementPlan;
  retirementAge?: number;
  saved?: TaxPlanStressTests;
  onResult?: (result: TaxPlanStressTests) => void;
  readOnly?: boolean;
}) {
  const { t } = useTranslation();
  const { formatAmount } = useAmountFormatting();
  const money = (value: number) => formatAmount(Math.round(Math.abs(value)), plan.currency);
  const mutation = useMutation({
    mutationFn: () => runRetirementTaxStressTests(goalId, retirementAge),
    onSuccess: (data) => onResult?.(data),
  });
  const result = mutation.data ?? saved;
  const running = mutation.isPending;
  const spendingGoal = result?.objective === "maxSpending";

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">{t("goals:tax_plan.stress.title")}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3 text-sm">
        <p className="text-muted-foreground">{t("goals:tax_plan.stress.description")}</p>
        {readOnly && !result && (
          <p className="text-muted-foreground">{t("goals:tax_plan.stress.not_run")}</p>
        )}
        {!readOnly && (
          <RunButton
            running={running}
            label={t("goals:tax_plan.stress.run")}
            onClick={() => mutation.mutate()}
          />
        )}
        {running && <p className="text-muted-foreground">{t("goals:tax_plan.stress.running")}</p>}
        {mutation.error && (
          <p className="text-destructive">
            {t("goals:tax_plan.failed", { message: String(mutation.error) })}
          </p>
        )}
        {result && !running && (
          <>
            <p className="text-muted-foreground">
              {spendingGoal
                ? t("goals:tax_plan.choices.base_spending", { amount: money(result.base) })
                : t("goals:tax_plan.choices.base_estate", { amount: money(result.base) })}
            </p>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t("goals:tax_plan.stress.stress")}</TableHead>
                  <TableHead className="text-right">{t("goals:tax_plan.choices.effect")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {result.stresses.map((stress) => (
                  <TableRow key={`${stress.stress}-${stress.year ?? ""}`}>
                    <TableCell>
                      {t(`goals:tax_plan.stress.${stress.stress}`, { year: stress.year })}
                    </TableCell>
                    <TableCell className={effectClass(stress.effect)}>
                      {goalEffect(t, spendingGoal, stress.effect, money)}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
            <p className="text-muted-foreground text-xs">{t("goals:tax_plan.stress.note")}</p>
          </>
        )}
      </CardContent>
    </Card>
  );
}

/**
 * Whether this year's recommended Roth conversion holds up if markets turn out
 * differently: each amount around it, re-solved on many market paths.
 */
export function TaxPlanConversionRegretCard({
  goalId,
  plan,
  retirementAge,
  saved,
  onResult,
  readOnly = false,
}: {
  goalId: string;
  plan: RetirementPlan;
  retirementAge?: number;
  saved?: TaxPlanConversionRegret;
  onResult?: (result: TaxPlanConversionRegret) => void;
  readOnly?: boolean;
}) {
  const { t } = useTranslation();
  const { formatAmount } = useAmountFormatting();
  const money = (value: number) => formatAmount(Math.round(value), plan.currency);
  const mutation = useMutation({
    mutationFn: (mode: TaxSuccessMode) =>
      runRetirementTaxConversionRegret(goalId, mode, retirementAge),
    onSuccess: (data) => onResult?.(data),
  });
  const result = mutation.data ?? saved;
  const running = mutation.isPending;
  const leastRegret = result
    ? result.options.reduce<ConversionOption | null>(
        (best, option) => (!best || option.meanRegret < best.meanRegret ? option : best),
        null,
      )
    : null;
  const pct = (share: number) => `${Math.round(share * 100)}%`;

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">{t("goals:tax_plan.regret.title")}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3 text-sm">
        <p className="text-muted-foreground">{t("goals:tax_plan.regret.description")}</p>
        {readOnly && !result && (
          <p className="text-muted-foreground">{t("goals:tax_plan.regret.not_run")}</p>
        )}
        {!readOnly && (
          <div className="flex flex-wrap gap-2">
            {SUCCESS_MODES.map((mode) => (
              <RunButton
                key={mode}
                running={running && mutation.variables === mode}
                label={t(`goals:tax_plan.regret.run_${mode}`, {
                  count: regretSimulationCount(plan),
                })}
                onClick={() => mutation.mutate(mode)}
              />
            ))}
          </div>
        )}
        {running && <p className="text-muted-foreground">{t("goals:tax_plan.regret.running")}</p>}
        {mutation.error && (
          <p className="text-destructive">
            {t("goals:tax_plan.failed", { message: String(mutation.error) })}
          </p>
        )}
        {result && leastRegret && !running && (
          <>
            <p>
              {t("goals:tax_plan.regret.summary", {
                recommended: money(result.recommended),
                best: money(leastRegret.amount),
                paths: result.paths,
              })}
            </p>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>{t("goals:tax_plan.regret.amount")}</TableHead>
                  <TableHead className="text-right">{t("goals:tax_plan.regret.mean")}</TableHead>
                  <TableHead className="text-right">{t("goals:tax_plan.regret.worst")}</TableHead>
                  <TableHead className="text-right">{t("goals:tax_plan.regret.best")}</TableHead>
                  <TableHead className="text-right">{t("goals:tax_plan.regret.funded")}</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {result.options.map((option) => (
                  <TableRow
                    key={option.amount}
                    className={option === leastRegret ? "bg-success/10" : undefined}
                  >
                    <TableCell className="tabular-nums">
                      {money(option.amount)}
                      {Math.abs(option.amount - Math.round(result.recommended)) < 1 && (
                        <span className="text-muted-foreground ml-2 text-xs">
                          {t("goals:tax_plan.regret.recommended")}
                        </span>
                      )}
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {money(option.meanRegret)}
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {money(option.worstRegret)}
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {pct(option.bestShare)}
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {pct(option.fundedShare)}
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
            <p className="text-muted-foreground text-xs">
              {t("goals:tax_plan.regret.note", { spending: money(result.netSpending) })}
            </p>
          </>
        )}
      </CardContent>
    </Card>
  );
}
