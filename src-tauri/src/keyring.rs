use log::{debug, error};

const SERVICE_NAME: &str = "com.handy.app";

fn entry_for(provider_id: &str) -> keyring::Result<keyring::Entry> {
    keyring::Entry::new(SERVICE_NAME, &format!("post_process_{}", provider_id))
}

pub fn set_api_key(provider_id: &str, api_key: &str) -> Result<(), String> {
    let entry = entry_for(provider_id).map_err(|e| {
        error!(
            "Failed to create keyring entry for '{}': {}",
            provider_id, e
        );
        e.to_string()
    })?;
    entry.set_password(api_key).map_err(|e| {
        error!(
            "Failed to store API key for '{}' in keychain: {}",
            provider_id, e
        );
        e.to_string()
    })
}

pub fn get_api_key(provider_id: &str) -> Result<String, String> {
    let entry = entry_for(provider_id).map_err(|e| {
        error!(
            "Failed to create keyring entry for '{}': {}",
            provider_id, e
        );
        e.to_string()
    })?;
    match entry.get_password() {
        Ok(key) => Ok(key),
        Err(keyring::Error::NoEntry) => {
            debug!("No keychain entry found for provider '{}'", provider_id);
            Ok(String::new())
        }
        Err(e) => {
            error!(
                "Failed to retrieve API key for '{}' from keychain: {}",
                provider_id, e
            );
            Ok(String::new())
        }
    }
}

pub fn delete_api_key(provider_id: &str) -> Result<(), String> {
    let entry = entry_for(provider_id).map_err(|e| {
        error!(
            "Failed to create keyring entry for '{}': {}",
            provider_id, e
        );
        e.to_string()
    })?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => {
            debug!("No keychain entry to delete for provider '{}'", provider_id);
            Ok(())
        }
        Err(e) => {
            error!(
                "Failed to delete API key for '{}' from keychain: {}",
                provider_id, e
            );
            Err(e.to_string())
        }
    }
}

pub fn get_api_key_hint(provider_id: &str) -> Result<Option<String>, String> {
    let entry = entry_for(provider_id).map_err(|e| {
        error!(
            "Failed to create keyring entry for '{}': {}",
            provider_id, e
        );
        e.to_string()
    })?;
    match entry.get_password() {
        Ok(key) if key.is_empty() => Ok(None),
        Ok(key) => {
            let chars: Vec<char> = key.chars().collect();
            let last4: String = if chars.len() >= 4 {
                chars[chars.len() - 4..].iter().collect()
            } else {
                key.clone()
            };
            Ok(Some(format!(
                "\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}\u{2022}{}",
                last4
            )))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => {
            error!(
                "Failed to retrieve API key hint for '{}' from keychain: {}",
                provider_id, e
            );
            Ok(None)
        }
    }
}
