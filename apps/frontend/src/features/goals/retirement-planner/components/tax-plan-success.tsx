import { runRetirementTaxSuccess } from "@/adapters";
import { useMutation } from "@tanstack/react-query";
import {
  Button,
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  useAmountFormatting,
} from "@wealthfolio/ui";
import { Icons } from "@wealthfolio/ui/components/ui/icons";
import { useTranslation } from "react-i18next";
import type { RetirementPlan, TaxPlanSuccess, TaxSuccessMode } from "../types";
import { simulationCount, SUCCESS_MODES } from "../lib/tax-plan-risk";

/**
 * Chance the planned expenses are funded across historical or resampled paths.
 * With `retirementAge` (an earliest-age search) it tests retiring at that age
 * on the must-have expenses. `saved` is the last check kept with the result;
 * `readOnly` shows it without offering a new check (a saved case).
 */
export function TaxPlanSuccessCard({
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
  saved?: TaxPlanSuccess;
  onResult?: (result: TaxPlanSuccess) => void;
  readOnly?: boolean;
}) {
  const { t } = useTranslation();
  const { formatAmount } = useAmountFormatting();
  const mutation = useMutation({
    mutationFn: (mode: TaxSuccessMode) => runRetirementTaxSuccess(goalId, mode, retirementAge),
    onSuccess: (data) => onResult?.(data),
  });
  const result: TaxPlanSuccess | undefined = mutation.data ?? saved;
  const money = (value: number) => formatAmount(Math.round(value), plan.currency);
  const running = mutation.isPending;

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">{t("goals:tax_plan.success.title")}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3 text-sm">
        <p className="text-muted-foreground">
          {retirementAge == null
            ? t("goals:tax_plan.success.description")
            : t("goals:tax_plan.success.description_required", { age: retirementAge })}
        </p>
        {readOnly && !result && (
          <p className="text-muted-foreground">{t("goals:tax_plan.success.not_run")}</p>
        )}
        {!readOnly && (
          <div className="flex flex-wrap gap-2">
            {SUCCESS_MODES.map((mode) => (
              <Button
                key={mode}
                variant="outline"
                size="sm"
                disabled={running}
                onClick={() => mutation.mutate(mode)}
              >
                {running && mutation.variables === mode && (
                  <Icons.Spinner className="mr-2 h-4 w-4 animate-spin" />
                )}
                {t(`goals:tax_plan.success.run_${mode}`, { count: simulationCount(plan) })}
              </Button>
            ))}
          </div>
        )}
        {running && <p className="text-muted-foreground">{t("goals:tax_plan.success.running")}</p>}
        {mutation.error && (
          <p className="text-destructive">
            {t("goals:tax_plan.failed", { message: String(mutation.error) })}
          </p>
        )}
        {result && !running && (
          <div className="space-y-2">
            <p className="text-2xl font-semibold tabular-nums">
              {Math.round(result.successRate * 100)}%
            </p>
            <p className="text-muted-foreground">
              {t("goals:tax_plan.success.summary", {
                funded: result.funded,
                paths: result.paths,
                spending: money(result.netSpending),
              })}
            </p>
            <p className="text-muted-foreground">
              {t("goals:tax_plan.success.estates", {
                p10: money(result.bequestPercentiles[0]),
                p50: money(result.bequestPercentiles[1]),
                p90: money(result.bequestPercentiles[2]),
              })}
            </p>
            {result.lifespans && (
              <p className="text-muted-foreground">
                {t("goals:tax_plan.success.lifespans", {
                  p10: result.lifespans[0],
                  p50: result.lifespans[1],
                  p90: result.lifespans[2],
                })}
              </p>
            )}
            {result.mode === "historical" && result.failedPaths.length > 0 && (
              <p className="text-muted-foreground">
                {t("goals:tax_plan.success.failed_years", {
                  years: result.failedPaths.join(", "),
                })}
              </p>
            )}
          </div>
        )}
      </CardContent>
    </Card>
  );
}
