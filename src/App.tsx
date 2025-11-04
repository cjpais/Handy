import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";
import { Toaster } from "sonner";
import "./App.css";
import AccessibilityPermissions from "./components/AccessibilityPermissions";
import Footer from "./components/footer";
import Onboarding from "./components/onboarding";
import { Sidebar, SidebarSection, SECTIONS_CONFIG } from "./components/Sidebar";
import { useSettings } from "./hooks/useSettings";
import i18n from "./i18n";
import LanguageSetup from "./components/onboarding/LanguageSetup";
import {
  UILanguage,
  normalizeUiLanguage,
  getStoredUiLanguage,
  setStoredUiLanguage,
} from "./lib/constants/uiLanguage";


const renderSettingsContent = (section: SidebarSection) => {
  const ActiveComponent =
    SECTIONS_CONFIG[section]?.component || SECTIONS_CONFIG.general.component;
  return <ActiveComponent />;
};

function App() {
  const [languageReady, setLanguageReady] = useState(false);
  const [shouldShowLanguageSetup, setShouldShowLanguageSetup] =
    useState(false);
  const [languageFallback, setLanguageFallback] = useState<UILanguage>("en");
  const [showOnboarding, setShowOnboarding] = useState<boolean | null>(null);
  const [currentSection, setCurrentSection] =
    useState<SidebarSection>("general");
  const { settings, updateSetting } = useSettings();

  useEffect(() => {
    // Vérifier si c'est le premier démarrage
    const isFirstLaunch = localStorage.getItem('handy_first_launch') !== 'false';
    const stored = getStoredUiLanguage();

    if (stored && !isFirstLaunch) {
      // Si la langue est déjà stockée et ce n'est pas le premier démarrage
      i18n.changeLanguage(stored);
      setLanguageFallback(stored);
      setLanguageReady(true);
      setShouldShowLanguageSetup(false);
    } else {
      // Premier démarrage ou langue non définie
      const fallback = normalizeUiLanguage(navigator.language);
      setLanguageFallback(fallback);
      setShouldShowLanguageSetup(true);
      i18n.changeLanguage(fallback);
    }
  }, []);

  useEffect(() => {
    if (!languageReady) {
      return;
    }
    checkOnboardingStatus();
  }, [languageReady]);

  // Handle keyboard shortcuts for debug mode toggle
  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      // Check for Ctrl+Shift+D (Windows/Linux) or Cmd+Shift+D (macOS)
      const isDebugShortcut =
        event.shiftKey &&
        event.key.toLowerCase() === "d" &&
        (event.ctrlKey || event.metaKey);

      if (isDebugShortcut) {
        event.preventDefault();
        const currentDebugMode = settings?.debug_mode ?? false;
        updateSetting("debug_mode", !currentDebugMode);
      }
    };

    // Add event listener when component mounts
    document.addEventListener("keydown", handleKeyDown);

    // Cleanup event listener when component unmounts
    return () => {
      document.removeEventListener("keydown", handleKeyDown);
    };
  }, [settings?.debug_mode, updateSetting]);

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

  const handleLanguageSelected = async (language: UILanguage) => {
    // Changer la langue dans i18n
    i18n.changeLanguage(language);
    // Stocker la langue sélectionnée
    setStoredUiLanguage(language);
    // Marquer que la langue est prête
    setLanguageReady(true);
    // Marquer que ce n'est plus le premier démarrage
    localStorage.setItem('handy_first_launch', 'false');
    // Masquer l'écran de sélection de langue
    setShouldShowLanguageSetup(false);
  };

  if (shouldShowLanguageSetup) {
    return (
      <LanguageSetup
        defaultLanguage={languageFallback}
        onSelect={handleLanguageSelected}
      />
    );
  }

  if (!languageReady) {
    return null;
  }

  if (showOnboarding) {
    return <Onboarding onModelSelected={handleModelSelected} />;
  }

  return (
    <div className="h-screen flex flex-col">
      <Toaster />
      {/* Main content area that takes remaining space */}
      <div className="flex-1 flex overflow-hidden">
        <Sidebar
          activeSection={currentSection}
          onSectionChange={setCurrentSection}
        />
        {/* Scrollable content area */}
        <div className="flex-1 flex flex-col overflow-hidden">
          <div className="flex-1 overflow-y-auto">
            <div className="flex flex-col items-center p-4 gap-4">
              <AccessibilityPermissions />
              {renderSettingsContent(currentSection)}
            </div>
          </div>
        </div>
      </div>
      {/* Fixed footer at bottom */}
      <Footer />
    </div>
  );
}

export default App;
