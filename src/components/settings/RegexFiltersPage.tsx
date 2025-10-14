import React from "react";
import { RegexFilters } from "./RegexFilters";
import { SettingsGroup } from "../ui/SettingsGroup";

export const RegexFiltersPage: React.FC = () => {
  return (
    <div className="max-w-3xl w-full mx-auto space-y-6">
      <SettingsGroup title="Regex Filters">
        <RegexFilters descriptionMode="tooltip" grouped />
      </SettingsGroup>
    </div>
  );
};