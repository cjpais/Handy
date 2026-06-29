import React from "react";

interface AppLanguageSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const AppLanguageSelector: React.FC<AppLanguageSelectorProps> =
  React.memo(() => {
    return null;
  });

AppLanguageSelector.displayName = "AppLanguageSelector";
