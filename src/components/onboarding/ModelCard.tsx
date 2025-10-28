import React from "react";
import { Download } from "lucide-react";
import { ModelInfo } from "../../lib/types";
import { formatModelSize } from "../../lib/utils/format";

interface ModelCardProps {
  model: ModelInfo;
  variant?: "default" | "featured";
  disabled?: boolean;
  badgeText?: string;
  className?: string;
  onSelect: (modelId: string) => void;
}

const ModelCard: React.FC<ModelCardProps> = ({
  model,
  variant = "default",
  disabled = false,
  badgeText,
  className = "",
  onSelect,
}) => {
  const isFeatured = variant === "featured";

  const baseButtonClasses =
    "relative rounded-xl p-3 p-4 text-left transition-all duration-200 disabled:opacity-50 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-logo-primary/25 active:scale-[0.98] cursor-pointer group";

  const variantClasses = isFeatured
    ? "border-2 border-logo-primary/40 bg-logo-primary/5 hover:border-logo-primary/60 hover:bg-logo-primary/10 hover:shadow-lg hover:scale-[1.02] disabled:hover:border-logo-primary/40 disabled:hover:bg-logo-primary/5 disabled:hover:shadow-none disabled:hover:scale-100"
    : "border-2 border-mid-gray/20 hover:border-logo-primary/50 hover:bg-logo-primary/5 hover:shadow-lg hover:scale-[1.02] disabled:hover:border-mid-gray/20 disabled:hover:bg-transparent disabled:hover:shadow-none disabled:hover:scale-100";

  const titleClasses =
    "text-lg font-semibold text-text group-hover:text-logo-primary transition-colors";

  const descriptionClasses = "text-text/60 text-sm leading-relaxed";

  const sizeRowClasses =
    "mt-1 flex items-center gap-2 text-xs text-text/60 tabular-nums";

  const containerSpacing = "space-y-3";

  const sizeIconClasses = "h-3.5 w-3.5 text-text/45";

  return (
    <button
      onClick={() => onSelect(model.id)}
      disabled={disabled}
      className={[baseButtonClasses, variantClasses, className]
        .filter(Boolean)
        .join(" ")}
      type="button"
    >
      {badgeText && (
        <div className="absolute -top-2 -right-2 bg-logo-primary text-white text-sm px-4 py-2 rounded-full font-medium shadow-md">
          {badgeText}
        </div>
      )}

      <div className={containerSpacing}>
        <div className="space-y-0">
          <h3 className={titleClasses}>{model.name}</h3>
          <p className={descriptionClasses}>{model.description}</p>
          <div className={sizeRowClasses}>
            <Download
              aria-hidden="true"
              className={sizeIconClasses}
              strokeWidth={1.75}
            />
            <span className="sr-only">Download size</span>
            <span className="font-medium text-text/70">
              {formatModelSize(model.size_mb)}
            </span>
          </div>
        </div>
      </div>
    </button>
  );
};

export default ModelCard;
