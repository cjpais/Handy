import React from "react";
import { Download } from "lucide-react";
import type { ModelInfo } from "@/bindings";
import { formatModelSize } from "../../lib/utils/format";
import Badge from "../ui/Badge";

interface ModelCardProps {
  model: ModelInfo;
  variant?: "default" | "featured";
  disabled?: boolean;
  className?: string;
  onSelect: (modelId: string) => void;
}

const ModelCard: React.FC<ModelCardProps> = ({
  model,
  variant = "default",
  disabled = false,
  className = "",
  onSelect,
}) => {
  const isFeatured = variant === "featured";

  const baseButtonClasses =
    "flex justify-between items-center rounded-xl p-3 px-4 text-left transition-all duration-200 disabled:opacity-50 disabled:cursor-not-allowed focus:outline-none focus:ring-2 focus:ring-logo-primary/25 active:scale-[0.98] cursor-pointer group";

  const variantClasses = isFeatured
    ? "border-2 border-logo-primary/25 bg-logo-primary/5 hover:border-logo-primary/40 hover:bg-logo-primary/8 hover:shadow-lg hover:scale-[1.02] disabled:hover:border-logo-primary/25 disabled:hover:bg-logo-primary/5 disabled:hover:shadow-none disabled:hover:scale-100"
    : "border-2 border-mid-gray/20 hover:border-logo-primary/50 hover:bg-logo-primary/5 hover:shadow-lg hover:scale-[1.02] disabled:hover:border-mid-gray/20 disabled:hover:bg-transparent disabled:hover:shadow-none disabled:hover:scale-100";

  return (
    <button
      onClick={() => onSelect(model.id)}
      disabled={disabled}
      className={[baseButtonClasses, variantClasses, className]
        .filter(Boolean)
        .join(" ")}
      type="button"
    >
      <div className="flex flex-col items-ce">
        <div className="flex items-center gap-4">
          <h3 className="text-lg font-semibold text-text group-hover:text-logo-primary transition-colors">
            {model.name}
          </h3>
          <DownloadSize sizeMb={Number(model.size_mb)} />
          {isFeatured && <Badge variant="primary">Recommended</Badge>}
        </div>
        <p className="text-text/60 text-sm leading-relaxed">
          {model.description}
        </p>
      </div>

      <div className="space-y-1">
        <div className="flex items-center gap-2">
          <p className="text-xs text-text/70 w-16 text-right">accuracy</p>
          <div className="w-20 h-2 bg-mid-gray/20 rounded-full overflow-hidden">
            <div
              className="h-full bg-logo-primary rounded-full transition-all duration-300"
              style={{ width: `${model.accuracy_score * 100}%` }}
            />
          </div>
        </div>
        <div className="flex items-center gap-2">
          <p className="text-xs text-text/70 w-16 text-right">speed</p>
          <div className="w-20 h-2 bg-mid-gray/20 rounded-full overflow-hidden">
            <div
              className="h-full bg-logo-primary rounded-full transition-all duration-300"
              style={{ width: `${model.speed_score * 100}%` }}
            />
          </div>
        </div>
      </div>
    </button>
  );
};

const DownloadSize = ({ sizeMb }: { sizeMb: number }) => {
  return (
    <div className="flex items-center gap-1.5 text-xs text-text/60 tabular-nums">
      <Download
        aria-hidden="true"
        className="h-3.5 w-3.5 text-text/45"
        strokeWidth={1.75}
      />
      <span className="sr-only">Download size</span>
      <span className="font-medium text-text/70">
        {formatModelSize(sizeMb)}
      </span>
    </div>
  );
};

export default ModelCard;
