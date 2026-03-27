import React from "react";
import { useTranslation } from "react-i18next";
import Badge from "@/components/ui/Badge";

interface StudioStatusBarProps {
  modelName: string;
}

export const StudioStatusBar: React.FC<StudioStatusBarProps> = ({ modelName }) => {
  const { t } = useTranslation();

  return (
    <div className="flex flex-wrap items-center gap-2">
      <Badge variant="secondary">
        {t("studio.status.model", {
          defaultValue: "Model: {{name}}",
          name: modelName || t("studio.status.noneSelected", { defaultValue: "None selected" }),
        })}
      </Badge>
      <Badge variant="success">
        {t("studio.status.import", { defaultValue: "Built-in audio import" })}
      </Badge>
    </div>
  );
};
