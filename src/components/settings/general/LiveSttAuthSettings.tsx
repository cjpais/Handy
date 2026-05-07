import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { commands } from "@/bindings";
import { Button } from "../../ui/Button";
import { Input } from "../../ui/Input";
import { SettingContainer } from "../../ui/SettingContainer";
import type { LiveSttServerUrlValidationResult } from "./livesttValidation";
import { isLiveSttServerUrlValidForLogin } from "./livesttValidation";

interface LiveSttAuthSettingsProps {
  serverUrlInput: string;
  serverUrlValidation: LiveSttServerUrlValidationResult;
  saveServerUrlIfValid: () => Promise<string | null>;
  refreshKey: number;
}

export const LiveSttAuthSettings: React.FC<LiveSttAuthSettingsProps> = ({
  serverUrlInput,
  serverUrlValidation,
  saveServerUrlIfValid,
  refreshKey,
}) => {
  const { t } = useTranslation();
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [isAuthenticated, setIsAuthenticated] = useState(false);
  const [authMessage, setAuthMessage] = useState<string | null>(null);
  const [authError, setAuthError] = useState<string | null>(null);
  const [isAuthUpdating, setIsAuthUpdating] = useState(false);

  const refreshAuthStatus = async () => {
    try {
      const result = await commands.livesttAuthStatus();
      if (result.status === "ok") {
        setIsAuthenticated(result.data.is_authenticated);
        return;
      }
    } catch {}

    setIsAuthenticated(false);
  };

  useEffect(() => {
    setAuthMessage(null);
    setAuthError(null);
    void refreshAuthStatus();
  }, [refreshKey]);

  const canLogin =
    !isAuthUpdating &&
    isLiveSttServerUrlValidForLogin(serverUrlInput) &&
    !serverUrlValidation.isEmpty &&
    username.trim().length > 0 &&
    password.length > 0;

  const handleLogin = async () => {
    setIsAuthUpdating(true);
    setAuthMessage(null);
    setAuthError(null);

    try {
      const normalizedServerUrl = await saveServerUrlIfValid();
      if (!normalizedServerUrl || !username.trim() || !password) {
        return;
      }

      const result = await commands.livesttLogin(
        normalizedServerUrl,
        username.trim(),
        password,
      );

      if (result.status === "ok") {
        setAuthMessage(t("settings.transcriptionBackend.livestt.loginSuccess"));
      } else {
        setAuthError(
          t("settings.transcriptionBackend.livestt.loginError", {
            error: result.error,
          }),
        );
      }
    } catch {
      setAuthError(t("settings.transcriptionBackend.livestt.loginErrorSafe"));
    } finally {
      setPassword("");
      await refreshAuthStatus();
      setIsAuthUpdating(false);
    }
  };

  const handleLogout = async () => {
    setIsAuthUpdating(true);
    setAuthMessage(null);
    setAuthError(null);

    try {
      const result = await commands.livesttLogout();
      if (result.status === "ok") {
        setAuthMessage(
          t("settings.transcriptionBackend.livestt.logoutSuccess"),
        );
      } else {
        setAuthError(
          t("settings.transcriptionBackend.livestt.logoutError", {
            error: result.error,
          }),
        );
      }
    } catch {
      setAuthError(t("settings.transcriptionBackend.livestt.logoutErrorSafe"));
    } finally {
      await refreshAuthStatus();
      setIsAuthUpdating(false);
    }
  };

  return (
    <SettingContainer
      title={t("settings.transcriptionBackend.livestt.auth.title")}
      description={t("settings.transcriptionBackend.livestt.auth.description")}
      descriptionMode="tooltip"
      grouped={true}
      layout="stacked"
    >
      <div className="space-y-2">
        <div className="flex flex-col gap-2 sm:flex-row">
          <Input
            type="text"
            value={username}
            onChange={(event) => setUsername(event.target.value)}
            placeholder={t(
              "settings.transcriptionBackend.livestt.username.placeholder",
            )}
            disabled={isAuthUpdating}
            className="flex-1"
            autoComplete="username"
          />
          <Input
            type="password"
            value={password}
            onChange={(event) => setPassword(event.target.value)}
            placeholder={t(
              "settings.transcriptionBackend.livestt.password.placeholder",
            )}
            disabled={isAuthUpdating}
            className="flex-1"
            autoComplete="current-password"
          />
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            type="button"
            size="sm"
            onClick={() => {
              void handleLogin();
            }}
            disabled={!canLogin}
          >
            {isAuthUpdating
              ? t("settings.transcriptionBackend.livestt.auth.updating")
              : t("settings.transcriptionBackend.livestt.login")}
          </Button>
          <Button
            type="button"
            size="sm"
            variant="secondary"
            onClick={() => {
              void handleLogout();
            }}
            disabled={isAuthUpdating || !isAuthenticated}
          >
            {t("settings.transcriptionBackend.livestt.logout")}
          </Button>
          <span
            className={`text-xs font-medium ${
              isAuthenticated ? "text-green-600" : "text-mid-gray"
            }`}
          >
            {isAuthenticated
              ? t("settings.transcriptionBackend.livestt.auth.authenticated")
              : t("settings.transcriptionBackend.livestt.auth.unauthenticated")}
          </span>
        </div>
        {authMessage && <p className="text-xs text-green-600">{authMessage}</p>}
        {authError && <p className="text-xs text-red-500">{authError}</p>}
      </div>
    </SettingContainer>
  );
};
