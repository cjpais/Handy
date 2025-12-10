import { useEffect, useState } from 'react';
import { I18nextProvider } from 'react-i18next';
import { useSettingsStore } from '@/stores/settingsStore';
import i18n from './config';
import { SupportedLanguage } from './config';

interface I18nProviderProps {
  children: React.ReactNode;
}

export function I18nProvider({ children }: I18nProviderProps) {
  const { settings } = useSettingsStore();
  const [isInitialized, setIsInitialized] = useState(false);

  useEffect(() => {
    // Initialize language from settings
    if (settings?.ui_language) {
      const language = settings.ui_language as SupportedLanguage;
      i18n.changeLanguage(language).catch((error) => {
        console.error('Failed to change language:', error);
      });
    }
    setIsInitialized(true);
  }, [settings?.ui_language]);

  // Don't render children until i18n is initialized
  if (!isInitialized) {
    return null;
  }

  return (
    <I18nextProvider i18n={i18n}>
      {children}
    </I18nextProvider>
  );
}