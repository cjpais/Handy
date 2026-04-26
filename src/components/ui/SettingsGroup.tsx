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
        <div className="flex items-center gap-2 px-1">
          <div className="h-3.5 w-0.5 rounded-full bg-logo-primary opacity-80" />
          <h2 className="text-xs font-semibold text-foreground/60 uppercase tracking-widest">
            {title}
          </h2>
          {description && (
            <p className="text-xs text-mid-gray ml-1">{description}</p>
          )}
        </div>
      )}
      <div className="rounded-xl border border-white/[0.06] bg-white/[0.04] shadow-[0_1px_3px_rgba(0,0,0,0.2),inset_0_1px_0_rgba(255,255,255,0.05)]">
        <div className="divide-y divide-white/[0.06]">{children}</div>
      </div>
    </div>
  );
};
