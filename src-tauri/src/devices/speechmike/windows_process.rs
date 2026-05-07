/// Best-effort scan of running processes for known Philips / Dragon programs.
/// Returns process names found; may return an empty list if tasklist fails.
#[cfg(target_os = "windows")]
pub fn scan() -> Vec<String> {
    const KNOWN_BLOCKERS: &[&str] = &[
        "SpeechExec.exe",
        "SpeechControl.exe",
        "DeviceControlCenter.exe",
        "SEDict.exe",
    ];

    use std::process::Command;

    let output = match Command::new("tasklist")
        .args(["/FO", "CSV", "/NH"])
        .output()
    {
        Ok(o) => o,
        Err(_) => return vec![],
    };
    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut found: Vec<String> = KNOWN_BLOCKERS
        .iter()
        .filter(|blocker| stdout.to_lowercase().contains(&blocker.to_lowercase()))
        .map(|s| s.to_string())
        .collect();

    // Catch Dragon variants (Dragon NaturallySpeaking, Dragon Medical, etc.)
    for line in stdout.lines() {
        let lower = line.to_lowercase();
        if lower.contains("dragon") {
            if let Some(name) = line.split('"').nth(1) {
                let name = name.to_string();
                if !found.contains(&name) {
                    found.push(name);
                }
            }
        }
    }

    found
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
pub fn scan() -> Vec<String> {
    vec![]
}
