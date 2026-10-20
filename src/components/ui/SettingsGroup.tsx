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
          <h2 className="text-xs font-medium text-mid-gray uppercase tracking-wide v2:text-label v2:font-semibold v2:text-ink-3">
            {title}
          </h2>
          {description && (
            <p className="text-xs text-mid-gray mt-1">{description}</p>
          )}
        </div>
      )}
      <div className="bg-background border border-mid-gray/20 rounded-lg overflow-visible v2:border-[1.5px] v2:border-line-strong v2:rounded-panel">
        <div className="divide-y divide-mid-gray/20 v2:divide-line">
          {children}
        </div>
      </div>
    </div>
  );
};
