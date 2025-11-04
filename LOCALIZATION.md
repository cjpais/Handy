# Guide d'internationalisation pour Handy

Ce document explique comment ajouter de nouvelles langues à l'application Handy. Il est destiné aux développeurs qui souhaitent contribuer à l'internationalisation du projet.

## Table des matières

1. [Structure de l'internationalisation](#structure-de-linternationalisation)
2. [Ajouter une nouvelle langue](#ajouter-une-nouvelle-langue)
3. [Tester les traductions](#tester-les-traductions)
4. [Bonnes pratiques](#bonnes-pratiques)
5. [Ressources utiles](#ressources-utiles)

## Structure de l'internationalisation

Handy utilise [i18next](https://www.i18next.com/) avec [react-i18next](https://react.i18next.com/) pour gérer les traductions. La structure est organisée comme suit :

- **`src/locales/`** : Répertoire contenant les fichiers de traduction
  - `en.json` : Traductions anglaises (langue par défaut)
  - `fr.json` : Traductions françaises
  - `es.json` : Traductions espagnoles
  - *Ajoutez ici vos nouveaux fichiers de langue*

- **`src/i18n.ts`** : Configuration de i18next

- **`src/lib/constants/uiLanguage.ts`** : Définition des langues supportées et fonctions utilitaires

## Ajouter une nouvelle langue

Pour ajouter une nouvelle langue à Handy, suivez ces étapes :

### 1. Créer un fichier de traduction

Créez un nouveau fichier JSON dans le répertoire `src/locales/` avec le code ISO de la langue (par exemple, `de.json` pour l'allemand ou `it.json` pour l'italien).

Copiez la structure du fichier `en.json` et traduisez chaque valeur dans la nouvelle langue. Assurez-vous de conserver exactement la même structure de clés.

```json
{
  "app": {
    "title": "Handy",
    "subtitle": "Herramienta de reconocimiento de voz gratuita, sin conexión y de código abierto"
  },
  ...
}
```

### 2. Mettre à jour la configuration i18next

Modifiez le fichier `src/i18n.ts` pour inclure la nouvelle langue :

```typescript
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "./locales/en.json";
import fr from "./locales/fr.json";
import es from "./locales/es.json";
import de from "./locales/de.json"; // Importez le nouveau fichier

i18n
  .use(initReactI18next)
  .init({
    fallbackLng: "en",
    supportedLngs: ["en", "fr", "es", "de"], // Ajoutez le code de la nouvelle langue
    defaultNS: "translation",
    resources: {
      en: { translation: en },
      fr: { translation: fr },
      es: { translation: es },
      de: { translation: de }, // Ajoutez la nouvelle ressource
    },
    interpolation: {
      escapeValue: false,
    },
  });

i18n.on("languageChanged", (lng: string) => {
  document.documentElement.lang = lng;
});

export default i18n;
```

### 3. Mettre à jour les constantes de langue

Modifiez le fichier `src/lib/constants/uiLanguage.ts` pour inclure la nouvelle langue :

```typescript
export type UILanguage = "en" | "fr" | "es" | "de"; // Ajoutez le code de la nouvelle langue

export const SUPPORTED_UI_LANGUAGES: UILanguage[] = ["en", "fr", "es", "de"]; // Ajoutez le code de la nouvelle langue

// Mettez à jour la fonction de normalisation si nécessaire
export const normalizeUiLanguage = (
  value: string | null | undefined,
): UILanguage => {
  if (!value) {
    return "en";
  }

  const normalized = value.toLowerCase().slice(0, 2);
  if (normalized === "fr") return "fr";
  if (normalized === "es") return "es";
  if (normalized === "de") return "de"; // Ajoutez la condition pour la nouvelle langue
  return "en";
};
```

### 4. Mettre à jour le composant de sélection de langue

Assurez-vous que la nouvelle langue apparaît dans le composant `LanguageSetup.tsx` :

```typescript
// Dans src/components/onboarding/LanguageSetup.tsx
type SupportedLanguage = "en" | "fr" | "es" | "de"; // Ajoutez le code de la nouvelle langue

const AVAILABLE_LANGUAGES: SupportedLanguage[] = ["en", "fr", "es", "de"]; // Ajoutez le code de la nouvelle langue
```

### 5. Ajouter les traductions pour le sélecteur de langue

Dans les fichiers de traduction existants (`en.json`, `fr.json`), ajoutez les entrées pour la nouvelle langue dans la section `language_setup.languages` :

```json
"language_setup": {
  "languages": {
    "en": { ... },
    "fr": { ... },
    "es": {
      "label": "Español",
      "description": "Use Handy in Spanish." // En anglais
    }
  }
}
```

Et dans le fichier `fr.json` :

```json
"language_setup": {
  "languages": {
    "en": { ... },
    "fr": { ... },
    "es": {
      "label": "Español",
      "description": "Utiliser Handy en espagnol." // En français
    }
  }
}
```

Dans votre nouveau fichier de langue (par exemple, `es.json`), ajoutez également les entrées pour toutes les langues supportées :

```json
"language_setup": {
  "languages": {
    "en": {
      "label": "English",
      "description": "Usar Handy en inglés." // En espagnol
    },
    "fr": {
      "label": "Français",
      "description": "Usar Handy en francés." // En espagnol
    },
    "es": {
      "label": "Español",
      "description": "Usar Handy en español." // En espagnol
    }
  }
}
```

## Tester les traductions

Pour tester vos traductions :

1. Lancez l'application en mode développement :
   ```bash
   pnpm tauri dev
   ```

2. Changez la langue dans l'interface utilisateur et vérifiez que tous les textes sont correctement traduits.

3. Vérifiez particulièrement :
   - Les messages d'erreur
   - Les tooltips
   - Les éléments dynamiques
   - Les formats de date et d'heure

## Bonnes pratiques

### Utilisation des variables

Lorsque vous utilisez des variables dans les traductions, utilisez la syntaxe `{{variable}}` :

```json
{
  "greeting": "Bonjour {{name}} !"
}
```

Et dans le code React :

```tsx
t("greeting", { name: "John" })
```

### Pluralisation

Pour gérer la pluralisation, utilisez la syntaxe suivante :

```json
{
  "items": "{{count}} élément",
  "items_plural": "{{count}} éléments"
}
```

Et dans le code React :

```tsx
t("items", { count: 5 })
```

### Textes longs

Pour les textes longs, utilisez des clés hiérarchiques pour une meilleure organisation :

```json
{
  "help": {
    "title": "Aide",
    "description": "Voici comment utiliser l'application...",
    "sections": {
      "getting_started": {
        "title": "Démarrage",
        "content": "Pour commencer..."
      }
    }
  }
}
```

## Ressources utiles

- [Documentation i18next](https://www.i18next.com/)
- [Documentation react-i18next](https://react.i18next.com/)
- [ISO 639-1 Language Codes](https://en.wikipedia.org/wiki/List_of_ISO_639-1_codes)
- [Bonnes pratiques d'internationalisation](https://phrase.com/blog/posts/react-i18n-best-practices/)

---

# Handy Localization Guide

This document explains how to add new languages to the Handy application. It is intended for developers who want to contribute to the internationalization of the project.

## Table of Contents

1. [Internationalization Structure](#internationalization-structure)
2. [Adding a New Language](#adding-a-new-language)
3. [Testing Translations](#testing-translations)
4. [Best Practices](#best-practices)
5. [Useful Resources](#useful-resources)

## Internationalization Structure

Handy uses [i18next](https://www.i18next.com/) with [react-i18next](https://react.i18next.com/) to manage translations. The structure is organized as follows:

- **`src/locales/`**: Directory containing translation files
  - `en.json`: English translations (default language)
  - `fr.json`: French translations
  - *Add your new language files here*

- **`src/i18n.ts`**: i18next configuration

- **`src/lib/constants/uiLanguage.ts`**: Definition of supported languages and utility functions

## Adding a New Language

To add a new language to Handy, follow these steps:

### 1. Create a Translation File

Create a new JSON file in the `src/locales/` directory with the ISO code of the language (e.g., `es.json` for Spanish).

Copy the structure of the `en.json` file and translate each value into the new language. Make sure to keep exactly the same key structure.

```json
{
  "app": {
    "title": "Handy",
    "subtitle": "Free, offline, open source speech-to-text tool"
  },
  ...
}
```

### 2. Update i18next Configuration

Modify the `src/i18n.ts` file to include the new language:

```typescript
import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "./locales/en.json";
import fr from "./locales/fr.json";
import es from "./locales/es.json"; // Import the new file

i18n
  .use(initReactI18next)
  .init({
    fallbackLng: "en",
    supportedLngs: ["en", "fr", "es"], // Add the code of the new language
    defaultNS: "translation",
    resources: {
      en: { translation: en },
      fr: { translation: fr },
      es: { translation: es }, // Add the new resource
    },
    interpolation: {
      escapeValue: false,
    },
  });

i18n.on("languageChanged", (lng: string) => {
  document.documentElement.lang = lng;
});

export default i18n;
```

### 3. Update Language Constants

Modify the `src/lib/constants/uiLanguage.ts` file to include the new language:

```typescript
export type UILanguage = "en" | "fr" | "es"; // Add the code of the new language

export const SUPPORTED_UI_LANGUAGES: UILanguage[] = ["en", "fr", "es"]; // Add the code of the new language

// Update the normalization function if needed
export const normalizeUiLanguage = (
  value: string | null | undefined,
): UILanguage => {
  if (!value) {
    return "en";
  }

  const normalized = value.toLowerCase().slice(0, 2);
  if (normalized === "fr") return "fr";
  if (normalized === "es") return "es"; // Add the condition for the new language
  return "en";
};
```

### 4. Update the Language Selection Component

Make sure the new language appears in the `LanguageSetup.tsx` component:

```typescript
// In src/components/onboarding/LanguageSetup.tsx
type SupportedLanguage = "en" | "fr" | "es"; // Add the code of the new language

const AVAILABLE_LANGUAGES: SupportedLanguage[] = ["en", "fr", "es"]; // Add the code of the new language
```

### 5. Add Translations for the Language Selector

In the existing translation files (`en.json`, `fr.json`), add entries for the new language in the `language_setup.languages` section:

```json
"language_setup": {
  "languages": {
    "en": { ... },
    "fr": { ... },
    "es": {
      "label": "Español",
      "description": "Use Handy in Spanish." // In English
    }
  }
}
```

And in the `fr.json` file:

```json
"language_setup": {
  "languages": {
    "en": { ... },
    "fr": { ... },
    "es": {
      "label": "Español",
      "description": "Utiliser Handy en espagnol." // In French
    }
  }
}
```

In your new language file (e.g., `es.json`), also add entries for all supported languages:

```json
"language_setup": {
  "languages": {
    "en": {
      "label": "English",
      "description": "Usar Handy en inglés." // In Spanish
    },
    "fr": {
      "label": "Français",
      "description": "Usar Handy en francés." // In Spanish
    },
    "es": {
      "label": "Español",
      "description": "Usar Handy en español." // In Spanish
    }
  }
}
```

## Testing Translations

To test your translations:

1. Launch the application in development mode:
   ```bash
   pnpm tauri dev
   ```

2. Change the language in the user interface and verify that all texts are correctly translated.

3. Particularly check:
   - Error messages
   - Tooltips
   - Dynamic elements
   - Date and time formats

## Best Practices

### Using Variables

When using variables in translations, use the `{{variable}}` syntax:

```json
{
  "greeting": "Hello {{name}}!"
}
```

And in React code:

```tsx
t("greeting", { name: "John" })
```

### Pluralization

To handle pluralization, use the following syntax:

```json
{
  "items": "{{count}} item",
  "items_plural": "{{count}} items"
}
```

And in React code:

```tsx
t("items", { count: 5 })
```

### Long Texts

For long texts, use hierarchical keys for better organization:

```json
{
  "help": {
    "title": "Help",
    "description": "Here's how to use the application...",
    "sections": {
      "getting_started": {
        "title": "Getting Started",
        "content": "To get started..."
      }
    }
  }
}
```

## Useful Resources

- [i18next Documentation](https://www.i18next.com/)
- [react-i18next Documentation](https://react.i18next.com/)
- [ISO 639-1 Language Codes](https://en.wikipedia.org/wiki/List_of_ISO_639-1_codes)
- [Internationalization Best Practices](https://phrase.com/blog/posts/react-i18n-best-practices/)
