import React, { useEffect } from "react";
import { useTranslation } from "react-i18next";
import { Github, MessageCircle, Tv, WifiOff, User, Loader2, AlertCircle } from "lucide-react";
import KBVETextLogo from "../icons/KBVETextLogo";
import { useAuth } from "@/hooks/useAuth";
import { OAuthProvider } from "@/lib/supabase";

export type AuthMode = "signed_in" | "ghost" | "guest" | null;

interface AuthScreenProps {
  onAuthComplete: (mode: AuthMode) => void;
}

interface AuthButtonProps {
  icon: React.ReactNode;
  label: string;
  onClick: () => void;
  disabled?: boolean;
  loading?: boolean;
  variant?: "primary" | "secondary";
}

const AuthButton: React.FC<AuthButtonProps> = ({
  icon,
  label,
  onClick,
  disabled,
  loading,
  variant = "primary",
}) => {
  const baseStyles =
    "w-full flex items-center gap-3 px-4 py-3 rounded-lg font-medium transition-all duration-200";
  const variantStyles =
    variant === "primary"
      ? "bg-background-dark/50 border border-mid-gray/30 hover:border-logo-primary hover:bg-logo-primary/10 text-text"
      : "bg-transparent border border-mid-gray/20 hover:border-mid-gray/40 hover:bg-mid-gray/10 text-text/70";
  const disabledStyles = disabled || loading ? "opacity-50 cursor-not-allowed" : "cursor-pointer";

  return (
    <button
      onClick={onClick}
      disabled={disabled || loading}
      className={`${baseStyles} ${variantStyles} ${disabledStyles}`}
    >
      {loading ? <Loader2 className="w-5 h-5 animate-spin" /> : icon}
      <span>{label}</span>
    </button>
  );
};

export const AuthScreen: React.FC<AuthScreenProps> = ({ onAuthComplete }) => {
  const { t } = useTranslation();
  const { isAuthenticated, isLoading, error, signIn, cancelAuth } = useAuth();

  // Track which provider is being used for loading state
  const [loadingProvider, setLoadingProvider] = React.useState<OAuthProvider | null>(null);

  // When authentication succeeds, notify parent
  useEffect(() => {
    if (isAuthenticated) {
      onAuthComplete("signed_in");
    }
  }, [isAuthenticated, onAuthComplete]);

  const handleSignIn = async (provider: OAuthProvider) => {
    setLoadingProvider(provider);
    try {
      await signIn(provider);
      // Don't clear loading state here - it will be cleared when auth completes or errors
    } catch (err) {
      console.error(`Failed to sign in with ${provider}:`, err);
      setLoadingProvider(null);
    }
  };

  // Clear loading state on error
  useEffect(() => {
    if (error) {
      setLoadingProvider(null);
    }
  }, [error]);

  // Clear loading state when auth loading stops
  useEffect(() => {
    if (!isLoading && loadingProvider) {
      // Small delay to prevent flicker
      const timeout = setTimeout(() => {
        setLoadingProvider(null);
      }, 500);
      return () => clearTimeout(timeout);
    }
  }, [isLoading, loadingProvider]);

  const handleGhostMode = () => {
    onAuthComplete("ghost");
  };

  const handleGuestMode = () => {
    onAuthComplete("guest");
  };

  const handleCancelAuth = async () => {
    await cancelAuth();
    setLoadingProvider(null);
  };

  const isAuthInProgress = loadingProvider !== null;

  return (
    <div className="h-screen w-screen flex bg-background">
      {/* Left Panel - Sign In Options */}
      <div className="flex-1 flex flex-col items-center justify-center p-8 border-r border-mid-gray/20">
        <div className="w-full max-w-sm space-y-6">
          <div className="text-center mb-8">
            <KBVETextLogo width={180} className="mx-auto mb-4" />
            <h2 className="text-xl font-semibold text-text mb-2">
              {t("auth.signIn.title")}
            </h2>
            <p className="text-sm text-text/60">
              {t("auth.signIn.description")}
            </p>
          </div>

          {/* Error message */}
          {error && (
            <div className="flex items-center gap-2 p-3 rounded-lg bg-red-500/10 border border-red-500/20 text-red-400 text-sm">
              <AlertCircle className="w-4 h-4 flex-shrink-0" />
              <span>{error}</span>
            </div>
          )}

          <div className="space-y-3">
            <AuthButton
              icon={<Github className="w-5 h-5" />}
              label={t("auth.signIn.github")}
              onClick={() => handleSignIn("github")}
              loading={loadingProvider === "github"}
              disabled={isAuthInProgress}
            />
            <AuthButton
              icon={<MessageCircle className="w-5 h-5" />}
              label={t("auth.signIn.discord")}
              onClick={() => handleSignIn("discord")}
              loading={loadingProvider === "discord"}
              disabled={isAuthInProgress}
            />
            <AuthButton
              icon={<Tv className="w-5 h-5" />}
              label={t("auth.signIn.twitch")}
              onClick={() => handleSignIn("twitch")}
              loading={loadingProvider === "twitch"}
              disabled={isAuthInProgress}
            />
          </div>

          {/* Cancel button when auth is in progress */}
          {isAuthInProgress && (
            <button
              onClick={handleCancelAuth}
              className="w-full text-center text-sm text-text/50 hover:text-text/70 transition-colors"
            >
              {t("auth.signIn.cancel")}
            </button>
          )}

          <p className="text-xs text-text/40 text-center mt-6">
            {t("auth.signIn.hint")}
          </p>
        </div>
      </div>

      {/* Right Panel - Offline Options */}
      <div className="flex-1 flex flex-col items-center justify-center p-8 bg-background-dark/30">
        <div className="w-full max-w-sm space-y-8">
          {/* Ghost Mode */}
          <div className="space-y-4">
            <div
              onClick={isAuthInProgress ? undefined : handleGhostMode}
              className={`group p-6 rounded-xl border border-mid-gray/20 transition-all duration-200 ${
                isAuthInProgress
                  ? "opacity-50 cursor-not-allowed"
                  : "cursor-pointer hover:border-logo-primary/50 hover:bg-logo-primary/5"
              }`}
            >
              <div className="flex items-center gap-4 mb-3">
                <div className="p-3 rounded-lg bg-background-dark/50 group-hover:bg-logo-primary/20 transition-colors">
                  <WifiOff className="w-6 h-6 text-text/60 group-hover:text-logo-primary transition-colors" />
                </div>
                <div>
                  <h3 className="font-semibold text-text group-hover:text-logo-primary transition-colors">
                    {t("auth.ghost.title")}
                  </h3>
                  <p className="text-xs text-text/50">
                    {t("auth.ghost.subtitle")}
                  </p>
                </div>
              </div>
              <p className="text-sm text-text/60">
                {t("auth.ghost.description")}
              </p>
              <div className="flex flex-wrap gap-2 mt-3">
                <span className="px-2 py-1 text-xs rounded-full bg-green-500/10 text-green-400 border border-green-500/20">
                  {t("auth.ghost.badge.private")}
                </span>
                <span className="px-2 py-1 text-xs rounded-full bg-blue-500/10 text-blue-400 border border-blue-500/20">
                  {t("auth.ghost.badge.offline")}
                </span>
              </div>
            </div>

            {/* Divider */}
            <div className="flex items-center gap-4">
              <div className="flex-1 h-px bg-mid-gray/20" />
              <span className="text-xs text-text/40">{t("auth.or")}</span>
              <div className="flex-1 h-px bg-mid-gray/20" />
            </div>

            {/* Guest Mode */}
            <div
              onClick={isAuthInProgress ? undefined : handleGuestMode}
              className={`group p-6 rounded-xl border border-mid-gray/20 transition-all duration-200 ${
                isAuthInProgress
                  ? "opacity-50 cursor-not-allowed"
                  : "cursor-pointer hover:border-mid-gray/40 hover:bg-mid-gray/5"
              }`}
            >
              <div className="flex items-center gap-4 mb-3">
                <div className="p-3 rounded-lg bg-background-dark/50 group-hover:bg-mid-gray/20 transition-colors">
                  <User className="w-6 h-6 text-text/60 group-hover:text-text/80 transition-colors" />
                </div>
                <div>
                  <h3 className="font-semibold text-text">
                    {t("auth.guest.title")}
                  </h3>
                  <p className="text-xs text-text/50">
                    {t("auth.guest.subtitle")}
                  </p>
                </div>
              </div>
              <p className="text-sm text-text/60">
                {t("auth.guest.description")}
              </p>
              <div className="flex flex-wrap gap-2 mt-3">
                <span className="px-2 py-1 text-xs rounded-full bg-yellow-500/10 text-yellow-400 border border-yellow-500/20">
                  {t("auth.guest.badge.anonymous")}
                </span>
                <span className="px-2 py-1 text-xs rounded-full bg-purple-500/10 text-purple-400 border border-purple-500/20">
                  {t("auth.guest.badge.cloud")}
                </span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

export default AuthScreen;
