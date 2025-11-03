import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "./locales/en.json";
import fr from "./locales/fr.json";

// 🔍 Détection automatique de la langue du navigateur
const browserLang = navigator.language.split("-")[0]; // ex: "fr-FR" → "fr"
const savedLang = localStorage.getItem("lang");
const defaultLang = savedLang || (["fr", "en"].includes(browserLang) ? browserLang : "fr");

i18n
  .use(initReactI18next)
  .init({
    resources: {
      en: { translation: en },
      fr: { translation: fr },
    },
    lng: defaultLang,
    fallbackLng: "fr", // 👈 français par défaut
    interpolation: { escapeValue: false },
    detection: { order: ["localStorage", "navigator"] },
  });

// 🧠 Sauvegarde automatique de la préférence langue
i18n.on("languageChanged", (lng) => {
  localStorage.setItem("lang", lng);
});

export default i18n;
