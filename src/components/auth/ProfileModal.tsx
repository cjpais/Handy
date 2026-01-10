import React from "react";
import { useTranslation } from "react-i18next";
import { X, User, Mail, Calendar, Shield, ExternalLink } from "lucide-react";
import { useAuth } from "@/hooks/useAuth";

interface ProfileModalProps {
  isOpen: boolean;
  onClose: () => void;
}

const ProfileModal: React.FC<ProfileModalProps> = ({ isOpen, onClose }) => {
  const { t } = useTranslation();
  const { user, signOut } = useAuth();

  if (!isOpen || !user) {
    return null;
  }

  const handleSignOut = async () => {
    await signOut();
    onClose();
  };

  // Get provider icon/color
  const getProviderInfo = (provider: string | null | undefined) => {
    switch (provider?.toLowerCase()) {
      case "github":
        return { color: "bg-gray-800", label: "GitHub" };
      case "discord":
        return { color: "bg-indigo-600", label: "Discord" };
      case "twitch":
        return { color: "bg-purple-600", label: "Twitch" };
      default:
        return { color: "bg-mid-gray", label: provider || "Unknown" };
    }
  };

  const providerInfo = getProviderInfo(user.provider);

  return (
    <>
      {/* Backdrop */}
      <div
        className="fixed inset-0 bg-black/50 backdrop-blur-sm z-50"
        onClick={onClose}
      />

      {/* Modal */}
      <div className="fixed inset-0 flex items-center justify-center z-50 p-4">
        <div
          className="bg-background border border-mid-gray/30 rounded-xl shadow-xl max-w-md w-full max-h-[90vh] overflow-hidden"
          onClick={(e) => e.stopPropagation()}
        >
          {/* Header */}
          <div className="flex items-center justify-between px-6 py-4 border-b border-mid-gray/20">
            <h2 className="text-lg font-semibold text-text">
              {t("auth.profile.title")}
            </h2>
            <button
              onClick={onClose}
              className="p-1 rounded-lg hover:bg-mid-gray/20 transition-colors"
            >
              <X className="w-5 h-5 text-text/60" />
            </button>
          </div>

          {/* Content */}
          <div className="p-6 space-y-6">
            {/* Avatar and name section */}
            <div className="flex items-center gap-4">
              {user.avatar_url ? (
                <img
                  src={user.avatar_url}
                  alt={user.name || t("auth.profile.avatar")}
                  className="w-16 h-16 rounded-full object-cover border-2 border-mid-gray/30"
                />
              ) : (
                <div className="w-16 h-16 rounded-full bg-logo-primary/20 border-2 border-logo-primary/30 flex items-center justify-center">
                  <User className="w-8 h-8 text-logo-primary" />
                </div>
              )}
              <div className="flex-1 min-w-0">
                <h3 className="text-xl font-semibold text-text truncate">
                  {user.name || t("auth.profile.anonymous")}
                </h3>
                <div className="flex items-center gap-2 mt-1">
                  <span
                    className={`px-2 py-0.5 text-xs rounded-full text-white ${providerInfo.color}`}
                  >
                    {providerInfo.label}
                  </span>
                </div>
              </div>
            </div>

            {/* User details */}
            <div className="space-y-3">
              {user.email && (
                <div className="flex items-center gap-3 p-3 rounded-lg bg-background-dark/50 border border-mid-gray/20">
                  <Mail className="w-5 h-5 text-text/40" />
                  <div className="flex-1 min-w-0">
                    <p className="text-xs text-text/50">{t("auth.profile.email")}</p>
                    <p className="text-sm text-text truncate">{user.email}</p>
                  </div>
                </div>
              )}

              <div className="flex items-center gap-3 p-3 rounded-lg bg-background-dark/50 border border-mid-gray/20">
                <Shield className="w-5 h-5 text-text/40" />
                <div className="flex-1 min-w-0">
                  <p className="text-xs text-text/50">{t("auth.profile.userId")}</p>
                  <p className="text-sm text-text font-mono truncate">{user.id}</p>
                </div>
              </div>

              <div className="flex items-center gap-3 p-3 rounded-lg bg-background-dark/50 border border-mid-gray/20">
                <Calendar className="w-5 h-5 text-text/40" />
                <div className="flex-1 min-w-0">
                  <p className="text-xs text-text/50">{t("auth.profile.provider")}</p>
                  <p className="text-sm text-text capitalize">
                    {user.provider || t("auth.profile.unknown")}
                  </p>
                </div>
              </div>
            </div>

            {/* Info note */}
            <div className="p-3 rounded-lg bg-blue-500/10 border border-blue-500/20">
              <p className="text-xs text-blue-400">
                {t("auth.profile.syncInfo")}
              </p>
            </div>
          </div>

          {/* Footer */}
          <div className="flex items-center justify-between px-6 py-4 border-t border-mid-gray/20 bg-background-dark/30">
            <button
              onClick={onClose}
              className="px-4 py-2 text-sm text-text/60 hover:text-text transition-colors"
            >
              {t("common.close")}
            </button>
            <button
              onClick={handleSignOut}
              className="px-4 py-2 text-sm text-red-400 hover:bg-red-500/10 rounded-lg transition-colors"
            >
              {t("auth.signOut")}
            </button>
          </div>
        </div>
      </div>
    </>
  );
};

export default ProfileModal;
