import React from "react";
import { useTranslation } from "react-i18next";
import { SettingsGroup } from "../../ui/SettingsGroup";
import { Snippets } from "../Snippets";

export const PoptartSettings: React.FC = () => {
  const { t } = useTranslation();

  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title={t("settings.poptart.groups.snippets")}>
        <Snippets descriptionMode="tooltip" grouped />
      </SettingsGroup>
    </div>
  );
};
