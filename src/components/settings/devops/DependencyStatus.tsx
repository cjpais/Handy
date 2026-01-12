import React from "react";
import { useTranslation } from "react-i18next";
import { DependencyStatus as DependencyStatusType } from "@/bindings";
import { CheckCircle2, XCircle, Copy, ExternalLink } from "lucide-react";

interface DependencyStatusProps {
  name: string;
  displayName: string;
  icon: React.ReactNode;
  status: DependencyStatusType;
}

export const DependencyStatus: React.FC<DependencyStatusProps> = ({
  name,
  displayName,
  icon,
  status,
}) => {
  const { t } = useTranslation();

  const copyInstallCommand = () => {
    navigator.clipboard.writeText(status.install_hint);
  };

  return (
    <div className="flex items-start gap-3 p-3 rounded-lg bg-mid-gray/10">
      {/* Status icon */}
      <div className="mt-0.5">
        {status.installed ? (
          <CheckCircle2 className="w-5 h-5 text-green-400" />
        ) : (
          <XCircle className="w-5 h-5 text-red-400" />
        )}
      </div>

      {/* Content */}
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          {icon}
          <span className="font-medium">{displayName}</span>
          <code className="text-xs px-1.5 py-0.5 rounded bg-mid-gray/20 text-mid-gray">
            {name}
          </code>
        </div>

        {status.installed ? (
          <div className="mt-1 text-sm text-mid-gray">
            <div className="flex items-center gap-2">
              <span>{t("devops.dependencies.version")}:</span>
              <code className="text-green-400">{status.version || t("devops.dependencies.unknown")}</code>
            </div>
            {status.path && (
              <div className="flex items-center gap-2 mt-0.5">
                <span>{t("devops.dependencies.path")}:</span>
                <code className="text-xs truncate max-w-[200px]" title={status.path}>
                  {status.path}
                </code>
              </div>
            )}
          </div>
        ) : (
          <div className="mt-2">
            <p className="text-sm text-yellow-400 mb-2">
              {t("devops.dependencies.notInstalled")}
            </p>
            <div className="flex items-center gap-2">
              <code className="flex-1 text-xs px-2 py-1.5 rounded bg-black/30 text-green-400 font-mono">
                {status.install_hint}
              </code>
              <button
                onClick={copyInstallCommand}
                className="p-1.5 rounded hover:bg-mid-gray/20 transition-colors"
                title={t("devops.dependencies.copyCommand")}
              >
                <Copy className="w-4 h-4" />
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
};
