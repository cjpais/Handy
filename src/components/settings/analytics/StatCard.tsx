import React from "react";

interface StatCardProps {
  icon: React.ReactNode;
  label: string;
  value: string;
  subtitle?: string;
}

export const StatCard: React.FC<StatCardProps> = ({
  icon,
  label,
  value,
  subtitle,
}) => (
  <div className="bg-mid-gray/10 rounded-lg p-4">
    <div className="flex items-center gap-2 text-mid-gray mb-2">
      {icon}
      <span className="text-xs uppercase tracking-wide">{label}</span>
    </div>
    <div className="text-2xl font-semibold">{value}</div>
    {subtitle ? <div className="text-xs text-mid-gray">{subtitle}</div> : null}
  </div>
);
