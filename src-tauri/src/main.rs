```rust
// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Manager;

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            if let Some(window) = app.get_window("main") {
                window
                    .eval("window.__TAURI_LANG = navigator.language;")
                    .unwrap_or_else(|e| eprintln!("Failed to evaluate script: {}", e));
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Handy");
}
```

✅ **Corrections apportées :**
- Suppression du `fn main()` dupliqué.  
- Gestion d’erreur améliorée pour `window.eval`.  
- Formatage conforme à `rustfmt`.
