import React from "react";

interface SettingsGroupProps {
  title?: React.ReactNode;
  description?: string;
  className?: string;
  children: React.ReactNode;
}

export const SettingsGroup: React.FC<SettingsGroupProps> = ({
  title,
  description,
  className = "",
  children,
}) => {
  return (
    <div className={`space-y-2 ${className}`}>
      {title && (
        <div className="px-4">
          <h2 className="text-[11px] font-semibold text-bark-grey font-mono uppercase tracking-[0.10em]">
            {title}
          </h2>
          {description && (
            <p className="text-xs text-bark-grey mt-1">{description}</p>
          )}
        </div>
      )}
      <div className="bg-orange-off-white/40 backdrop-blur-sm border border-stone-mist rounded-cards overflow-visible shadow-sm">
        <div className="divide-y divide-stone-mist">{children}</div>
      </div>
    </div>
  );
};
