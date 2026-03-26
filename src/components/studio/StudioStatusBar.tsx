import React from "react";
import Badge from "@/components/ui/Badge";

interface StudioStatusBarProps {
  modelName: string;
}

export const StudioStatusBar: React.FC<StudioStatusBarProps> = ({ modelName }) => {
  return (
    <div className="flex flex-wrap items-center gap-2">
      <Badge variant="secondary">Model: {modelName || "None selected"}</Badge>
      <Badge variant="success">Built-in audio import</Badge>
    </div>
  );
};
