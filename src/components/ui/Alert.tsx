import React from "react";
import { AlertCircle, AlertTriangle, Info, CheckCircle } from "lucide-react";

type AlertVariant = "error" | "warning" | "info" | "success";

interface AlertProps {
  variant?: AlertVariant;
  /** When true, removes rounded corners for use inside containers */
  contained?: boolean;
  children: React.ReactNode;
  className?: string;
}

const variantStyles: Record<
  AlertVariant,
  { container: string; icon: string; text: string }
> = {
  error: {
    container: "bg-alarm-red/10 border border-alarm-red/20",
    icon: "text-alarm-red",
    text: "text-charcoal",
  },
  warning: {
    container: "bg-terracotta/10 border border-terracotta/20",
    icon: "text-terracotta",
    text: "text-charcoal",
  },
  info: {
    container: "bg-tide-teal/10 border border-tide-teal/20",
    icon: "text-tide-teal",
    text: "text-charcoal",
  },
  success: {
    container: "bg-forest-green/10 border border-forest-green/20",
    icon: "text-forest-green",
    text: "text-charcoal",
  },
};

const variantIcons: Record<AlertVariant, React.ElementType> = {
  error: AlertCircle,
  warning: AlertTriangle,
  info: Info,
  success: CheckCircle,
};

export const Alert: React.FC<AlertProps> = ({
  variant = "error",
  contained = false,
  children,
  className = "",
}) => {
  const styles = variantStyles[variant];
  const Icon = variantIcons[variant];

  return (
    <div
      className={`flex items-start gap-3 p-4 ${styles.container} ${contained ? "" : "rounded-buttons"} ${className}`}
    >
      <Icon className={`w-5 h-5 shrink-0 mt-0.5 ${styles.icon}`} />
      <div className={`text-sm ${styles.text}`}>{children}</div>
    </div>
  );
};
