import {
  Button,
  Card,
  CardContent,
  CardHeader,
  CardTitle,
  Input,
  Label,
  Popover,
  PopoverContent,
  PopoverTrigger,
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
  Switch,
  useAmountFormatting,
} from "@wealthfolio/ui";
import { Icons } from "@wealthfolio/ui/components/ui/icons";
import { useEffect, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type {
  AssetMix,
  BucketMixes,
  GlideShape,
  InflationMethod,
  MarketSettings,
  RetirementPlan,
  ReturnMethod,
  TaxOptimizerAccounts,
  TaxOptimizerDebt,
  TaxOptimizerObjective,
  TaxOptimizerSettings,
  TaxPlanGoal,
} from "../types";
import { activeExpenseItems } from "../lib/expense-items";

const NO_STREAM = "__none__";

/** What a number field holds, shown inside the field. */
type Unit = "perYear" | "perMonth" | "total" | "percent" | "age" | "year";

const UNIT_KEYS: Record<Unit, string> = {
  perYear: "goals:tax_plan.units.per_year",
  perMonth: "goals:tax_plan.units.per_month",
  total: "goals:tax_plan.units.total",
  percent: "goals:tax_plan.units.percent",
  age: "goals:tax_plan.units.age",
  year: "goals:tax_plan.units.year",
};

/**
 * Explains a setting in a pop-up: what it is, why it matters, and the kind of
 * value expected. Texts live under `goals:tax_plan.info.<topic>`; paragraphs
 * are separated by blank lines.
 */
function InfoTip({ topic, label }: { topic: string; label: string }) {
  const { t } = useTranslation();
  const paragraphs = t(`goals:tax_plan.info.${topic}.body`).split("\n\n");
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          type="button"
          className="text-muted-foreground hover:text-foreground inline-flex shrink-0"
          aria-label={t("goals:tax_plan.info.aria", { setting: label })}
        >
          <Icons.Info className="h-3.5 w-3.5" />
        </button>
      </PopoverTrigger>
      <PopoverContent align="start" className="w-80 space-y-2 text-xs leading-relaxed">
        <p className="text-sm font-medium">{label}</p>
        {paragraphs.map((paragraph, index) => (
          <p key={index}>{paragraph}</p>
        ))}
        <p className="bg-muted rounded px-2 py-1">
          <span className="font-medium">{t("goals:tax_plan.info.expected")}</span>{" "}
          {t(`goals:tax_plan.info.${topic}.value`)}
        </p>
      </PopoverContent>
    </Popover>
  );
}

function FieldLabel({ htmlFor, label, info }: { htmlFor?: string; label: string; info: string }) {
  return (
    <div className="flex items-center gap-1.5">
      <Label htmlFor={htmlFor} className="text-xs">
        {label}
      </Label>
      <InfoTip topic={info} label={label} />
    </div>
  );
}

/** A number field that commits on blur, so typing does not save every keystroke. */
function AmountField({
  id,
  label,
  info,
  unit,
  note,
  value,
  onCommit,
  optional = false,
}: {
  id: string;
  label: string;
  info: string;
  unit: Unit;
  note?: string;
  value: number | null | undefined;
  onCommit: (value: number | null) => void;
  optional?: boolean;
}) {
  const { t } = useTranslation();
  const percent = unit === "percent";
  const shown = value == null ? "" : String(percent ? Math.round(value * 10_000) / 100 : value);
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);
  const commit = () => {
    if (text.trim() === "") {
      onCommit(optional ? null : 0);
      return;
    }
    const parsed = Number(text);
    if (Number.isFinite(parsed) && parsed >= 0) onCommit(percent ? parsed / 100 : parsed);
    else setText(shown);
  };
  return (
    <div className="space-y-1">
      <FieldLabel htmlFor={id} label={label} info={info} />
      <div className="relative">
        <Input
          id={id}
          inputMode="decimal"
          className="pr-20"
          value={text}
          onChange={(e) => setText(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => e.key === "Enter" && commit()}
        />
        <span className="text-muted-foreground pointer-events-none absolute inset-y-0 right-3 flex items-center text-xs">
          {t(UNIT_KEYS[unit])}
        </span>
      </div>
      {note && <p className="text-muted-foreground text-xs">{note}</p>}
    </div>
  );
}

function SwitchField({
  id,
  label,
  info,
  checked,
  disabled = false,
  note,
  onChange,
}: {
  id: string;
  label: string;
  info: string;
  checked: boolean;
  disabled?: boolean;
  note?: string;
  onChange: (checked: boolean) => void;
}) {
  return (
    <div className="space-y-1 rounded-md border p-3">
      <div className="flex items-center justify-between gap-3">
        <FieldLabel htmlFor={id} label={label} info={info} />
        <Switch id={id} checked={checked} disabled={disabled} onCheckedChange={onChange} />
      </div>
      {note && <p className="text-muted-foreground text-xs">{note}</p>}
    </div>
  );
}

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="space-y-3">
      <h3 className="text-muted-foreground text-xs font-semibold uppercase tracking-wide">
        {title}
      </h3>
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">{children}</div>
    </section>
  );
}

/** Loans paid on a schedule; extra principal is left to the optimizer. */
function DebtEditor({
  debts,
  onChange,
}: {
  debts: TaxOptimizerDebt[];
  onChange: (debts: TaxOptimizerDebt[]) => void;
}) {
  const { t } = useTranslation();
  const set = (index: number, patch: Partial<TaxOptimizerDebt>) =>
    onChange(debts.map((debt, i) => (i === index ? { ...debt, ...patch } : debt)));
  return (
    <div className="space-y-2 sm:col-span-2 lg:col-span-3">
      <div className="flex items-center justify-between">
        <FieldLabel label={t("goals:tax_plan.settings.debts")} info="debts" />
        <Button
          variant="outline"
          size="sm"
          onClick={() =>
            onChange([...debts, { label: "", balance: 0, annualRate: 0, annualPayment: 0 }])
          }
        >
          {t("goals:tax_plan.settings.add_debt")}
        </Button>
      </div>
      {debts.map((debt, index) => (
        <div key={index} className="grid grid-cols-2 items-end gap-2 sm:grid-cols-5">
          <div className="col-span-2 space-y-1 sm:col-span-1">
            <Label className="text-xs">{t("goals:tax_plan.settings.debt_label")}</Label>
            <Input value={debt.label} onChange={(e) => set(index, { label: e.target.value })} />
          </div>
          <AmountField
            id={`tax-debt-balance-${index}`}
            label={t("goals:tax_plan.settings.debt_balance")}
            info="debt_balance"
            unit="total"
            value={debt.balance}
            onCommit={(value) => set(index, { balance: value ?? 0 })}
          />
          <AmountField
            id={`tax-debt-rate-${index}`}
            label={t("goals:tax_plan.settings.debt_rate")}
            info="debt_rate"
            unit="percent"
            value={debt.annualRate}
            onCommit={(value) => set(index, { annualRate: value ?? 0 })}
          />
          <AmountField
            id={`tax-debt-payment-${index}`}
            label={t("goals:tax_plan.settings.debt_payment")}
            info="debt_payment"
            unit="perYear"
            value={debt.annualPayment}
            onCommit={(value) => set(index, { annualPayment: value ?? 0 })}
          />
          <Button
            variant="ghost"
            size="sm"
            onClick={() => onChange(debts.filter((_, i) => i !== index))}
          >
            {t("goals:tax_plan.settings.remove_debt")}
          </Button>
        </div>
      ))}
    </div>
  );
}

/** Selling the current home when the linked home is bought (tax-optimized plan). */
export function HomeSaleFields({
  settings,
  update,
}: {
  settings: TaxOptimizerSettings;
  update: (patch: Partial<TaxOptimizerSettings>) => void;
}) {
  const { t } = useTranslation();
  const debts = (settings.debts ?? []).filter((debt) => debt.label.trim() !== "");
  const selling = settings.sellCurrentHome ?? false;
  return (
    <>
      <SwitchField
        id="tax-sell-home"
        label={t("goals:tax_plan.settings.sell_current_home")}
        info="sell_current_home"
        checked={selling}
        onChange={(checked) => update({ sellCurrentHome: checked })}
      />
      {selling && (
        <>
          <AmountField
            id="tax-current-home-value"
            label={t("goals:tax_plan.settings.current_home_value")}
            info="current_home_value"
            unit="total"
            value={settings.currentHomeValue}
            onCommit={(value) => update({ currentHomeValue: value ?? 0 })}
          />
          <AmountField
            id="tax-selling-cost-rate"
            label={t("goals:tax_plan.settings.selling_cost_rate")}
            info="selling_cost_rate"
            unit="percent"
            value={settings.sellingCostRate ?? 0.06}
            onCommit={(value) => update({ sellingCostRate: Math.min(value ?? 0.06, 1) })}
          />
          <div className="space-y-1">
            <FieldLabel
              label={t("goals:tax_plan.settings.current_home_debt")}
              info="current_home_debt"
            />
            <Select
              value={
                debts.some((debt) => debt.label === settings.currentHomeDebt)
                  ? (settings.currentHomeDebt ?? NO_STREAM)
                  : NO_STREAM
              }
              onValueChange={(value) =>
                update({ currentHomeDebt: value === NO_STREAM ? null : value })
              }
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NO_STREAM}>
                  {t("goals:tax_plan.settings.current_home_debt_none")}
                </SelectItem>
                {debts.map((debt) => (
                  <SelectItem key={debt.label} value={debt.label}>
                    {debt.label}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <p className="text-muted-foreground text-xs">
            {t("goals:tax_plan.settings.sell_current_home_items")}
          </p>
        </>
      )}
    </>
  );
}

/** Owl's fixed-rate presets: stocks, Baa bonds, T-notes and inflation. */
const MARKET_PRESETS = {
  conservative: { fixed: { stocks: 0.06, bonds: 0.04, notes: 0.033 }, inflation: 0.028 },
  optimistic: { fixed: { stocks: 0.086, bonds: 0.049, notes: 0.033 }, inflation: 0.025 },
} as const;
/** Years of the embedded market history. */
const HISTORY_FIRST_YEAR = 1928;
const HISTORY_LAST_YEAR = 2025;
const TRAILING_YEARS = 30;
const BUCKETS = ["taxable", "taxDeferred", "roth", "hsa"] as const;
const ASSET_CLASSES = ["stocks", "bonds", "notes", "cash"] as const;
const DEFAULT_TARGET_MIX: AssetMix = { stocks: 0.4, bonds: 0.3, notes: 0.3, cash: 0 };

function mixTotal(mix: AssetMix) {
  return ASSET_CLASSES.reduce((sum, key) => sum + Math.max(0, mix[key]), 0);
}

/** The mix scaled to 100%, as the planner uses it. */
function scaledMix(mix: AssetMix): AssetMix {
  const total = mixTotal(mix);
  if (total <= 0) return mix;
  return {
    stocks: Math.max(0, mix.stocks) / total,
    bonds: Math.max(0, mix.bonds) / total,
    notes: Math.max(0, mix.notes) / total,
    cash: Math.max(0, mix.cash) / total,
  };
}

function allBuckets(mix: AssetMix): BucketMixes {
  return { taxable: mix, taxDeferred: mix, roth: mix, hsa: mix };
}

/** One weight of a mix, in percent; read-only without `onCommit`. */
function PercentCell({
  label,
  value,
  onCommit,
}: {
  label: string;
  value: number;
  onCommit?: (value: number) => void;
}) {
  const shown = String(Math.round(value * 1000) / 10);
  const [text, setText] = useState(shown);
  useEffect(() => setText(shown), [shown]);
  if (!onCommit) return <span className="tabular-nums">{shown}%</span>;
  const commit = () => {
    const parsed = Number(text);
    if (text.trim() !== "" && Number.isFinite(parsed) && parsed >= 0) onCommit(parsed / 100);
    else setText(shown);
  };
  return (
    <Input
      aria-label={label}
      inputMode="decimal"
      className="h-8 w-20 text-right tabular-nums"
      value={text}
      onChange={(e) => setText(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => e.key === "Enter" && commit()}
    />
  );
}

/**
 * Owl's market options: where yearly returns and inflation come from, and how
 * each account bucket is invested (from the holdings, or entered), optionally
 * gliding to a target mix.
 */
function MarketsSection({
  plan,
  settings,
  startingAccounts,
  update,
}: {
  plan: RetirementPlan;
  settings: TaxOptimizerSettings;
  startingAccounts?: TaxOptimizerAccounts;
  update: (patch: Partial<TaxOptimizerSettings>) => void;
}) {
  const { t } = useTranslation();
  const { formatAmount } = useAmountFormatting();
  const markets: MarketSettings = settings.markets ?? {};
  const setMarkets = (patch: Partial<MarketSettings>) =>
    update({ markets: { ...markets, ...patch } });
  const method: ReturnMethod = markets.method ?? "plan";
  const historical = method === "historical" || method === "historicalAverage";
  const inflationMethod: InflationMethod = historical
    ? (markets.inflationMethod ?? "fixed")
    : "fixed";
  const percent = (value: number) => `${Math.round(value * 1000) / 10}%`;
  const investment = plan.investment;

  const equityShare = Math.min(Math.max(settings.equityShare ?? 0.6, 0), 1);
  const fallback: AssetMix = { stocks: equityShare, bonds: 1 - equityShare, notes: 0, cash: 0 };
  const derived = startingAccounts?.mixes ?? null;
  const manual = markets.startingMix ?? null;
  const starting: BucketMixes = manual ?? derived ?? allBuckets(fallback);
  const target = markets.targetMix ?? null;
  const unclassified = startingAccounts?.unclassified ?? 0;
  const rows = [...BUCKETS.map((bucket) => starting[bucket]), ...(target ? [target] : [])];
  const unbalanced = rows.some((mix) => Math.abs(mixTotal(mix) - 1) > 0.005);

  const setStartingWeight = (
    bucket: (typeof BUCKETS)[number],
    key: keyof AssetMix,
    value: number,
  ) =>
    setMarkets({ startingMix: { ...starting, [bucket]: { ...starting[bucket], [key]: value } } });

  return (
    <Section title={t("goals:tax_plan.settings.section.markets")}>
      <div className="space-y-1">
        <FieldLabel label={t("goals:tax_plan.settings.markets.method")} info="markets_method" />
        <Select
          value={method}
          onValueChange={(value) => setMarkets({ method: value as ReturnMethod })}
        >
          <SelectTrigger>
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value="plan">
              {t("goals:tax_plan.settings.markets.method_plan", {
                working: percent(investment.preRetirementAnnualReturn),
                retired: percent(investment.retirementAnnualReturn),
              })}
            </SelectItem>
            <SelectItem value="fixed">
              {t("goals:tax_plan.settings.markets.method_fixed")}
            </SelectItem>
            <SelectItem value="historical">
              {t("goals:tax_plan.settings.markets.method_historical")}
            </SelectItem>
            <SelectItem value="historicalAverage">
              {t("goals:tax_plan.settings.markets.method_average")}
            </SelectItem>
          </SelectContent>
        </Select>
      </div>

      {method === "fixed" && (
        <>
          {(["stocks", "bonds", "notes"] as const).map((key) => (
            <AmountField
              key={key}
              id={`tax-market-${key}`}
              label={t(`goals:tax_plan.settings.markets.return_${key}`)}
              info="fixed_returns"
              unit="percent"
              value={(markets.fixed ?? MARKET_PRESETS.conservative.fixed)[key]}
              onCommit={(value) =>
                setMarkets({
                  fixed: {
                    ...(markets.fixed ?? MARKET_PRESETS.conservative.fixed),
                    [key]: value ?? 0,
                  },
                })
              }
            />
          ))}
          <div className="flex flex-wrap items-center gap-2 sm:col-span-2 lg:col-span-3">
            <span className="text-muted-foreground text-xs">
              {t("goals:tax_plan.settings.markets.presets")}
            </span>
            {(["conservative", "optimistic"] as const).map((name) => (
              <Button
                key={name}
                variant="outline"
                size="sm"
                onClick={() =>
                  setMarkets({
                    fixed: { ...MARKET_PRESETS[name].fixed },
                    inflation: MARKET_PRESETS[name].inflation,
                  })
                }
              >
                {t(`goals:tax_plan.settings.markets.preset_${name}`)}
              </Button>
            ))}
            <Button
              variant="outline"
              size="sm"
              onClick={() =>
                setMarkets({
                  method: "historicalAverage",
                  averageFrom: HISTORY_LAST_YEAR - TRAILING_YEARS + 1,
                  averageTo: HISTORY_LAST_YEAR,
                })
              }
            >
              {t("goals:tax_plan.settings.markets.preset_trailing", { years: TRAILING_YEARS })}
            </Button>
          </div>
        </>
      )}

      {method === "historical" && (
        <AmountField
          id="tax-market-start-year"
          label={t("goals:tax_plan.settings.markets.start_year")}
          info="start_year"
          unit="year"
          value={markets.startYear ?? 1969}
          onCommit={(value) =>
            setMarkets({
              startYear: Math.min(
                Math.max(Math.round(value ?? 1969), HISTORY_FIRST_YEAR),
                HISTORY_LAST_YEAR,
              ),
            })
          }
        />
      )}

      {method === "historicalAverage" && (
        <>
          <AmountField
            id="tax-market-average-from"
            label={t("goals:tax_plan.settings.markets.average_from")}
            info="average_range"
            unit="year"
            value={markets.averageFrom ?? HISTORY_FIRST_YEAR}
            onCommit={(value) =>
              setMarkets({
                averageFrom: Math.min(
                  Math.max(Math.round(value ?? HISTORY_FIRST_YEAR), HISTORY_FIRST_YEAR),
                  HISTORY_LAST_YEAR - 1,
                ),
              })
            }
          />
          <AmountField
            id="tax-market-average-to"
            label={t("goals:tax_plan.settings.markets.average_to")}
            info="average_range"
            unit="year"
            value={markets.averageTo ?? HISTORY_LAST_YEAR}
            onCommit={(value) =>
              setMarkets({
                averageTo: Math.min(
                  Math.max(Math.round(value ?? HISTORY_LAST_YEAR), HISTORY_FIRST_YEAR + 1),
                  HISTORY_LAST_YEAR,
                ),
              })
            }
          />
        </>
      )}

      {historical && (
        <div className="space-y-1">
          <FieldLabel
            label={t("goals:tax_plan.settings.markets.inflation_method")}
            info="inflation_method"
          />
          <Select
            value={inflationMethod}
            onValueChange={(value) => setMarkets({ inflationMethod: value as InflationMethod })}
          >
            <SelectTrigger>
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="fixed">
                {t("goals:tax_plan.settings.markets.inflation_fixed")}
              </SelectItem>
              <SelectItem value="historical">
                {t(
                  method === "historical"
                    ? "goals:tax_plan.settings.markets.inflation_historical"
                    : "goals:tax_plan.settings.markets.inflation_average",
                )}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>
      )}

      {inflationMethod === "fixed" && (
        <AmountField
          id="tax-market-inflation"
          label={t("goals:tax_plan.settings.markets.inflation")}
          info="market_inflation"
          unit="percent"
          value={markets.inflation ?? investment.inflationRate}
          onCommit={(value) => setMarkets({ inflation: value ?? investment.inflationRate })}
        />
      )}

      <div className="space-y-3 sm:col-span-2 lg:col-span-3">
        <div className="grid gap-3 sm:grid-cols-2">
          <SwitchField
            id="tax-market-manual-mix"
            label={t("goals:tax_plan.settings.markets.manual_mix")}
            info="asset_mix"
            checked={manual !== null}
            note={
              manual
                ? t("goals:tax_plan.settings.markets.mix_manual_note")
                : derived
                  ? t("goals:tax_plan.settings.markets.mix_from_holdings")
                  : t("goals:tax_plan.settings.markets.mix_fallback", {
                      stocks: percent(equityShare),
                      bonds: percent(1 - equityShare),
                    })
            }
            onChange={(checked) =>
              setMarkets({
                startingMix: checked
                  ? {
                      taxable: scaledMix(starting.taxable),
                      taxDeferred: scaledMix(starting.taxDeferred),
                      roth: scaledMix(starting.roth),
                      hsa: scaledMix(starting.hsa),
                    }
                  : null,
              })
            }
          />
          <SwitchField
            id="tax-market-glide"
            label={t("goals:tax_plan.settings.markets.glide_to_target")}
            info="target_mix"
            checked={target !== null}
            onChange={(checked) => setMarkets({ targetMix: checked ? DEFAULT_TARGET_MIX : null })}
          />
        </div>

        {!manual && unclassified > 0 && (
          <p className="rounded-md border border-amber-500/40 bg-amber-500/10 px-3 py-2 text-xs text-amber-800 dark:text-amber-300">
            {t("goals:tax_plan.settings.markets.unclassified_warning", {
              amount: formatAmount(Math.round(unclassified), plan.currency),
            })}
          </p>
        )}

        <div className="overflow-x-auto">
          <table className="w-full text-sm">
            <thead>
              <tr className="text-muted-foreground text-xs">
                <th className="py-1 pr-3 text-left font-medium">
                  {t("goals:tax_plan.settings.markets.mix_account")}
                </th>
                {ASSET_CLASSES.map((key) => (
                  <th key={key} className="px-2 py-1 text-right font-medium">
                    {t(`goals:tax_plan.settings.markets.class_${key}`)}
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {BUCKETS.map((bucket) => (
                <tr key={bucket} className="border-t">
                  <td className="py-1.5 pr-3">
                    {t(`goals:tax_plan.settings.markets.bucket_${bucket}`)}
                  </td>
                  {ASSET_CLASSES.map((key) => {
                    const mix = manual ? starting[bucket] : scaledMix(starting[bucket]);
                    return (
                      <td key={key} className="px-2 py-1.5 text-right">
                        <PercentCell
                          label={`${t(`goals:tax_plan.settings.markets.bucket_${bucket}`)} ${t(
                            `goals:tax_plan.settings.markets.class_${key}`,
                          )}`}
                          value={mix[key]}
                          onCommit={
                            manual ? (value) => setStartingWeight(bucket, key, value) : undefined
                          }
                        />
                      </td>
                    );
                  })}
                </tr>
              ))}
              {target && (
                <tr className="border-t font-medium">
                  <td className="py-1.5 pr-3">{t("goals:tax_plan.settings.markets.target")}</td>
                  {ASSET_CLASSES.map((key) => (
                    <td key={key} className="px-2 py-1.5 text-right">
                      <PercentCell
                        label={`${t("goals:tax_plan.settings.markets.target")} ${t(
                          `goals:tax_plan.settings.markets.class_${key}`,
                        )}`}
                        value={target[key]}
                        onCommit={(value) => setMarkets({ targetMix: { ...target, [key]: value } })}
                      />
                    </td>
                  ))}
                </tr>
              )}
            </tbody>
          </table>
        </div>
        {unbalanced && (
          <p className="text-muted-foreground text-xs">
            {t("goals:tax_plan.settings.markets.mix_scaled")}
          </p>
        )}
        {method === "plan" && (
          <p className="text-muted-foreground text-xs">
            {t("goals:tax_plan.settings.markets.mix_success_only")}
          </p>
        )}
      </div>

      {target && (
        <>
          <div className="space-y-1">
            <FieldLabel label={t("goals:tax_plan.settings.markets.glide")} info="glide" />
            <Select
              value={markets.glide ?? "linear"}
              onValueChange={(value) => setMarkets({ glide: value as GlideShape })}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="linear">
                  {t("goals:tax_plan.settings.markets.glide_linear")}
                </SelectItem>
                <SelectItem value="sCurve">
                  {t("goals:tax_plan.settings.markets.glide_s_curve")}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          <AmountField
            id="tax-market-target-age"
            label={t("goals:tax_plan.settings.markets.target_age")}
            info="target_age"
            unit="age"
            value={markets.targetAge}
            optional
            note={
              markets.targetAge == null
                ? t("goals:tax_plan.settings.markets.target_age_default", {
                    age: plan.personal.planningHorizonAge,
                  })
                : undefined
            }
            onCommit={(value) =>
              setMarkets({ targetAge: value == null ? null : Math.round(value) })
            }
          />
        </>
      )}
    </Section>
  );
}

export function TaxOptimizerSettingsCard({
  plan,
  startingAccounts,
  onSavePlan,
}: {
  plan: RetirementPlan;
  /** Balances and holdings mixes the plan starts from, when loaded. */
  startingAccounts?: TaxOptimizerAccounts;
  onSavePlan: (plan: RetirementPlan) => void;
}) {
  const { t } = useTranslation();
  const { formatAmount } = useAmountFormatting();
  const settings: TaxOptimizerSettings = plan.taxOptimizer ?? {};
  const update = (patch: Partial<TaxOptimizerSettings>) =>
    onSavePlan({ ...plan, taxOptimizer: { ...settings, ...patch } });
  const updatePersonal = (patch: Partial<RetirementPlan["personal"]>) =>
    onSavePlan({ ...plan, personal: { ...plan.personal, ...patch } });
  const goal: TaxPlanGoal = settings.goal ?? "fixedAge";
  const earliest = goal === "earliestAge";
  const linkedStream = plan.incomeStreams.find((s) => s.id === settings.socialSecurityStreamId);
  // Amounts typed here before Social Security moved to the Retirement Income
  // card; the planner still uses them until they are moved.
  const enteredSocialSecurity = !linkedStream && (settings.socialSecurityMonthly ?? 0) > 0;
  const hasSocialSecurity = linkedStream
    ? (linkedStream.monthlyAmount ?? 0) > 0
    : enteredSocialSecurity;
  const claimOptimized = (settings.optimizeClaimAge ?? false) && hasSocialSecurity;
  const moveSocialSecurity = () => {
    const id = crypto.randomUUID?.() ?? `stream-${Date.now()}`;
    onSavePlan({
      ...plan,
      incomeStreams: [
        ...plan.incomeStreams,
        {
          id,
          label: t("goals:sidebar.income.social_security"),
          streamType: "db",
          startAge: Math.round(settings.socialSecurityClaimAge ?? 67),
          adjustForInflation: true,
          monthlyAmount: settings.socialSecurityMonthly,
        },
      ],
      taxOptimizer: {
        ...settings,
        socialSecurityStreamId: id,
        socialSecurityMonthly: 0,
        socialSecurityClaimAge: null,
      },
    });
  };
  const salary = plan.personal.currentAnnualSalary ?? 0;
  const defaultMagi = Math.max(
    0,
    salary - (settings.preTaxContribution ?? 0) - (settings.hsaContribution ?? 0),
  );
  // Today's must-have spending: the floor every year has to cover.
  const spendingFloor =
    activeExpenseItems(plan.expenses, plan.personal.currentAge)
      .filter((item) => item.essential ?? true)
      .reduce((sum, item) => sum + item.monthlyAmount, 0) * 12;
  const oldMinimum = settings.minimumExpenses ?? 0;
  const money = (value: number) => formatAmount(Math.round(value), plan.currency);

  return (
    <Card>
      <CardHeader className="pb-2">
        <CardTitle className="text-sm">{t("goals:tax_plan.settings.title")}</CardTitle>
      </CardHeader>
      <CardContent className="space-y-6">
        <Section title={t("goals:tax_plan.settings.section.goal")}>
          <div className="space-y-1">
            <FieldLabel label={t("goals:tax_plan.settings.goal")} info="goal" />
            <Select value={goal} onValueChange={(value) => update({ goal: value as TaxPlanGoal })}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="fixedAge">
                  {t("goals:tax_plan.settings.goal_fixed_age", {
                    age: plan.personal.targetRetirementAge,
                  })}
                </SelectItem>
                <SelectItem value="earliestAge">
                  {t("goals:tax_plan.settings.goal_earliest_age")}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
          {!earliest && (
            <>
              <AmountField
                id="tax-retirement-age"
                label={t("goals:tax_plan.settings.retirement_age")}
                info="retirement_age"
                unit="age"
                value={plan.personal.targetRetirementAge}
                onCommit={(value) =>
                  value != null &&
                  value > plan.personal.currentAge &&
                  value < plan.personal.planningHorizonAge &&
                  updatePersonal({ targetRetirementAge: Math.round(value) })
                }
              />
              <div className="space-y-1">
                <FieldLabel label={t("goals:tax_plan.settings.objective")} info="objective" />
                <Select
                  value={settings.objective ?? "maxBequest"}
                  onValueChange={(value) => update({ objective: value as TaxOptimizerObjective })}
                >
                  <SelectTrigger>
                    <SelectValue />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value="maxBequest">
                      {t("goals:tax_plan.settings.objective_max_bequest")}
                    </SelectItem>
                    <SelectItem value="maxSpending">
                      {t("goals:tax_plan.settings.objective_max_spending")}
                    </SelectItem>
                  </SelectContent>
                </Select>
              </div>
            </>
          )}
          {(earliest || settings.objective === "maxSpending") && (
            <AmountField
              id="tax-bequest-floor"
              label={t("goals:tax_plan.settings.bequest_floor")}
              info="bequest_floor"
              unit="total"
              value={settings.bequestFloor}
              onCommit={(value) => update({ bequestFloor: value ?? 0 })}
            />
          )}
        </Section>

        <Section title={t("goals:tax_plan.settings.section.expenses")}>
          <p className="text-muted-foreground text-xs">
            {spendingFloor > 0
              ? t("goals:tax_plan.settings.spending_floor", { amount: money(spendingFloor) })
              : t("goals:tax_plan.settings.spending_floor_none")}
          </p>
          {oldMinimum > 0 && (
            <div className="bg-muted/40 space-y-2 rounded-md p-3 text-xs">
              <p>
                {t("goals:tax_plan.settings.old_minimum", {
                  amount: money(oldMinimum),
                  floor: money(spendingFloor),
                })}
              </p>
              <Button variant="outline" size="sm" onClick={() => update({ minimumExpenses: 0 })}>
                {t("goals:tax_plan.settings.old_minimum_dismiss")}
              </Button>
            </div>
          )}
        </Section>

        <Section title={t("goals:tax_plan.settings.section.social_security")}>
          <p className="text-muted-foreground text-xs">
            {linkedStream
              ? t(
                  claimOptimized
                    ? "goals:tax_plan.settings.social_security_summary_optimized"
                    : "goals:tax_plan.settings.social_security_summary",
                  {
                    label: linkedStream.label,
                    amount: money(linkedStream.monthlyAmount ?? 0),
                    age: linkedStream.startAge,
                  },
                )
              : enteredSocialSecurity
                ? t("goals:tax_plan.settings.social_security_entered", {
                    amount: money(settings.socialSecurityMonthly ?? 0),
                    age: settings.socialSecurityClaimAge ?? 67,
                  })
                : t("goals:tax_plan.settings.social_security_none")}
          </p>
          {enteredSocialSecurity && (
            <div className="bg-muted/40 space-y-2 rounded-md p-3 text-xs">
              <p>{t("goals:tax_plan.settings.social_security_move_note")}</p>
              <Button variant="outline" size="sm" onClick={moveSocialSecurity}>
                {t("goals:tax_plan.settings.social_security_move")}
              </Button>
            </div>
          )}
          <SwitchField
            id="tax-optimize-claim"
            label={t("goals:tax_plan.settings.optimize_claim_age")}
            info="optimize_claim_age"
            checked={claimOptimized}
            disabled={!hasSocialSecurity}
            note={
              hasSocialSecurity
                ? undefined
                : t("goals:tax_plan.settings.optimize_claim_age_needs_benefit")
            }
            onChange={(checked) => update({ optimizeClaimAge: checked })}
          />
        </Section>

        <Section title={t("goals:tax_plan.settings.section.work")}>
          <AmountField
            id="tax-salary"
            label={t("goals:tax_plan.settings.salary")}
            info="salary"
            unit="perYear"
            value={plan.personal.currentAnnualSalary}
            optional
            onCommit={(value) => updatePersonal({ currentAnnualSalary: value ?? undefined })}
          />
          <AmountField
            id="tax-salary-growth"
            label={t("goals:tax_plan.settings.salary_growth")}
            info="salary_growth"
            unit="percent"
            value={plan.personal.salaryGrowthRate ?? plan.investment.inflationRate}
            onCommit={(value) => updatePersonal({ salaryGrowthRate: value ?? undefined })}
          />
          <SwitchField
            id="tax-optimize-contributions"
            label={t("goals:tax_plan.settings.optimize_contributions")}
            info="optimize_contributions"
            checked={settings.optimizeContributions ?? false}
            onChange={(checked) => update({ optimizeContributions: checked })}
          />
          {settings.optimizeContributions ? (
            <>
              <AmountField
                id="tax-match-rate"
                label={t("goals:tax_plan.settings.match_rate")}
                info="match_rate"
                unit="percent"
                value={settings.matchRate}
                onCommit={(value) => update({ matchRate: value ?? 0 })}
              />
              <AmountField
                id="tax-match-cap"
                label={t("goals:tax_plan.settings.match_cap")}
                info="match_cap"
                unit="percent"
                value={settings.matchCap}
                onCommit={(value) => update({ matchCap: value ?? 0 })}
              />
              <SwitchField
                id="tax-hsa-eligible"
                label={t("goals:tax_plan.settings.hsa_eligible")}
                info="hsa_eligible"
                checked={settings.hsaEligible ?? false}
                onChange={(checked) => update({ hsaEligible: checked })}
              />
              {settings.hsaEligible && (
                <SwitchField
                  id="tax-hsa-family"
                  label={t("goals:tax_plan.settings.hsa_family_coverage")}
                  info="hsa_family_coverage"
                  checked={settings.hsaFamilyCoverage ?? false}
                  onChange={(checked) => update({ hsaFamilyCoverage: checked })}
                />
              )}
              <SwitchField
                id="tax-roth-401k"
                label={t("goals:tax_plan.settings.roth_401k_available")}
                info="roth_401k_available"
                checked={settings.roth401kAvailable ?? true}
                onChange={(checked) => update({ roth401kAvailable: checked })}
              />
              <SwitchField
                id="tax-leftover-partly"
                label={t("goals:tax_plan.settings.leftover_partly")}
                info="leftover_partly"
                checked={settings.leftoverInvestRate != null}
                onChange={(checked) => update({ leftoverInvestRate: checked ? 0.8 : null })}
              />
              {settings.leftoverInvestRate != null && (
                <AmountField
                  id="tax-leftover-rate"
                  label={t("goals:tax_plan.settings.leftover_invest_rate")}
                  info="leftover_invest_rate"
                  unit="percent"
                  value={settings.leftoverInvestRate}
                  onCommit={(value) =>
                    update({ leftoverInvestRate: Math.min(Math.max(value ?? 0.8, 0), 1) })
                  }
                />
              )}
            </>
          ) : (
            <>
              <AmountField
                id="tax-pre-tax"
                label={t("goals:tax_plan.settings.pre_tax_contribution")}
                info="pre_tax_contribution"
                unit="perYear"
                value={settings.preTaxContribution}
                onCommit={(value) => update({ preTaxContribution: value ?? 0 })}
              />
              <AmountField
                id="tax-employer"
                label={t("goals:tax_plan.settings.employer_contribution")}
                info="employer_contribution"
                unit="perYear"
                value={settings.employerContribution}
                onCommit={(value) => update({ employerContribution: value ?? 0 })}
              />
              <AmountField
                id="tax-roth"
                label={t("goals:tax_plan.settings.roth_contribution")}
                info="roth_contribution"
                unit="perYear"
                value={settings.rothContribution}
                onCommit={(value) => update({ rothContribution: value ?? 0 })}
              />
              <AmountField
                id="tax-hsa"
                label={t("goals:tax_plan.settings.hsa_contribution")}
                info="hsa_contribution"
                unit="perYear"
                value={settings.hsaContribution}
                onCommit={(value) => update({ hsaContribution: value ?? 0 })}
              />
              {(settings.hsaContribution ?? 0) > 0 && (
                <SwitchField
                  id="tax-hsa-family"
                  label={t("goals:tax_plan.settings.hsa_family_coverage")}
                  info="hsa_family_coverage"
                  checked={settings.hsaFamilyCoverage ?? false}
                  onChange={(checked) => update({ hsaFamilyCoverage: checked })}
                />
              )}
            </>
          )}
          <AmountField
            id="tax-taxable"
            label={t("goals:tax_plan.settings.taxable_contribution")}
            info="taxable_contribution"
            unit="perYear"
            value={settings.taxableContribution}
            onCommit={(value) => update({ taxableContribution: value ?? 0 })}
          />
          {/* Grows the fixed amounts above; optimized ones are chosen each year. */}
          {(!settings.optimizeContributions || (settings.taxableContribution ?? 0) > 0) && (
            <AmountField
              id="tax-contribution-growth"
              label={t("goals:tax_plan.settings.contribution_growth")}
              info="contribution_growth"
              unit="percent"
              value={plan.investment.contributionGrowthRate}
              onCommit={(value) =>
                onSavePlan({
                  ...plan,
                  investment: { ...plan.investment, contributionGrowthRate: value ?? 0 },
                })
              }
            />
          )}
        </Section>

        <Section title={t("goals:tax_plan.settings.section.taxes")}>
          <div className="space-y-1">
            <FieldLabel label={t("goals:tax_plan.settings.state")} info="state" />
            <Select
              value={settings.state ?? NO_STREAM}
              onValueChange={(value) => update({ state: value === NO_STREAM ? null : value })}
            >
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value={NO_STREAM}>{t("goals:tax_plan.settings.no_state")}</SelectItem>
                <SelectItem value="VA">{t("goals:tax_plan.settings.state_va")}</SelectItem>
              </SelectContent>
            </Select>
          </div>
          <AmountField
            id="tax-prior-magi"
            label={t("goals:tax_plan.settings.prior_magi")}
            info="prior_magi"
            unit="perYear"
            note={t("goals:tax_plan.settings.prior_magi_help", { amount: money(defaultMagi) })}
            value={settings.priorMagi}
            optional
            onCommit={(value) => update({ priorMagi: value })}
          />
          <AmountField
            id="tax-max-conversion"
            label={t("goals:tax_plan.settings.max_roth_conversion")}
            info="max_roth_conversion"
            unit="perYear"
            value={settings.maxRothConversion}
            optional
            onCommit={(value) => update({ maxRothConversion: value })}
          />
          <SwitchField
            id="tax-harvest"
            label={t("goals:tax_plan.settings.harvest_gains")}
            info="harvest_gains"
            checked={settings.harvestGains ?? false}
            onChange={(checked) => update({ harvestGains: checked })}
          />
          <AmountField
            id="tax-aca"
            label={t("goals:tax_plan.settings.aca_benchmark_premium")}
            info="aca_benchmark_premium"
            unit="perYear"
            value={settings.acaBenchmarkPremium}
            onCommit={(value) => update({ acaBenchmarkPremium: value ?? 0 })}
          />
          <AmountField
            id="tax-medical"
            label={t("goals:tax_plan.settings.other_medical_expenses")}
            info="other_medical_expenses"
            unit="perYear"
            value={settings.otherMedicalExpenses}
            onCommit={(value) => update({ otherMedicalExpenses: value ?? 0 })}
          />
        </Section>

        <Section title={t("goals:tax_plan.settings.section.early_access")}>
          <AmountField
            id="tax-rule-55"
            label={t("goals:tax_plan.settings.rule_of_55_balance")}
            info="rule_of_55_balance"
            unit="total"
            value={settings.ruleOf55Balance}
            optional
            onCommit={(value) => update({ ruleOf55Balance: value })}
          />
          <AmountField
            id="tax-sepp-age"
            label={t("goals:tax_plan.settings.sepp_start_age")}
            info="sepp_start_age"
            unit="age"
            value={settings.seppStartAge}
            optional
            onCommit={(value) => update({ seppStartAge: value == null ? null : Math.round(value) })}
          />
          <AmountField
            id="tax-sepp-payment"
            label={t("goals:tax_plan.settings.sepp_annual_payment")}
            info="sepp_annual_payment"
            unit="perYear"
            value={settings.seppAnnualPayment}
            onCommit={(value) => update({ seppAnnualPayment: value ?? 0 })}
          />
        </Section>

        <MarketsSection
          plan={plan}
          settings={settings}
          startingAccounts={startingAccounts}
          update={update}
        />

        <Section title={t("goals:tax_plan.settings.section.estate")}>
          <AmountField
            id="tax-heirs-rate"
            label={t("goals:tax_plan.settings.heirs_tax_rate")}
            info="heirs_tax_rate"
            unit="percent"
            value={settings.heirsTaxRate ?? 0.3}
            onCommit={(value) => update({ heirsTaxRate: Math.min(value ?? 0, 0.99) })}
          />
          <DebtEditor debts={settings.debts ?? []} onChange={(debts) => update({ debts })} />
        </Section>
      </CardContent>
    </Card>
  );
}
