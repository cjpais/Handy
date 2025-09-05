import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { Toaster } from "sonner";
import "./App.css";
import AccessibilityPermissions from "./components/AccessibilityPermissions";
import Footer from "./components/footer";
import HandyTextLogo from "./components/icons/HandyTextLogo";
import Onboarding from "./components/onboarding";
import { Settings } from "./components/settings/Settings";
import HandyHand from "./components/icons/HandyHand";
import { Cog, FlaskConical, History, Info } from "lucide-react";

const Option1 = () => {
  return (
    <div className="flex flex-col w-24 gap-3 border-r border-mid-gray/20 items-center py-4 px-2">
      <div className="flex flex-col gap-0.5 items-center justify-center w-18 h-18 bg-logo-primary/80 rounded-lg">
        <HandyHand width={32} height={32} />
        <p className="text-sm font-medium">General</p>
      </div>
      <div className="flex flex-col gap-1 items-center justify-center w-18 h-18 rounded-lg bg-mid-gray/10 hover:bg-logo-primary/50 hover:cursor-pointer">
        <Cog />
        <p className="text-xs font-semibold">Advanced</p>
      </div>
      <div className="flex flex-col gap-1 items-center justify-center w-18 h-18 rounded-lg bg-mid-gray/10 hover:bg-logo-primary/50 hover:cursor-pointer">
        <History />
        <p className="text-sm font-medium">History</p>
      </div>
      <div className="flex flex-col gap-1 items-center justify-center w-18 h-18 rounded-lg bg-mid-gray/10 hover:bg-logo-primary/50 hover:cursor-pointer">
        <Info />
        <p className="text-sm font-medium">About</p>
      </div>
      <div className="flex flex-col gap-1 items-center justify-center w-18 h-18 rounded-lg bg-mid-gray/10 hover:bg-logo-primary/50 hover:cursor-pointer">
        <FlaskConical />
        <p className="text-sm font-medium">Debug</p>
      </div>
    </div>
  );
};

const Option2 = () => {
  return (
    <div className="flex flex-col w-40 gap-5 border-r border-mid-gray/20 items-center py-4 px-2">
      <HandyTextLogo width={120} />
      <div className="flex flex-col w-full items-center gap-1 pt-6 border-t border-mid-gray/20">
        {/*<div className="border-b w-full border-mid-gray/20"></div>*/}
        <div className="flex gap-2 items-center p-2 w-full rounded-lg bg-logo-primary/80 hover:cursor-pointer">
          <HandyHand width={24} height={24} />
          <p className="text-sm font-medium">General</p>
        </div>
        <div className="flex gap-2 items-center p-2 w-full rounded-lg hover:bg-logo-primary/50 hover:cursor-pointer">
          <Cog width={24} height={24} />
          <p className="text-sm font-semibold">Advanced</p>
        </div>
        <div className="flex gap-2 items-center p-2 w-full rounded-lg  hover:bg-logo-primary/50 hover:cursor-pointer">
          <History width={24} height={24} />
          <p className="text-sm font-medium">History</p>
        </div>
        <div className="flex gap-2 items-center p-2 w-full rounded-lg  hover:bg-logo-primary/50 hover:cursor-pointer">
          <FlaskConical width={24} height={24} />
          <p className="text-sm font-medium">Debug</p>
        </div>
        <div className="flex gap-2 items-center p-2 w-full rounded-lg  hover:bg-logo-primary/50 hover:cursor-pointer">
          <Info width={24} height={24} />
          <p className="text-sm font-medium">About</p>
        </div>
      </div>
    </div>
  );
};

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
        <div className="flex flex-col items-center p-4 gap-4 flex-1">
          <HandyTextLogo width={200} />
          <Onboarding onModelSelected={handleModelSelected} />
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen flex flex-col w-full">
      <Toaster />
      <div className="flex">
        {/*<Option1 />*/}
        <Option2 />
        <div className="flex flex-col items-center p-4 gap-4 flex-1">
          {/*<HandyTextLogo width={200} />*/}
          <AccessibilityPermissions />
          <Settings />
        </div>
      </div>
      <Footer />
    </div>
  );
}

export default App;
