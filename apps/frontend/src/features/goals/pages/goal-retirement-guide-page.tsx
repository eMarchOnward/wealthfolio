import { AnimatedToggleGroup, Page, PageContent, PageHeader } from "@wealthfolio/ui";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate, useParams } from "react-router-dom";
import GuidePage from "@/features/goals/retirement-planner/pages/guide-page";
import { TaxOptimizedGuide } from "@/features/goals/retirement-planner/components/tax-optimized-guide";
import { timezoneToCountry } from "@/features/goals/retirement-planner/lib/timezone";
import { useSettingsContext } from "@/lib/settings-provider";
import { useGoalDetail } from "../hooks/use-goal-detail";

type GuideStyle = "classic" | "optimized";

export default function GoalRetirementGuidePage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { goalId } = useParams<{ goalId: string }>();
  const { settings } = useSettingsContext();
  const { plan } = useGoalDetail(goalId);
  // Opens on the goal's own style; the other style's guide is one click away.
  const [chosen, setChosen] = useState<GuideStyle | null>(null);
  const style: GuideStyle = chosen ?? (plan?.plannerMode === "optimized" ? "optimized" : "classic");

  return (
    <Page>
      <PageHeader
        heading={t("goals:guide.title")}
        text={
          style === "optimized" ? t("goals:guide.subtitle_optimized") : t("goals:guide.subtitle")
        }
        onBack={() => navigate(goalId ? `/goals/${goalId}` : "/goals")}
        actions={
          <AnimatedToggleGroup<GuideStyle>
            variant="default"
            size="sm"
            rounded="full"
            className="bg-muted/60 p-1"
            items={[
              { value: "classic", label: t("goals:guide.style_classic") },
              { value: "optimized", label: t("goals:guide.style_optimized") },
            ]}
            value={style}
            onValueChange={setChosen}
          />
        }
      />
      <PageContent>
        <div className="mx-auto max-w-4xl">
          {style === "optimized" ? (
            <TaxOptimizedGuide />
          ) : (
            <GuidePage country={timezoneToCountry(settings?.timezone)} />
          )}
        </div>
      </PageContent>
    </Page>
  );
}
