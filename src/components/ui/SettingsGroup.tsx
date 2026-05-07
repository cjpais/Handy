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
    <div className="space-y-3">
      {title && (
        <div className="px-4">
          <h2 className="text-xs font-semibold uppercase tracking-[0.18em] text-slate-500">
            {title}
          </h2>
          {description && (
            <p className="mt-1 text-xs text-slate-500">{description}</p>
          )}
        </div>
      )}
      <div className="overflow-visible rounded-[24px] border border-slate-200 bg-white shadow-[0_14px_36px_rgba(15,23,42,0.04)]">
        <div className="divide-y divide-slate-200">{children}</div>
      </div>
    </div>
  );
};
