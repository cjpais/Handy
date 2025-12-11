# Contributing Translations to Handy

Thank you for helping translate Handy! This guide explains how to add or improve translations.

## Quick Start

1. Fork the repository
2. Copy the English translation file to your language folder
3. Translate the values (not the keys!)
4. Submit a pull request

## File Structure

Translation files are located in:
```
src/i18n/locales/
├── en/
│   └── translation.json    # English (source)
├── vi/
│   └── translation.json    # Vietnamese
├── fr/
│   └── translation.json    # French
└── [your-language]/
    └── translation.json    # Your contribution!
```

## Adding a New Language

### Step 1: Create the Language Folder

Create a new folder using the [ISO 639-1 language code](https://en.wikipedia.org/wiki/List_of_ISO_639-1_codes):

```bash
mkdir src/i18n/locales/[language-code]
```

Examples:
- `de` for German
- `es` for Spanish
- `ja` for Japanese
- `zh` for Chinese
- `ko` for Korean
- `pt` for Portuguese

### Step 2: Copy the English File

```bash
cp src/i18n/locales/en/translation.json src/i18n/locales/[language-code]/translation.json
```

### Step 3: Translate the Values

Open the file and translate only the **values** (right side), not the keys (left side):

```json
{
  "sidebar": {
    "general": "General",      // ← Translate this value
    "advanced": "Advanced",    // ← Translate this value
    ...
  }
}
```

**Important:**
- Keep all keys exactly the same
- Preserve any `{{variables}}` in the text (e.g., `{{error}}`, `{{model}}`)
- Keep the JSON structure and formatting intact

### Step 4: Register Your Language

Edit `src/i18n/languages.ts` and add your language metadata:

```typescript
export const LANGUAGE_METADATA: Record<string, { name: string; nativeName: string }> = {
  en: { name: "English", nativeName: "English" },
  es: { name: "Spanish", nativeName: "Español" },
  fr: { name: "French", nativeName: "Français" },
  vi: { name: "Vietnamese", nativeName: "Tiếng Việt" },
  de: { name: "German", nativeName: "Deutsch" }, // ← Add your language
};
```

That's it! The translation files are automatically discovered by Vite.

### Step 5: Test Your Translation

1. Run the app: `bun run tauri dev`
2. Go to Settings → General → App Language
3. Select your language
4. Verify all text displays correctly

### Step 6: Submit a Pull Request

1. Commit your changes
2. Push to your fork
3. Open a pull request with:
   - Language name in the title (e.g., "Add German translation")
   - Any notes about the translation

## Improving Existing Translations

Found a typo or better translation?

1. Edit the relevant `translation.json` file
2. Submit a PR with a brief description of the change

## Translation Guidelines

### Do:
- Use natural, native-sounding language
- Keep translations concise (UI space is limited)
- Match the tone of the English text (friendly, clear)
- Preserve technical terms when appropriate (e.g., "API", "GPU")

### Don't:
- Translate brand names (Handy, Whisper.cpp, OpenAI)
- Change or remove `{{variables}}`
- Modify JSON keys
- Add extra spaces or formatting

### Handling Variables

Some strings contain variables like `{{error}}` or `{{model}}`. Keep these exactly as-is:

```json
// English
"downloadModel": "Failed to download model: {{error}}"

// French (correct)
"downloadModel": "Échec du téléchargement du modèle : {{error}}"

// French (incorrect - don't translate the variable!)
"downloadModel": "Échec du téléchargement du modèle : {{erreur}}"
```

### Handling Plurals

Some languages have complex plural rules. For now, use a general form that works for all cases. We may add proper plural support in the future.

## Questions?

- Open an issue on GitHub
- Join the discussion in existing translation PRs

## Currently Supported Languages

| Language | Code | Status |
|----------|------|--------|
| English | `en` | Complete (source) |
| Spanish | `es` | Complete |
| French | `fr` | Complete |
| Vietnamese | `vi` | Complete |

## Requested Languages

We'd love help with:
- German (`de`)
- Japanese (`ja`)
- Chinese (`zh`)
- Korean (`ko`)
- Portuguese (`pt`)
- And more!

---

Thank you for making Handy accessible to more users around the world! 🌍
