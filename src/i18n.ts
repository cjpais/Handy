import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "./locales/en.json";
import fr from "./locales/fr.json";

const systemLang = navigator.language.startsWith("fr") ? "fr" : "en";
const savedLang = localStorage.getItem("lang");

i18n
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: en },
      fr: { translation: fr },
    },
    lng: savedLang || systemLang,  // 👈 Détection automatique
    fallbackLng: "en",
    interpolation: { escapeValue: false },
  });

export default i18n;
