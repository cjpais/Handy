// src/i18n.ts
import i18n from "i18next";
import { initReactI18next } from "react-i18next";

i18n
  .use(initReactI18next)
  .init({
    fallbackLng: "en",
    supportedLngs: ["en", "fr"],
    resources: {
      en: {
        translation: {
          "hello": "Hello",
          "start": "Start",
          "stop": "Stop",
          "settings": "Settings",
        },
      },
      fr: {
        translation: {
          "hello": "Bonjour",
          "start": "Démarrer",
          "stop": "Arrêter",
          "settings": "Paramètres",
        },
      },
    },
    interpolation: {
      escapeValue: false,
    },
  });

// 🔁 Mise à jour automatique de la langue
i18n.on("languageChanged", (lng: string) => {
  document.documentElement.lang = lng;
});

export default i18n;
