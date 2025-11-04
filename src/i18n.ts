import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "./locales/en.json";
import fr from "./locales/fr.json";

i18n
  .use(initReactI18next)
  .init({
    fallbackLng: "en",
    supportedLngs: ["en", "fr"],
    defaultNS: "translation",
    resources: {
      en: { translation: en },
      fr: { translation: fr },
    },
    interpolation: {
      escapeValue: false,
    },
  });

i18n.on("languageChanged", (lng: string) => {
  document.documentElement.lang = lng;
});

export default i18n;
