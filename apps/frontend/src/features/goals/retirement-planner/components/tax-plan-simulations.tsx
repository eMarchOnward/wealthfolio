import {
  Card,
  CardContent,
  Input,
  Label,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Switch,
} from "@wealthfolio/ui";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  MAX_SIMULATIONS,
  MIN_SIMULATIONS,
  regretSimulationCount,
  simulationCount,
} from "../lib/tax-plan-risk";
import type { MortalityTable, RetirementPlan, Sex, TaxOptimizerSettings } from "../types";

/** Owl's mortality tables, shortest-lived first at 65 after the general population. */
const MORTALITY_TABLES: readonly MortalityTable[] = [
  "SSA2025",
  "VBT2015-SM",
  "Pub2010-Safety",
  "Pub2010-General",
  "RP2014",
  "VBT2015-NS",
  "IAM2012",
  "Pub2010-Teacher",
];
const SEXES: readonly Sex[] = ["Female", "Male"];

/**
 * Settings of the Monte Carlo risk checks: how many paths they draw (each is a
 * full re-solve of the plan, so this sets how long they take) and whether
 * each run draws its own lifespan.
 */
export function TaxPlanSimulationsCard({
  plan,
  onChange,
}: {
  plan: RetirementPlan;
  onChange: (patch: Partial<TaxOptimizerSettings>) => void;
}) {
  const { t } = useTranslation();
  const lifespans = plan.taxOptimizer?.randomLifespans ?? null;
  // Turned on but no sex picked yet: nothing to save until there is one.
  const [choosing, setChoosing] = useState(false);
  const on = lifespans != null || choosing;

  return (
    <Card>
      <CardContent className="space-y-4 py-4 text-sm">
        <div className="space-y-1">
          <p className="font-medium">{t("goals:tax_plan.simulations.title")}</p>
          <p className="text-muted-foreground">
            {t("goals:tax_plan.simulations.description", {
              min: MIN_SIMULATIONS,
              max: MAX_SIMULATIONS.toLocaleString(),
            })}
          </p>
        </div>
        <div className="flex flex-wrap gap-4">
          <CountField
            id="tax-simulation-count"
            label={t("goals:tax_plan.simulations.label")}
            count={simulationCount(plan)}
            onChange={(simulationCount) => onChange({ simulationCount })}
          />
          <CountField
            id="tax-regret-simulation-count"
            label={t("goals:tax_plan.simulations.regret_label")}
            count={regretSimulationCount(plan)}
            onChange={(regretSimulationCount) => onChange({ regretSimulationCount })}
          />
        </div>
        <div className="space-y-3 border-t pt-4">
          <div className="flex items-center justify-between gap-3">
            <Label htmlFor="tax-random-lifespans" className="font-medium">
              {t("goals:tax_plan.simulations.lifespans")}
            </Label>
            <Switch
              id="tax-random-lifespans"
              checked={on}
              onCheckedChange={(checked) => {
                setChoosing(checked);
                if (!checked && lifespans) onChange({ randomLifespans: null });
              }}
            />
          </div>
          <p className="text-muted-foreground">
            {t("goals:tax_plan.simulations.lifespans_description", {
              age: plan.personal.targetRetirementAge,
            })}
          </p>
          {on && (
            <div className="flex flex-wrap gap-4">
              <div className="space-y-1">
                <Label className="text-xs">{t("goals:tax_plan.simulations.sex")}</Label>
                <Select
                  value={lifespans?.sex ?? ""}
                  onValueChange={(value) => {
                    setChoosing(false);
                    onChange({
                      randomLifespans: { sex: value as Sex, table: lifespans?.table ?? "SSA2025" },
                    });
                  }}
                >
                  <SelectTrigger className="w-36">
                    <SelectValue placeholder={t("goals:tax_plan.simulations.sex_placeholder")} />
                  </SelectTrigger>
                  <SelectContent>
                    {SEXES.map((sex) => (
                      <SelectItem key={sex} value={sex}>
                        {t(`goals:tax_plan.simulations.sex_${sex}`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
              <div className="min-w-0 flex-1 space-y-1">
                <Label className="text-xs">{t("goals:tax_plan.simulations.table")}</Label>
                <Select
                  value={lifespans?.table ?? "SSA2025"}
                  disabled={!lifespans}
                  onValueChange={(value) =>
                    lifespans &&
                    onChange({
                      randomLifespans: { ...lifespans, table: value as MortalityTable },
                    })
                  }
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    {MORTALITY_TABLES.map((table) => (
                      <SelectItem key={table} value={table}>
                        {t(`goals:tax_plan.simulations.tables.${table}`)}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              </div>
            </div>
          )}
        </div>
      </CardContent>
    </Card>
  );
}

/** A path count, saved on blur or Enter when it is in range. */
function CountField({
  id,
  label,
  count,
  onChange,
}: {
  id: string;
  label: string;
  count: number;
  onChange: (count: number) => void;
}) {
  const [text, setText] = useState(String(count));
  useEffect(() => setText(String(count)), [count]);
  const commit = () => {
    const value = Math.round(Number(text));
    if (Number.isFinite(value) && value >= MIN_SIMULATIONS && value <= MAX_SIMULATIONS) {
      if (value !== count) onChange(value);
    } else {
      setText(String(count));
    }
  };

  return (
    <div className="space-y-1">
      <Label htmlFor={id} className="text-xs">
        {label}
      </Label>
      <Input
        id={id}
        inputMode="numeric"
        className="w-28"
        value={text}
        onChange={(e) => setText(e.target.value)}
        onBlur={commit}
        onKeyDown={(e) => e.key === "Enter" && commit()}
      />
    </div>
  );
}
