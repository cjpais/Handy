import "./App.css";
import { Settings } from "./components/settings/Settings";
import HandyTextLogo from "./components/icons/HandyTextLogo";
import AccessibilityPermissions from "./components/AccessibilityPermissions";
import Footer from "./components/footer";
import Onboarding from "./components/onboarding";

import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

function App() {
  const [showOnboarding, setShowOnboarding] = useState<boolean | null>(null);

  useEffect(() => {
    checkOnboardingStatus();
  }, []);

  const checkOnboardingStatus = async () => {
    try {
      // Always check if they have any models available
      const modelsAvailable: boolean = await invoke("has_any_models_available");
      setShowOnboarding(!modelsAvailable);
    } catch (error) {
      console.error("Failed to check onboarding status:", error);
      setShowOnboarding(true);
    }
  };

  const handleModelSelected = () => {
    // Transition to main app - user has started a download
    setShowOnboarding(false);
  };

  if (showOnboarding) {
    return (
      <div className="min-h-screen flex flex-col w-full">
        <div className="flex flex-col items-center pt-6 gap-8 px-4 flex-1">
          <HandyTextLogo width={300} />
          <Onboarding onModelSelected={handleModelSelected} />
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen flex flex-col w-full">
      <div className="flex flex-col items-center pt-6 gap-8 px-4 flex-1">
        <HandyTextLogo width={300} />
        <AccessibilityPermissions />
        <Settings />
      </div>
      <Footer />
    </div>
  );
}

export default App;
