import { useTranslation } from 'react-i18next';
import { supportedLanguages, SupportedLanguage } from '@/i18n/config';
import { useSettingsStore } from '@/stores/settingsStore';
import { SettingContainer } from '@/components/ui/SettingContainer';
import { useState } from 'react';

export function UiLanguageSelector() {
  const { i18n } = useTranslation();
  const { settings, updateSetting, isUpdatingKey } = useSettingsStore();
  const [isChanging, setIsChanging] = useState(false);

  const currentLanguage = settings?.ui_language || 'en';
  const isLoading = isUpdatingKey('ui_language') || isChanging;

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

  return (
    <SettingContainer
      title={i18n.t('settings.general.uiLanguage.title')}
      description={i18n.t('settings.general.uiLanguage.description')}
    >
      <div className="flex flex-col space-y-2">
        {Object.entries(supportedLanguages).map(([code, name]) => (
          <label
            key={code}
            className={`
              flex items-center space-x-3 p-3 rounded-lg border cursor-pointer
              transition-colors duration-200
              ${currentLanguage === code
                ? 'bg-blue-50 border-blue-200 text-blue-900 dark:bg-blue-900/20 dark:border-blue-800 dark:text-blue-100'
                : 'bg-gray-50 border-gray-200 hover:bg-gray-100 dark:bg-gray-800 dark:border-gray-700 dark:hover:bg-gray-700'
              }
              ${isLoading ? 'cursor-not-allowed opacity-50' : ''}
            `}
          >
            <input
              type="radio"
              name="ui-language"
              value={code}
              checked={currentLanguage === code}
              onChange={() => handleLanguageChange(code)}
              disabled={isLoading}
              className="w-4 h-4 text-blue-600 border-gray-300 focus:ring-blue-500 dark:focus:ring-blue-600 dark:ring-offset-gray-800 focus:ring-2 dark:bg-gray-700 dark:border-gray-600"
            />
            <span className="text-sm font-medium">{name}</span>
            {currentLanguage === code && (
              <span className="ml-auto text-xs text-blue-600 dark:text-blue-400">
                ✓ {i18n.t('common.ok')}
              </span>
            )}
          </label>
        ))}
      </div>
    </SettingContainer>
  );
}