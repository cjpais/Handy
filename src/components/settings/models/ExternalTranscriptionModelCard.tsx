import React from "react";
import { useTranslation } from "react-i18next";
import { Check, Cloud, Globe, Loader2, Pencil, Trash2 } from "lucide-react";
import Badge from "@/components/ui/Badge";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import type { ExternalTranscriptionModelProfile } from "@/lib/utils/externalTranscriptionModel";

interface ExternalTranscriptionModelCardProps {
  profile: ExternalTranscriptionModelProfile;
  isActive: boolean;
  isInstalled: boolean;
  savedApiKey: string;
  apiKeySaving?: boolean;
  apiKeyError?: string | null;
  isSwitching?: boolean;
  onSelect: () => void;
  onSaveApiKey: (value: string) => Promise<void> | void;
  onRemoveApiKey: () => Promise<void> | void;
}

export const ExternalTranscriptionModelCard: React.FC<
  ExternalTranscriptionModelCardProps
> = ({
  profile,
  isActive,
  isInstalled,
  savedApiKey,
  apiKeySaving = false,
  apiKeyError = null,
  isSwitching = false,
  onSelect,
  onSaveApiKey,
  onRemoveApiKey,
}) => {
  const { t } = useTranslation();
  const [isEditing, setIsEditing] = React.useState(!isInstalled);
  const [draftApiKey, setDraftApiKey] = React.useState(savedApiKey);

  React.useEffect(() => {
    setDraftApiKey(savedApiKey);
  }, [savedApiKey]);

  React.useEffect(() => {
    if (!isInstalled) {
      setIsEditing(true);
      return;
    }

    if (savedApiKey.trim()) {
      setIsEditing(false);
    }
  }, [isInstalled, savedApiKey]);

  const isBusy = apiKeySaving || isSwitching;
  const isClickable = isInstalled && !isActive && !isBusy && !isEditing;
  const trimmedDraftApiKey = draftApiKey.trim();

  const maskApiKey = (apiKey: string) => {
    const trimmedApiKey = apiKey.trim();
    if (!trimmedApiKey) {
      return "";
    }

    const suffix = trimmedApiKey.slice(-4);
    return `••••••••${suffix}`;
  };

  const handleClick = () => {
    if (!isClickable) return;
    onSelect();
  };

  const handleSave = async () => {
    if (!trimmedDraftApiKey || isBusy) {
      return;
    }

    await onSaveApiKey(trimmedDraftApiKey);
  };

  const handleCancelEdit = () => {
    setDraftApiKey(savedApiKey);
    setIsEditing(false);
  };

  return (
    <div
      onClick={handleClick}
      onKeyDown={(event) => {
        if (!isClickable) return;
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onSelect();
        }
      }}
      role={isClickable ? "button" : undefined}
      tabIndex={isClickable ? 0 : undefined}
      className={[
        "flex flex-col rounded-xl px-4 py-3 gap-2 text-left transition-all duration-200 border-2",
        isActive
          ? "border-logo-primary/50 bg-logo-primary/10"
          : "border-mid-gray/20",
        isClickable
          ? "cursor-pointer hover:border-logo-primary/50 hover:bg-logo-primary/5 hover:shadow-lg hover:scale-[1.01] active:scale-[0.99] group"
          : "",
      ]
        .filter(Boolean)
        .join(" ")}
    >
      <div className="flex justify-between items-center w-full">
        <div className="flex flex-col items-start flex-1 min-w-0">
          <div className="flex items-center gap-3 flex-wrap">
            <h3
              className={`text-base font-semibold text-text ${isClickable ? "group-hover:text-logo-primary" : ""} transition-colors`}
            >
              {profile.fullLabel}
            </h3>
            {isActive && (
              <Badge variant="primary">
                <Check className="w-3 h-3 mr-1" />
                {t("modelSelector.active")}
              </Badge>
            )}
            {isSwitching && (
              <Badge variant="secondary">
                <Loader2 className="w-3 h-3 mr-1 animate-spin" />
                {t("modelSelector.switching")}
              </Badge>
            )}
          </div>
          <p className="text-text/60 text-sm leading-relaxed">
            {profile.description}
          </p>
        </div>

        <div className="hidden sm:flex items-center ms-4">
          <div className="space-y-1">
            <div className="flex items-center gap-2">
              <p className="text-xs text-text/60 w-24 text-end">
                {t("onboarding.modelCard.accuracy")}
              </p>
              <div className="w-16 h-1.5 bg-mid-gray/20 rounded-full overflow-hidden">
                <div
                  className="h-full bg-logo-primary rounded-full"
                  style={{ width: `${profile.accuracyScore * 100}%` }}
                />
              </div>
            </div>
            <div className="flex items-center gap-2">
              <p className="text-xs text-text/60 w-24 text-end">
                {t("onboarding.modelCard.speed")}
              </p>
              <div className="w-16 h-1.5 bg-mid-gray/20 rounded-full overflow-hidden">
                <div
                  className="h-full bg-logo-primary rounded-full"
                  style={{ width: `${profile.speedScore * 100}%` }}
                />
              </div>
            </div>
          </div>
        </div>
      </div>

      <hr className="w-full border-mid-gray/20" />

      <div className="flex items-center gap-3 w-full -mb-0.5 mt-0.5 min-h-5">
        <div className="flex items-center gap-1 text-xs text-text/50">
          <Globe className="w-3.5 h-3.5" />
          <span>{t("modelSelector.capabilities.multiLanguage")}</span>
        </div>
        <div className="flex items-center gap-1 text-xs text-text/50 ms-auto">
          <Cloud className="w-3.5 h-3.5" />
          <span>
            {t("settings.models.external.cloudStorage", {
              defaultValue: "Cloud",
            })}
          </span>
        </div>
      </div>

      <div
        className="mt-2 pt-2 border-t border-mid-gray/20"
        onClick={(event) => event.stopPropagation()}
      >
        {isInstalled && !isEditing ? (
          <div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <div className="min-w-0">
              <p className="text-xs text-text/60">
                {t("settings.models.external.apiKey.title", {
                  defaultValue: "API key",
                })}
              </p>
              <p className="text-sm font-medium text-text/80 tracking-[0.18em] truncate">
                {maskApiKey(savedApiKey)}
              </p>
            </div>
            <div className="flex items-center gap-2">
              <Button
                type="button"
                variant="ghost"
                size="sm"
                disabled={isBusy}
                className="flex items-center gap-1.5"
                onClick={(event) => {
                  event.stopPropagation();
                  setIsEditing(true);
                }}
              >
                <Pencil className="w-3.5 h-3.5" />
                <span>{t("common.edit")}</span>
              </Button>
              <Button
                type="button"
                variant="danger-ghost"
                size="sm"
                disabled={isBusy}
                className="flex items-center gap-1.5"
                onClick={(event) => {
                  event.stopPropagation();
                  void onRemoveApiKey();
                }}
              >
                <Trash2 className="w-3.5 h-3.5" />
                <span>{t("common.remove")}</span>
              </Button>
            </div>
          </div>
        ) : (
          <div className="flex flex-col gap-3">
            <p className="text-xs text-text/60">
              {t("settings.models.external.apiKey.title", {
                defaultValue: "API key",
              })}
            </p>
            <div className="flex flex-col gap-2 sm:flex-row sm:items-center">
              <Input
                type="password"
                value={draftApiKey}
                onChange={(event) => setDraftApiKey(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    void handleSave();
                  }
                }}
                disabled={isBusy}
                placeholder={t("settings.models.external.apiKey.placeholder", {
                  defaultValue: "Enter your ElevenLabs API key",
                })}
                className="flex-1 min-w-0 sm:min-w-[320px]"
              />
              <div className="flex items-center gap-2">
                <Button
                  type="button"
                  variant="primary-soft"
                  size="sm"
                  disabled={!trimmedDraftApiKey || isBusy}
                  className="flex items-center gap-1.5"
                  onClick={(event) => {
                    event.stopPropagation();
                    void handleSave();
                  }}
                >
                  {apiKeySaving ? (
                    <>
                      <Loader2 className="w-3.5 h-3.5 animate-spin" />
                      <span>
                        {t("settings.models.external.apiKey.verifying", {
                          defaultValue: "Verifying...",
                        })}
                      </span>
                    </>
                  ) : (
                    <span>
                      {isInstalled
                        ? t("common.save")
                        : t("settings.models.external.addApiKey", {
                            defaultValue: "Add API key",
                          })}
                    </span>
                  )}
                </Button>
                {isInstalled && (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    disabled={isBusy}
                    onClick={(event) => {
                      event.stopPropagation();
                      handleCancelEdit();
                    }}
                  >
                    {t("common.cancel")}
                  </Button>
                )}
              </div>
            </div>
            {apiKeyError && (
              <p className="text-xs text-red-400">{apiKeyError}</p>
            )}
          </div>
        )}
      </div>
    </div>
  );
};
