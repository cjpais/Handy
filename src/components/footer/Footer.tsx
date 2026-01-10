import React, { useState, useEffect } from "react";
import { getVersion } from "@tauri-apps/api/app";

import ModelSelector from "../model-selector";
import UpdateChecker from "../update-checker";
import SidecarStatus from "./SidecarStatus";
import UserAvatar from "../auth/UserAvatar";

const Footer: React.FC = () => {
  const [version, setVersion] = useState("");

  useEffect(() => {
    const fetchVersion = async () => {
      try {
        const appVersion = await getVersion();
        setVersion(appVersion);
      } catch (error) {
        console.error("Failed to get app version:", error);
        setVersion("0.1.2");
      }
    };

    fetchVersion();
  }, []);

  const handleSignInClick = () => {
    // Clear auth mode and reload to show auth screen
    localStorage.removeItem("auth_mode");
    window.location.reload();
  };

  return (
    <div className="w-full border-t border-mid-gray/20 pt-3">
      <div className="flex justify-between items-center text-xs px-4 pb-3 text-text/60">
        <div className="flex items-center gap-4">
          <ModelSelector />
          <span className="text-text/30">|</span>
          <SidecarStatus />
        </div>

        {/* Right side: User Avatar, Update Status, Version */}
        <div className="flex items-center gap-3">
          {/* User Avatar (shows avatar when authenticated, sign in button in guest mode) */}
          <UserAvatar size="sm" onSignInClick={handleSignInClick} />

          {/* Update Status */}
          <div className="flex items-center gap-1">
            <UpdateChecker />
            <span>•</span>
            {/* eslint-disable-next-line i18next/no-literal-string */}
            <span>v{version}</span>
          </div>
        </div>
      </div>
    </div>
  );
};

export default Footer;
