import { useTranslation } from "react-i18next";
import { Section, Term } from "../pages/guide-page";

/** Sections of the tax-optimized guide and their terms, in reading order. */
const SECTIONS: [string, string[]][] = [
  ["quick_start", ["plan", "run", "overview", "risk", "cases"]],
  ["milp", ["idea", "lp", "mixed_integer", "loop", "foresight", "today", "owl"]],
  ["goals", ["goal", "objective", "objective_bequest", "floor", "bequest"]],
  ["working", ["salary", "fixed", "optimize", "match", "leftover", "hsa"]],
  ["social_security", ["benefit", "optimize_age"]],
  ["taxes", ["brackets", "conversions", "rmd", "harvest", "irmaa", "aca", "state"]],
  ["early_access", ["penalty", "rule_of_55", "sepp"]],
  ["estate_markets", ["debts", "home", "markets"]],
  ["overview", ["summary", "value", "contributions", "playbook"]],
  ["risk", ["success", "runs", "lifespans", "safe_spending", "choices", "regret", "stress"]],
  ["limits", ["single", "taxes", "markets"]],
];

/** How the tax-optimized planner works and how to read it. */
export function TaxOptimizedGuide() {
  const { t } = useTranslation();
  return (
    <div className="space-y-6 pb-8">
      {SECTIONS.map(([section, terms]) => (
        <Section key={section} title={t(`goals:guide.tax.${section}.title`)}>
          <div className="space-y-4">
            {terms.map((term) => (
              <Term key={term} t={t(`goals:guide.tax.${section}.${term}_title`)}>
                {t(`goals:guide.tax.${section}.${term}_desc`)}
              </Term>
            ))}
          </div>
        </Section>
      ))}
    </div>
  );
}
