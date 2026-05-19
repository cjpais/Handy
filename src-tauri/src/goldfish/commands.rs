//! Tauri commands exposed by Goldfish. Listed in `collect_commands![]`
//! in `lib.rs` under the `// === Goldfish ===` marker.

#[tauri::command]
#[specta::specta]
pub fn goldfish_ping() -> String {
    "goldfish: pong".to_string()
}
