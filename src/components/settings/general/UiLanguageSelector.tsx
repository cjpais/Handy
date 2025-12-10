import { useTranslation } from 'react-i18next';
import { supportedLanguages, SupportedLanguage } from '@/i18n/config';
import { useSettingsStore } from '@/stores/settingsStore';
import { SettingContainer } from '@/components/ui/SettingContainer';
import { ResetButton } from '@/components/ui/ResetButton';
import { useState } from 'react';

export function UiLanguageSelector() {
  const { i18n } = useTranslation();
  const { settings, updateSetting, resetSetting, isUpdatingKey } = useSettingsStore();
  const [isChanging, setIsChanging] = useState(false);

  const currentLanguage = settings?.ui_language || 'en';
  const isLoading = isUpdatingKey('ui_language') || isChanging;

  const languageOptions = Object.entries(supportedLanguages).map(([code, name]) => ({
    value: code,
    label: name,
  }));

  const handleLanguageChange = async (language: string) => {
    if (language === currentLanguage || isLoading) return;

    setIsChanging(true);

    try {
      // Update settings
      await updateSetting('ui_language' as any, language);

      // Change i18n language
      await i18n.changeLanguage(language as SupportedLanguage);

    } catch (error) {
      console.error('Failed to change UI language:', error);
    } finally {
      setIsChanging(false);
    }
  };

  const handleReset = async () => {
    try {
      await resetSetting('ui_language' as any);
      // Reset to default language (English)
      await i18n.changeLanguage('en');
    } catch (error) {
      console.error('Failed to reset UI language:', error);
    }
  };

  const getSelectedLanguageName = () => {
    return supportedLanguages[currentLanguage as SupportedLanguage] || supportedLanguages.en;
  };

  return (
    <SettingContainer
      title={i18n.t('settings.general.uiLanguage.title')}
      description={i18n.t('settings.general.uiLanguage.description')}
    >
      <div className="flex items-center space-x-1">
        <div className="relative">
          <select
            value={currentLanguage}
            onChange={(e) => handleLanguageChange(e.target.value)}
            disabled={isLoading}
            className={`
              px-3 py-2 text-sm font-semibold bg-mid-gray/10 border border-mid-gray/80 rounded min-w-[140px] appearance-none cursor-pointer
              transition-all duration-150 pr-8
              ${isLoading
                ? "opacity-50 cursor-not-allowed"
                : "hover:bg-logo-primary/10 cursor-pointer hover:border-logo-primary"
              }
            `}
          >
            {languageOptions.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>

          {/* Custom dropdown arrow */}
          <div className="absolute inset-y-0 right-0 flex items-center pr-2 pointer-events-none">
            <svg
              className={`w-4 h-4 transition-transform duration-200 ${
                isLoading ? "opacity-50" : "text-mid-gray"
              }`}
              fill="none"
              stroke="currentColor"
              viewBox="0 0 24 24"
            >
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                strokeWidth={2}
                d="M19 9l-7 7-7-7"
              />
            </svg>
          </div>
        </div>

        <ResetButton
          onClick={handleReset}
          disabled={isLoading}
        />
      </div>

      {isLoading && (
        <div className="absolute inset-0 bg-mid-gray/10 rounded flex items-center justify-center">
          <div className="w-4 h-4 border-2 border-logo-primary border-t-transparent rounded-full animate-spin"></div>
        </div>
      )}
    </SettingContainer>
  );
}