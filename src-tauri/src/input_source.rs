use std::collections::HashMap;
use std::sync::LazyLock;

/// Maps keyboard input source IDs to ISO 639-1 language codes
static INPUT_SOURCE_MAP: LazyLock<HashMap<&'static str, &'static str>> = LazyLock::new(|| {
    let mut m = HashMap::new();
    // macOS input sources (TIS identifiers)
    m.insert("com.apple.keylayout.US", "en");
    m.insert("com.apple.keylayout.British", "en");
    m.insert("com.apple.keylayout.ABC", "en");
    m.insert("com.apple.keylayout.Australian", "en");
    m.insert("com.apple.keylayout.Russian", "ru");
    m.insert("com.apple.keylayout.Russian-PC", "ru");
    m.insert("com.apple.keylayout.RussianWin", "ru");
    m.insert("com.apple.keylayout.German", "de");
    m.insert("com.apple.keylayout.Spanish", "es");
    m.insert("com.apple.keylayout.Spanish-ISO", "es");
    m.insert("com.apple.keylayout.French", "fr");
    m.insert("com.apple.keylayout.Italian", "it");
    m.insert("com.apple.keylayout.Portuguese", "pt");
    m.insert("com.apple.keylayout.Brazilian", "pt");
    m.insert("com.apple.keylayout.Japanese", "ja");
    m.insert("com.apple.keylayout.Chinese", "zh");
    m.insert("com.apple.keylayout.SimplifiedChinese", "zh");
    m.insert("com.apple.keylayout.TraditionalChinese", "zh");
    m.insert("com.apple.keylayout.Korean", "ko");
    m.insert("com.apple.keylayout.Arabic", "ar");
    m.insert("com.apple.keylayout.Hebrew", "he");
    m.insert("com.apple.keylayout.Turkish", "tr");
    m.insert("com.apple.keylayout.Polish", "pl");
    m.insert("com.apple.keylayout.Dutch", "nl");
    m.insert("com.apple.keylayout.Ukrainian", "uk");
    m.insert("com.apple.keylayout.Greek", "el");
    m.insert("com.apple.keylayout.Swedish", "sv");
    m.insert("com.apple.keylayout.Norwegian", "no");
    m.insert("com.apple.keylayout.Danish", "da");
    m.insert("com.apple.keylayout.Finnish", "fi");
    m.insert("com.apple.keylayout.Czech", "cs");
    m.insert("com.apple.keylayout.Hungarian", "hu");
    m.insert("com.apple.keylayout.Romanian", "ro");
    m.insert("com.apple.keylayout.Thai", "th");
    m.insert("com.apple.keylayout.Vietnamese", "vi");
    m.insert("com.apple.keylayout.Hindi", "hi");
    m.insert("com.apple.keylayout.Indonesian", "id");
    m.insert("com.apple.keylayout.Malay", "ms");
    // macOS input method sources
    m.insert("com.apple.inputmethod.SCIM.ITABC", "zh");
    m.insert("com.apple.inputmethod.SCIM.Shuangpin", "zh");
    m.insert("com.apple.inputmethod.TCIM.Cangjie", "zh");
    m.insert("com.apple.inputmethod.Kotoeri.Japanese", "ja");
    m.insert("com.apple.inputmethod.Korean", "ko");
    // Windows KLID codes (hex keyboard layout IDs)
    m.insert("00000409", "en"); // US
    m.insert("00000809", "en"); // UK
    m.insert("00000419", "ru"); // Russian
    m.insert("00000407", "de"); // German
    m.insert("0000040a", "es"); // Spanish
    m.insert("0000040c", "fr"); // French
    m.insert("00000410", "it"); // Italian
    m.insert("00000816", "pt"); // Portuguese
    m.insert("00000416", "pt"); // Brazilian Portuguese
    m.insert("00000411", "ja"); // Japanese
    m.insert("00000804", "zh"); // Chinese Simplified
    m.insert("00000404", "zh"); // Chinese Traditional
    m.insert("00000412", "ko"); // Korean
    m.insert("00000401", "ar"); // Arabic
    m.insert("0000040d", "he"); // Hebrew
    m.insert("0000041f", "tr"); // Turkish
    m.insert("00000415", "pl"); // Polish
    m.insert("00000413", "nl"); // Dutch
    m.insert("00000422", "uk"); // Ukrainian
    m.insert("00000408", "el"); // Greek
    m.insert("0000041d", "sv"); // Swedish
    m.insert("00000414", "no"); // Norwegian
    m.insert("00000406", "da"); // Danish
    m.insert("0000040b", "fi"); // Finnish
    m.insert("00000405", "cs"); // Czech
    m.insert("0000040e", "hu"); // Hungarian
    m.insert("00000418", "ro"); // Romanian
    m.insert("0000041e", "th"); // Thai
    m.insert("0000042a", "vi"); // Vietnamese
    m.insert("00000439", "hi"); // Hindi
    m.insert("00000421", "id"); // Indonesian
    m.insert("0000043e", "ms"); // Malay
    // Linux XKB layouts
    m.insert("us", "en");
    m.insert("gb", "en");
    m.insert("ru", "ru");
    m.insert("de", "de");
    m.insert("es", "es");
    m.insert("fr", "fr");
    m.insert("it", "it");
    m.insert("pt", "pt");
    m.insert("br", "pt");
    m.insert("jp", "ja");
    m.insert("cn", "zh");
    m.insert("tw", "zh");
    m.insert("kr", "ko");
    m.insert("ara", "ar");
    m.insert("il", "he");
    m.insert("tr", "tr");
    m.insert("pl", "pl");
    m.insert("nl", "nl");
    m.insert("ua", "uk");
    m.insert("gr", "el");
    m.insert("se", "sv");
    m.insert("no", "no");
    m.insert("dk", "da");
    m.insert("fi", "fi");
    m.insert("cz", "cs");
    m.insert("hu", "hu");
    m.insert("ro", "ro");
    m.insert("th", "th");
    m.insert("vn", "vi");
    m.insert("in", "hi");
    m.insert("id", "id");
    m.insert("my", "ms");
    m
});

#[cfg(target_os = "macos")]
pub fn get_current_input_source() -> Option<String> {
    use std::process::Command;
    let output = Command::new("defaults")
        .args(["read", "com.apple.HIToolbox", "AppleSelectedInputSources"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Find "KeyboardLayout Name" specifically (not Bundle ID)
    for line in stdout.lines() {
        if line.contains("\"KeyboardLayout Name\"") {
            if let Some(start) = line.find('=') {
                let value = line[start + 1..].trim().trim_matches(|c| c == '"' || c == ';' || c == ' ');
                if !value.is_empty() {
                    return Some(format!("com.apple.keylayout.{}", value.replace(" ", "")));
                }
            }
        }
    }
    // Fallback: Input Source ID for input methods
    for line in stdout.lines() {
        if line.contains("\"Input Source ID\"") {
            if let Some(start) = line.find('=') {
                let value = line[start + 1..].trim().trim_matches(|c| c == '"' || c == ';' || c == ' ');
                if !value.is_empty() && value.starts_with("com.apple") {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

#[cfg(target_os = "windows")]
pub fn get_current_input_source() -> Option<String> {
    use std::process::Command;
    // Use PowerShell to get current keyboard layout
    let output = Command::new("powershell")
        .args(["-Command", "(Get-WinUserLanguageList)[0].InputMethodTips[0]"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    // Format: "0409:00000409" - extract the KLID part
    if let Some(klid) = stdout.split(':').last() {
        return Some(klid.to_lowercase());
    }
    None
}

#[cfg(target_os = "linux")]
pub fn get_current_input_source() -> Option<String> {
    use std::process::Command;
    // Try setxkbmap first
    if let Ok(output) = Command::new("setxkbmap").args(["-query"]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            if line.starts_with("layout:") {
                let layout = line.split(':').nth(1)?.trim();
                // Take first layout if multiple (e.g., "us,ru" -> "us")
                return Some(layout.split(',').next()?.trim().to_string());
            }
        }
    }
    // Fallback: check LANG environment variable
    if let Ok(lang) = std::env::var("LANG") {
        let code = lang.split('_').next()?;
        return Some(code.to_lowercase());
    }
    None
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
pub fn get_current_input_source() -> Option<String> {
    None
}

/// Converts input source ID to ISO 639-1 language code
pub fn input_source_to_language(source: &str) -> Option<&'static str> {
    // Direct lookup
    if let Some(&lang) = INPUT_SOURCE_MAP.get(source) {
        return Some(lang);
    }
    // Try suffix match for macOS (e.g., "Russian-PC" -> "Russian")
    let base = source.split('-').next().unwrap_or(source);
    if let Some(&lang) = INPUT_SOURCE_MAP.get(base) {
        return Some(lang);
    }
    None
}

/// Gets the language code from current OS input source
pub fn get_language_from_input_source() -> Option<String> {
    let source = get_current_input_source()?;
    input_source_to_language(&source).map(|s| s.to_string())
}
