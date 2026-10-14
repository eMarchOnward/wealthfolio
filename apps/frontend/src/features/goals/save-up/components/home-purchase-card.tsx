import { getGoalPlan } from "@/adapters";
import { QueryKeys } from "@/lib/query-keys";
import type { Goal, GoalPlan } from "@/lib/types";
import { useQuery } from "@tanstack/react-query";
import {
  Button,
  Input,
  Label,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  useAmountFormatting,
} from "@wealthfolio/ui";
import { Card, CardContent, CardHeader, CardTitle } from "@wealthfolio/ui/components/ui/card";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { useGoalPlanMutations } from "../../hooks/use-goal-detail";
import { useGoalMutations, useGoals } from "../../hooks/use-goals";
import type { HomePurchase } from "../../retirement-planner/types";
import {
  DEFAULT_HOME_PURCHASE,
  homePurchaseCosts,
  parseHomePurchase,
  purchaseDateAtAge,
  rawPlanSettings,
} from "../lib/home-purchase";

type Timing = "date" | "age";

/** A number input that commits on blur; percent fields show 6 for 0.06. */
function NumberField({
  id,
  label,
  hint,
  value,
  percent = false,
  onCommit,
}: {
  id: string;
  label: string;
  hint?: string;
  value: number | null | undefined;
  percent?: boolean;
  onCommit: (value: number | null) => void;
}) {
  const shown = value == null ? "" : String(percent ? Math.round(value * 10_000) / 100 : value);
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);
  const commit = () => {
    if (text.trim() === "") return onCommit(null);
    const parsed = Number(text);
    if (Number.isFinite(parsed) && parsed >= 0) onCommit(percent ? parsed / 100 : parsed);
    else setText(shown);
  };
  return (
    <div className="space-y-1">
      <Label htmlFor={id} className="text-xs">
        {label}
      </Label>
      <div className="relative">
        <Input
          id={id}
          inputMode="decimal"
          className={percent ? "pr-8" : undefined}
          value={text}
          onChange={(e) => setText(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => e.key === "Enter" && commit()}
        />
        {percent && (
          <span className="text-muted-foreground pointer-events-none absolute inset-y-0 right-3 flex items-center text-xs">
            %
          </span>
        )}
      </div>
      {hint && <p className="text-muted-foreground text-xs">{hint}</p>}
    </div>
  );
}

/**
 * The home a home goal saves for: its price, mortgage and owning costs. The
 * savings target is the down payment plus closing costs at the purchase date,
 * and the Tax Plan can include the purchase.
 */
export function HomePurchaseCard({
  goal,
  plan,
}: {
  goal: Goal;
  plan: GoalPlan | null | undefined;
}) {
  const { t } = useTranslation();
  const { formatAmount } = useAmountFormatting();
  const currency = goal.currency ?? "USD";
  const money = (value: number) => formatAmount(Math.round(value), currency);
  const { savePlanMutation } = useGoalPlanMutations(goal.id);
  const { updateMutation } = useGoalMutations();

  // The retirement goal's birth month turns a purchase age into a date.
  const { goals } = useGoals();
  const retirementGoal = goals.find(
    (g) => g.goalType === "retirement" && g.statusLifecycle !== "archived",
  );
  const { data: retirementPlan } = useQuery({
    queryKey: QueryKeys.goalPlan(retirementGoal?.id ?? ""),
    queryFn: () => getGoalPlan(retirementGoal!.id),
    enabled: !!retirementGoal,
  });
  const birthYearMonth = useMemo(() => {
    const personal = rawPlanSettings(retirementPlan).personal as
      | { birthYearMonth?: string }
      | undefined;
    return personal?.birthYearMonth;
  }, [retirementPlan]);

  const saved = parseHomePurchase(plan);
  const savedKey = JSON.stringify([saved, goal.targetDate]);
  const [draft, setDraft] = useState<HomePurchase>(saved ?? DEFAULT_HOME_PURCHASE);
  const [timing, setTiming] = useState<Timing>(saved?.purchaseAge != null ? "age" : "date");
  useEffect(() => {
    setDraft(saved ?? DEFAULT_HOME_PURCHASE);
    setTiming(saved?.purchaseAge != null ? "age" : "date");
    // Only when the saved purchase changes.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [savedKey]);
  const set = (patch: Partial<HomePurchase>) => setDraft((d) => ({ ...d, ...patch }));

  const purchaseDate =
    timing === "age"
      ? draft.purchaseAge != null && birthYearMonth
        ? purchaseDateAtAge(birthYearMonth, draft.purchaseAge)
        : null
      : (goal.targetDate ?? null);
  const costs = homePurchaseCosts(draft, purchaseDate, new Date());
  const dirty = JSON.stringify(draft) !== JSON.stringify(saved ?? DEFAULT_HOME_PURCHASE);
  const targetDiffers =
    costs != null && Math.abs(Math.round(costs.savingsTarget) - (goal.targetAmount ?? 0)) >= 1;
  const canSave = draft.price > 0 && costs != null && (dirty || targetDiffers);

  const save = () => {
    if (!costs || !purchaseDate) return;
    const purchase: HomePurchase = {
      ...draft,
      purchaseAge: timing === "age" ? draft.purchaseAge : null,
    };
    const targetAmount = Math.round(costs.savingsTarget);
    savePlanMutation.mutate({
      goalId: goal.id,
      planKind: "save_up",
      settingsJson: JSON.stringify({
        ...rawPlanSettings(plan),
        homePurchase: purchase,
        targetAmount,
        targetDate: purchaseDate,
      }),
    });
    updateMutation.mutate({ ...goal, targetAmount, targetDate: purchaseDate });
  };

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-md">{t("goals:home_purchase.title")}</CardTitle>
        <p className="text-muted-foreground text-sm">{t("goals:home_purchase.description")}</p>
      </CardHeader>
      <CardContent className="space-y-5">
        <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
          <NumberField
            id="home-price"
            label={t("goals:home_purchase.price")}
            hint={t("goals:home_purchase.price_hint")}
            value={draft.price || null}
            onCommit={(value) => set({ price: value ?? 0 })}
          />
          <NumberField
            id="home-down"
            label={t("goals:home_purchase.down_payment")}
            value={draft.downPaymentRate}
            percent
            onCommit={(value) => set({ downPaymentRate: Math.min(value ?? 0, 1) })}
          />
          <NumberField
            id="home-closing"
            label={t("goals:home_purchase.closing_costs")}
            value={draft.closingCostRate}
            percent
            onCommit={(value) => set({ closingCostRate: value ?? 0 })}
          />
          <NumberField
            id="home-rate"
            label={t("goals:home_purchase.mortgage_rate")}
            value={draft.mortgageRate}
            percent
            onCommit={(value) => set({ mortgageRate: value ?? 0 })}
          />
          <NumberField
            id="home-term"
            label={t("goals:home_purchase.term")}
            value={draft.termYears}
            onCommit={(value) => set({ termYears: Math.round(value ?? 30) })}
          />
          <NumberField
            id="home-appreciation"
            label={t("goals:home_purchase.appreciation")}
            hint={t("goals:home_purchase.appreciation_hint")}
            value={draft.appreciation}
            percent
            onCommit={(value) => set({ appreciation: value ?? 0 })}
          />
          <NumberField
            id="home-tax"
            label={t("goals:home_purchase.property_tax")}
            value={draft.propertyTaxRate}
            percent
            onCommit={(value) => set({ propertyTaxRate: value ?? 0 })}
          />
          <NumberField
            id="home-insurance"
            label={t("goals:home_purchase.insurance")}
            value={draft.insurance}
            onCommit={(value) => set({ insurance: value ?? 0 })}
          />
          <NumberField
            id="home-maintenance"
            label={t("goals:home_purchase.maintenance")}
            value={draft.maintenanceRate}
            percent
            onCommit={(value) => set({ maintenanceRate: value ?? 0 })}
          />
          <NumberField
            id="home-hoa"
            label={t("goals:home_purchase.hoa")}
            value={draft.hoaMonthly}
            onCommit={(value) => set({ hoaMonthly: value ?? 0 })}
          />
          <div className="space-y-1">
            <Label className="text-xs">{t("goals:home_purchase.when")}</Label>
            <Select value={timing} onValueChange={(value) => setTiming(value as Timing)}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="date">{t("goals:home_purchase.when_date")}</SelectItem>
                <SelectItem value="age" disabled={!birthYearMonth}>
                  {t("goals:home_purchase.when_age")}
                </SelectItem>
              </SelectContent>
            </Select>
            {!birthYearMonth && (
              <p className="text-muted-foreground text-xs">
                {t("goals:home_purchase.age_needs_birth")}
              </p>
            )}
          </div>
          {timing === "age" ? (
            <NumberField
              id="home-age"
              label={t("goals:home_purchase.age")}
              value={draft.purchaseAge}
              onCommit={(value) => set({ purchaseAge: value == null ? null : Math.round(value) })}
            />
          ) : (
            <p className="text-muted-foreground self-end text-xs">
              {goal.targetDate
                ? t("goals:home_purchase.date_from_goal", { date: goal.targetDate })
                : t("goals:home_purchase.date_missing")}
            </p>
          )}
        </div>

        {costs && draft.price > 0 && (
          <dl className="bg-muted/40 grid gap-x-6 gap-y-2 rounded-md p-4 text-sm sm:grid-cols-2 lg:grid-cols-3">
            {(
              [
                [t("goals:home_purchase.price_then"), money(costs.priceAtPurchase)],
                [
                  t("goals:home_purchase.savings_target"),
                  t("goals:home_purchase.savings_target_value", {
                    total: money(costs.savingsTarget),
                    down: money(costs.downPayment),
                    closing: money(costs.closingCosts),
                  }),
                ],
                [t("goals:home_purchase.loan"), money(costs.loan)],
                [t("goals:home_purchase.monthly_payment"), money(costs.monthlyPayment)],
                [t("goals:home_purchase.monthly_owning"), money(costs.monthlyOwningCosts)],
                [
                  t("goals:home_purchase.monthly_total"),
                  money(costs.monthlyPayment + costs.monthlyOwningCosts),
                ],
              ] as [string, string][]
            ).map(([label, value]) => (
              <div key={label}>
                <dt className="text-muted-foreground text-xs">{label}</dt>
                <dd className="font-medium tabular-nums">{value}</dd>
              </div>
            ))}
          </dl>
        )}

        <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
          <p className="text-muted-foreground text-xs">{t("goals:home_purchase.tax_plan_note")}</p>
          <Button
            size="sm"
            onClick={save}
            disabled={!canSave || savePlanMutation.isPending || updateMutation.isPending}
          >
            {t("goals:home_purchase.save")}
          </Button>
        </div>
      </CardContent>
    </Card>
  );
}
