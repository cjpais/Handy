import React from "react";

interface OutputLanguageSelectorProps {
  descriptionMode?: "inline" | "tooltip";
  grouped?: boolean;
}

export const OutputLanguageSelector: React.FC<OutputLanguageSelectorProps> =
  React.memo(() => {
    return null;
  });

OutputLanguageSelector.displayName = "OutputLanguageSelector";
