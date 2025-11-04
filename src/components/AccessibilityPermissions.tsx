import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Button } from "./ui/Button";
import {
  checkAccessibilityPermission,
  requestAccessibilityPermission,
} from "tauri-plugin-macos-permissions-api";

interface AccessibilityPermissionsProps {
  // Callback appelé lorsque les permissions sont accordées
  onGranted?: () => void;
  // Pour forcer l'affichage du prompt
  forceShow?: boolean;
}

const AccessibilityPermissions: React.FC<AccessibilityPermissionsProps> = ({
  onGranted,
  forceShow = false,
}) => {
  const { t } = useTranslation();
  const [isChecking, setIsChecking] = useState<boolean>(false);
  const [showPrompt, setShowPrompt] = useState<boolean>(forceShow);
  const [hasAccessibility, setHasAccessibility] = useState<boolean>(false);

  // Vérifier les permissions sans demander
  const checkPermissions = async (): Promise<boolean> => {
    try {
      const hasPermissions = await checkAccessibilityPermission();
      setHasAccessibility(hasPermissions);
      if (hasPermissions && onGranted) {
        onGranted();
      }
      return hasPermissions;
    } catch (error) {
      console.error("Error checking accessibility permissions:", error);
      return false;
    }
  };

  // Gérer la demande de permission
  const handleRequestPermission = async (): Promise<void> => {
    if (isChecking) return;
    
    setIsChecking(true);
    try {
      // D'abord vérifier si on a déjà la permission
      const hasPermission = await checkPermissions();
      
      if (!hasPermission) {
        // Si on n'a pas la permission, on la demande
        const granted = await requestAccessibilityPermission();
        if (granted) {
          await checkPermissions();
        }
      }
    } catch (error) {
      console.error("Error requesting accessibility permissions:", error);
    } finally {
      setIsChecking(false);
    }
  };

  // Si on a déjà les permissions, on n'affiche rien
  if (hasAccessibility && !forceShow) {
    return null;
  }

  // Si on ne doit pas afficher le prompt, on n'affiche rien
  if (!showPrompt && !forceShow) {
    return null;
  }

  return (
    <div className="p-4 w-full rounded-lg border border-mid-gray bg-white dark:bg-gray-800 shadow-sm">
      <div className="flex flex-col sm:flex-row justify-between items-start sm:items-center gap-4">
        <div className="flex-1">
          <h3 className="font-medium text-gray-900 dark:text-white">
            {t("accessibility.title", "Autorisations requises")}
          </h3>
          <p className="text-sm text-gray-600 dark:text-gray-300 mt-1">
            {t(
              "accessibility.message",
              "Certaines fonctionnalités de Handy nécessitent des autorisations d'accessibilité pour fonctionner correctement."
            )}
          </p>
          <details className="mt-2 text-xs text-gray-500 dark:text-gray-400">
            <summary className="cursor-pointer hover:text-gray-700 dark:hover:text-gray-200">
              {t("accessibility.why_needed", "Pourquoi ces autorisations sont-elles nécessaires ?")}
            </summary>
            <p className="mt-1">
              {t(
                "accessibility.explanation",
                "Ces autorisations sont nécessaires pour permettre à Handy d'interagir avec d'autres applications et de fournir des fonctionnalités avancées comme la lecture et la saisie de texte dans d'autres applications."
              )}
            </p>
          </details>
        </div>
        <div className="shrink-0">
          <Button
            onClick={handleRequestPermission}
            disabled={isChecking}
            className="whitespace-nowrap"
          >
            {isChecking
              ? t("accessibility.checking", "Vérification...")
              : t("accessibility.grant", "Autoriser l'accès")}
          </Button>
        </div>
      </div>
      
      {!forceShow && (
        <div className="mt-3 pt-3 border-t border-gray-100 dark:border-gray-700 flex justify-end">
          <button
            onClick={() => setShowPrompt(false)}
            className="text-xs text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200"
          >
            {t("common.later", "Plus tard")}
          </button>
        </div>
      )}
    </div>
  );
};

// Fonction utilitaire pour vérifier les permissions depuis n'importe où
export const checkAccessibility = async (): Promise<boolean> => {
  try {
    return await checkAccessibilityPermission();
  } catch (error) {
    console.error("Error checking accessibility:", error);
    return false;
  }
};

// Fonction utilitaire pour demander les permissions
export const requestAccessibility = async (): Promise<boolean> => {
  try {
    const result = await requestAccessibilityPermission();
    return result === true;
  } catch (error) {
    console.error("Error requesting accessibility:", error);
    return false;
  }
};

export default AccessibilityPermissions;
