import React, { useEffect, useState, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { SettingsGroup } from "../../ui/SettingsGroup";
import {
  Key,
  Bot,
  Database,
  Trash2,
  Check,
  Loader2,
  AlertCircle,
  Globe,
} from "lucide-react";

export const CredentialsSettings: React.FC = () => {
  const { t } = useTranslation();

  // Discord token state
  const [hasDiscordToken, setHasDiscordToken] = useState(false);
  const [maskedDiscordToken, setMaskedDiscordToken] = useState<string | null>(null);

  // Supabase state
  const [supabaseUrl, setSupabaseUrl] = useState("");
  const [supabaseUrlInput, setSupabaseUrlInput] = useState("");
  const [hasSupabaseKey, setHasSupabaseKey] = useState(false);
  const [maskedSupabaseKey, setMaskedSupabaseKey] = useState<string | null>(null);
  const [supabaseKeyInput, setSupabaseKeyInput] = useState("");

  // UI state
  const [isSavingUrl, setIsSavingUrl] = useState(false);
  const [isSavingKey, setIsSavingKey] = useState(false);
  const [urlSaved, setUrlSaved] = useState(false);
  const [keySaved, setKeySaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Load existing credentials on mount
  useEffect(() => {
    const loadCredentials = async () => {
      // Load Discord token status
      const discordHasToken = await commands.discordHasToken();
      setHasDiscordToken(discordHasToken);
      if (discordHasToken) {
        const masked = await commands.discordGetToken();
        setMaskedDiscordToken(masked ?? null);
      }

      // Load Supabase URL (always has a value due to default)
      const url = await commands.getSupabaseUrl();
      setSupabaseUrl(url);
      setSupabaseUrlInput(url);

      // Load Supabase anon key status (always true due to default)
      const hasKey = await commands.hasSupabaseAnonKey();
      setHasSupabaseKey(hasKey);
      const masked = await commands.getSupabaseAnonKey();
      setMaskedSupabaseKey(masked);
    };

    loadCredentials();
  }, []);

  const handleSaveSupabaseUrl = useCallback(async () => {
    setIsSavingUrl(true);
    setError(null);

    const result = await commands.setSupabaseUrl(supabaseUrlInput.trim());
    if (result.status === "error") {
      setError(result.error);
      setIsSavingUrl(false);
      return;
    }

    setSupabaseUrl(supabaseUrlInput.trim());
    setIsSavingUrl(false);
    setUrlSaved(true);
    setTimeout(() => setUrlSaved(false), 2000);
  }, [supabaseUrlInput]);

  const handleSaveSupabaseKey = useCallback(async () => {
    if (!supabaseKeyInput.trim()) {
      setError(t("credentials.errors.keyRequired"));
      return;
    }

    setIsSavingKey(true);
    setError(null);

    const result = await commands.setSupabaseAnonKey(supabaseKeyInput.trim());
    if (result.status === "error") {
      setError(result.error);
      setIsSavingKey(false);
      return;
    }

    setHasSupabaseKey(true);
    const masked = await commands.getSupabaseAnonKey();
    setMaskedSupabaseKey(masked);
    setSupabaseKeyInput("");
    setIsSavingKey(false);
    setKeySaved(true);
    setTimeout(() => setKeySaved(false), 2000);
  }, [supabaseKeyInput, t]);

  const handleClearSupabaseCredentials = useCallback(async () => {
    const result = await commands.clearSupabaseCredentials();
    if (result.status === "error") {
      setError(result.error);
      return;
    }

    // Reload defaults after clearing
    const url = await commands.getSupabaseUrl();
    setSupabaseUrl(url);
    setSupabaseUrlInput(url);
    const masked = await commands.getSupabaseAnonKey();
    setMaskedSupabaseKey(masked);
    setSupabaseKeyInput("");
  }, []);

  return (
    <div className="flex flex-col gap-6 max-w-3xl w-full mx-auto">
      {/* Discord Token Reference (Read-only) */}
      <SettingsGroup title={t("credentials.discord.title")}>
        <div className="p-4">
          <div className="flex items-center gap-2 mb-4">
            <Bot className="w-5 h-5 text-logo-primary" />
            <h3 className="font-medium">{t("credentials.discord.heading")}</h3>
          </div>

          <p className="text-sm text-text/60 mb-4">
            {t("credentials.discord.description")}
          </p>

          {hasDiscordToken ? (
            <div className="p-3 bg-green-500/10 border border-green-500/30 rounded-lg">
              <div className="flex items-center gap-2 mb-2">
                <Check className="w-4 h-4 text-green-400" />
                <span className="text-sm font-medium text-green-400">
                  {t("credentials.discord.configured")}
                </span>
              </div>
              <div className="flex items-center gap-2 text-sm text-text/60">
                <Key className="w-3 h-3" />
                <span className="font-mono">{maskedDiscordToken}</span>
              </div>
              <p className="text-xs text-text/40 mt-2">
                {t("credentials.discord.manageHint")}
              </p>
            </div>
          ) : (
            <div className="p-3 bg-yellow-500/10 border border-yellow-500/30 rounded-lg">
              <div className="flex items-center gap-2">
                <AlertCircle className="w-4 h-4 text-yellow-400" />
                <span className="text-sm text-yellow-400">
                  {t("credentials.discord.notConfigured")}
                </span>
              </div>
              <p className="text-xs text-text/40 mt-2">
                {t("credentials.discord.setupHint")}
              </p>
            </div>
          )}
        </div>
      </SettingsGroup>

      {/* Supabase Configuration */}
      <SettingsGroup title={t("credentials.supabase.title")}>
        <div className="p-4">
          <div className="flex items-center gap-2 mb-4">
            <Database className="w-5 h-5 text-logo-primary" />
            <h3 className="font-medium">{t("credentials.supabase.heading")}</h3>
          </div>

          <p className="text-sm text-text/60 mb-4">
            {t("credentials.supabase.description")}
          </p>

          {/* Error Display */}
          {error && (
            <div className="mb-4 p-3 bg-red-500/10 border border-red-500/30 rounded-lg flex items-center gap-2">
              <AlertCircle className="w-4 h-4 text-red-400 shrink-0" />
              <span className="text-sm text-red-400">{error}</span>
            </div>
          )}

          {/* Supabase URL */}
          <div className="mb-4">
            <label className="flex items-center gap-2 text-sm font-medium mb-2">
              <Globe className="w-4 h-4 text-text/60" />
              {t("credentials.supabase.urlLabel")}
            </label>
            <div className="flex gap-2">
              <input
                type="url"
                value={supabaseUrlInput}
                onChange={(e) => setSupabaseUrlInput(e.target.value)}
                placeholder={t("credentials.supabase.urlPlaceholder")}
                className="flex-1 px-3 py-2 bg-background-dark/50 border border-background-dark rounded-lg text-sm focus:outline-none focus:border-logo-primary"
              />
              <button
                onClick={handleSaveSupabaseUrl}
                disabled={isSavingUrl || supabaseUrlInput === supabaseUrl}
                className={`px-4 py-2 rounded-lg text-sm font-medium transition-colors flex items-center gap-1 ${
                  isSavingUrl || supabaseUrlInput === supabaseUrl
                    ? "bg-background-dark/30 text-text/30 cursor-not-allowed"
                    : "bg-logo-primary/20 text-logo-primary hover:bg-logo-primary/30"
                }`}
              >
                {isSavingUrl ? (
                  <Loader2 className="w-4 h-4 animate-spin" />
                ) : urlSaved ? (
                  <Check className="w-4 h-4" />
                ) : null}
                {t("credentials.supabase.save")}
              </button>
            </div>
            <p className="text-xs text-text/40 mt-1">
              {t("credentials.supabase.urlHint")}
            </p>
          </div>

          {/* Supabase Anon Key */}
          <div className="mb-4">
            <label className="flex items-center gap-2 text-sm font-medium mb-2">
              <Key className="w-4 h-4 text-text/60" />
              {t("credentials.supabase.keyLabel")}
            </label>
            {hasSupabaseKey ? (
              <div className="flex gap-2 items-center">
                <div className="flex-1 px-3 py-2 bg-background-dark/50 border border-background-dark rounded-lg text-sm font-mono text-text/60">
                  {maskedSupabaseKey || "********"}
                </div>
                <button
                  onClick={handleClearSupabaseCredentials}
                  className="px-3 py-2 bg-red-500/20 text-red-400 hover:bg-red-500/30 rounded-lg text-sm font-medium transition-colors flex items-center gap-1"
                  title={t("credentials.supabase.clear")}
                >
                  <Trash2 className="w-4 h-4" />
                  {t("credentials.supabase.clear")}
                </button>
              </div>
            ) : (
              <div className="flex gap-2">
                <input
                  type="password"
                  value={supabaseKeyInput}
                  onChange={(e) => setSupabaseKeyInput(e.target.value)}
                  placeholder={t("credentials.supabase.keyPlaceholder")}
                  className="flex-1 px-3 py-2 bg-background-dark/50 border border-background-dark rounded-lg text-sm focus:outline-none focus:border-logo-primary"
                  autoComplete="off"
                  spellCheck={false}
                />
                <button
                  onClick={handleSaveSupabaseKey}
                  disabled={isSavingKey || !supabaseKeyInput.trim()}
                  className={`px-4 py-2 rounded-lg text-sm font-medium transition-colors flex items-center gap-1 ${
                    isSavingKey || !supabaseKeyInput.trim()
                      ? "bg-background-dark/30 text-text/30 cursor-not-allowed"
                      : "bg-logo-primary/20 text-logo-primary hover:bg-logo-primary/30"
                  }`}
                >
                  {isSavingKey ? (
                    <Loader2 className="w-4 h-4 animate-spin" />
                  ) : keySaved ? (
                    <Check className="w-4 h-4" />
                  ) : null}
                  {t("credentials.supabase.save")}
                </button>
              </div>
            )}
            <p className="text-xs text-text/40 mt-1">
              {t("credentials.supabase.keyHint")}
            </p>
            {hasSupabaseKey && (
              <p className="text-xs text-green-400/70 mt-1 flex items-center gap-1">
                <Check className="w-3 h-3" />
                {t("credentials.supabase.keySaved")}
              </p>
            )}
          </div>

          {/* Status indicator */}
          {supabaseUrl && hasSupabaseKey && (
            <div className="p-3 bg-green-500/10 border border-green-500/30 rounded-lg">
              <div className="flex items-center gap-2">
                <Check className="w-4 h-4 text-green-400" />
                <span className="text-sm font-medium text-green-400">
                  {t("credentials.supabase.configured")}
                </span>
              </div>
              <p className="text-xs text-text/40 mt-1">
                {t("credentials.supabase.readyHint")}
              </p>
            </div>
          )}
        </div>
      </SettingsGroup>

      {/* Info Section */}
      <div className="text-center text-text/60 py-4">
        <Database className="w-12 h-12 mx-auto mb-3 text-text/30" />
        <p className="text-sm mb-2">{t("credentials.info.title")}</p>
        <p className="text-xs text-text/40 max-w-md mx-auto">
          {t("credentials.info.description")}
        </p>
      </div>
    </div>
  );
};
