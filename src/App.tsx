import { useEffect, useState, useCallback } from "react";
import { Toaster } from "sonner";
import "./App.css";
import AccessibilityPermissions from "./components/AccessibilityPermissions";
import { AuthScreen, AuthMode } from "./components/auth";
import Footer from "./components/footer";
import Onboarding from "./components/onboarding";
import { Sidebar, SidebarSection, SECTIONS_CONFIG } from "./components/Sidebar";
import { useSettings } from "./hooks/useSettings";
import { useSidecarStore } from "./stores/sidecarStore";
import { useAuth } from "./hooks/useAuth";
import { commands } from "@/bindings";

const renderSettingsContent = (section: SidebarSection) => {
  const ActiveComponent =
    SECTIONS_CONFIG[section]?.component || SECTIONS_CONFIG.general.component;
  return <ActiveComponent />;
};

function App() {
  const [authMode, setAuthMode] = useState<AuthMode | "loading">("loading");
  const [showOnboarding, setShowOnboarding] = useState<boolean | null>(null);
  const [currentSection, setCurrentSection] =
    useState<SidebarSection>("general");
  const { settings, updateSetting } = useSettings();
  const initializeSidecar = useSidecarStore((state) => state.initialize);
  const cleanupSidecar = useSidecarStore((state) => state.cleanup);
  const { isAuthenticated, isLoading: authLoading } = useAuth();

  // Notify backend of section changes for context-aware shortcuts
  const handleSectionChange = useCallback((section: SidebarSection) => {
    setCurrentSection(section);
    commands.setActiveUiSection(section).catch((err) => {
      console.error("Failed to set active UI section:", err);
    });
  }, []);

  useEffect(() => {
    // Initialize sidecar store for global state management
    initializeSidecar();
    return () => {
      cleanupSidecar();
    };
  }, [initializeSidecar, cleanupSidecar]);

  // Check auth status when auth loading completes
  useEffect(() => {
    if (!authLoading) {
      checkAuthStatus();
    }
  }, [authLoading, isAuthenticated]);

  // Check for stored auth mode on startup
  const checkAuthStatus = async () => {
    try {
      const storedAuthMode = localStorage.getItem("auth_mode") as AuthMode | null;

      // If they previously chose "signed_in", verify they're still authenticated
      if (storedAuthMode === "signed_in") {
        // Wait for auth hook to finish loading
        if (authLoading) return;

        if (isAuthenticated) {
          setAuthMode("signed_in");
          checkOnboardingStatus();
        } else {
          // Session expired or logged out - show auth screen again
          localStorage.removeItem("auth_mode");
          setAuthMode(null);
        }
      } else if (storedAuthMode) {
        // Ghost or Guest mode - just restore it
        setAuthMode(storedAuthMode);
        checkOnboardingStatus();
      } else {
        // No stored mode - show auth screen
        setAuthMode(null);
      }
    } catch (error) {
      console.error("Failed to check auth status:", error);
      setAuthMode(null);
    }
  };

  const handleAuthComplete = (mode: AuthMode) => {
    setAuthMode(mode);
    // Store auth mode for future sessions
    if (mode) {
      localStorage.setItem("auth_mode", mode);
    }
    checkOnboardingStatus();
  };

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
      const result = await commands.hasAnyModelsAvailable();
      if (result.status === "ok") {
        setShowOnboarding(!result.data);
      } else {
        setShowOnboarding(true);
      }
    } catch (error) {
      console.error("Failed to check onboarding status:", error);
      setShowOnboarding(true);
    }
  };

  const handleModelSelected = () => {
    // Transition to main app - user has started a download
    setShowOnboarding(false);
  };

  // Show loading state while checking auth status
  if (authMode === "loading") {
    return (
      <div className="h-screen w-screen flex items-center justify-center">
        <div className="w-8 h-8 border-2 border-logo-primary border-t-transparent rounded-full animate-spin" />
      </div>
    );
  }

  // Show auth screen if user hasn't authenticated yet
  if (authMode === null) {
    return <AuthScreen onAuthComplete={handleAuthComplete} />;
  }

  // Show loading state while checking onboarding status
  if (showOnboarding === null) {
    return (
      <div className="h-screen w-screen flex items-center justify-center">
        <div className="w-8 h-8 border-2 border-logo-primary border-t-transparent rounded-full animate-spin" />
      </div>
    );
  }

  if (showOnboarding) {
    return <Onboarding onModelSelected={handleModelSelected} />;
  }

  return (
    <div className="h-screen flex flex-col select-none cursor-default">
      <Toaster
        theme="system"
        toastOptions={{
          unstyled: true,
          classNames: {
            toast:
              "bg-background border border-mid-gray/20 rounded-lg shadow-lg px-4 py-3 flex items-center gap-3 text-sm",
            title: "font-medium",
            description: "text-mid-gray",
          },
        }}
      />
      {/* Main content area that takes remaining space */}
      <div className="flex-1 flex overflow-hidden">
        <Sidebar
          activeSection={currentSection}
          onSectionChange={handleSectionChange}
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
