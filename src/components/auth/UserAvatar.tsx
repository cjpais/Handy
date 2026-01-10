import React, { useState } from "react";
import { useTranslation } from "react-i18next";
import { User, LogOut, Settings, ChevronDown, LogIn } from "lucide-react";
import { useAuth } from "@/hooks/useAuth";
import ProfileModal from "./ProfileModal";

interface UserAvatarProps {
  size?: "sm" | "md" | "lg";
  onSignInClick?: () => void;
}

const UserAvatar: React.FC<UserAvatarProps> = ({ size = "sm", onSignInClick }) => {
  const { t } = useTranslation();
  const { isAuthenticated, isLoading, user, signOut } = useAuth();
  const [showDropdown, setShowDropdown] = useState(false);
  const [showProfileModal, setShowProfileModal] = useState(false);

  const sizeClasses = {
    sm: "w-6 h-6",
    md: "w-8 h-8",
    lg: "w-10 h-10",
  };

  // Show nothing while loading
  if (isLoading) {
    return null;
  }

  // Show sign in button if not authenticated
  if (!isAuthenticated || !user) {
    // Check if we're in guest/ghost mode (stored in localStorage)
    const authMode = localStorage.getItem("auth_mode");

    // Only show sign in button if in guest mode (ghost mode is fully offline)
    if (authMode === "guest" && onSignInClick) {
      return (
        <button
          onClick={onSignInClick}
          className="flex items-center gap-1.5 px-2 py-1 text-xs text-text/60 hover:text-logo-primary hover:bg-logo-primary/10 rounded transition-colors"
          title={t("auth.signIn.title")}
        >
          <LogIn className="w-3.5 h-3.5" />
          <span>{t("footer.signIn")}</span>
        </button>
      );
    }

    return null;
  }

  const handleSignOut = async () => {
    setShowDropdown(false);
    await signOut();
    // Clear auth mode so they see the auth screen on next load
    localStorage.removeItem("auth_mode");
    // Reload to show auth screen
    window.location.reload();
  };

  const handleOpenProfile = () => {
    setShowDropdown(false);
    setShowProfileModal(true);
  };

  return (
    <>
      <div className="relative">
        <button
          onClick={() => setShowDropdown(!showDropdown)}
          className="flex items-center gap-1.5 rounded-full hover:bg-mid-gray/20 p-0.5 transition-colors"
          title={user.name || user.email || t("auth.profile.title")}
        >
          {user.avatar_url ? (
            <img
              src={user.avatar_url}
              alt={user.name || t("auth.profile.avatar")}
              className={`${sizeClasses[size]} rounded-full object-cover border border-mid-gray/30`}
            />
          ) : (
            <div
              className={`${sizeClasses[size]} rounded-full bg-logo-primary/20 border border-logo-primary/30 flex items-center justify-center`}
            >
              <User className="w-3 h-3 text-logo-primary" />
            </div>
          )}
          <ChevronDown className="w-3 h-3 text-text/50" />
        </button>

        {/* Dropdown menu */}
        {showDropdown && (
          <>
            {/* Backdrop */}
            <div
              className="fixed inset-0 z-40"
              onClick={() => setShowDropdown(false)}
            />

            {/* Menu */}
            <div className="absolute right-0 bottom-full mb-2 w-48 py-1 bg-background-dark border border-mid-gray/30 rounded-lg shadow-lg z-50">
              {/* User info header */}
              <div className="px-3 py-2 border-b border-mid-gray/20">
                <p className="text-sm font-medium text-text truncate">
                  {user.name || t("auth.profile.anonymous")}
                </p>
                {user.email && (
                  <p className="text-xs text-text/50 truncate">{user.email}</p>
                )}
                {user.provider && (
                  <p className="text-xs text-text/40 capitalize mt-0.5">
                    {t("auth.profile.via")} {user.provider}
                  </p>
                )}
              </div>

              {/* Menu items */}
              <button
                onClick={handleOpenProfile}
                className="w-full flex items-center gap-2 px-3 py-2 text-sm text-text/80 hover:bg-mid-gray/20 transition-colors"
              >
                <Settings className="w-4 h-4" />
                {t("auth.profile.viewProfile")}
              </button>

              <button
                onClick={handleSignOut}
                className="w-full flex items-center gap-2 px-3 py-2 text-sm text-red-400 hover:bg-red-500/10 transition-colors"
              >
                <LogOut className="w-4 h-4" />
                {t("auth.signOut")}
              </button>
            </div>
          </>
        )}
      </div>

      {/* Profile Modal */}
      <ProfileModal
        isOpen={showProfileModal}
        onClose={() => setShowProfileModal(false)}
      />
    </>
  );
};

export default UserAvatar;
