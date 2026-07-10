import React from "react";

interface SettingsGroupProps {
  title?: string;
  description?: string;
  children: React.ReactNode;
}

export const SettingsGroup: React.FC<SettingsGroupProps> = ({
  title,
  description,
  children,
}) => {
  return (
    <div className="space-y-2">
      {title && (
        <div className="px-4">
          <h2 className="text-xs font-medium text-muted-foreground uppercase tracking-wide">
            {title}
          </h2>
          {description && (
            <p className="text-xs text-muted-foreground mt-1">{description}</p>
          )}
        </div>
      )}
      <div className="bg-background border border-border/60 rounded-lg overflow-visible">
        <div className="divide-y divide-border/60">{children}</div>
      </div>
    </div>
  );
};
