use zbus::{interface, Result, blocking::Connection};
use zbus::object_server::SignalContext;
use zbus::zvariant::{Value, OwnedObjectPath, OwnedValue};
use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use once_cell::sync::Lazy;
use log::{info, debug};

static IBUS_CONN: Lazy<Mutex<Option<Connection>>> = Lazy::new(|| Mutex::new(None));
static ENGINE_ENABLED: AtomicBool = AtomicBool::new(false);
static ENGINE_FOCUSED: AtomicBool = AtomicBool::new(false);
static ACTIVE_ENGINE_PATH: Lazy<Mutex<Option<String>>> = Lazy::new(|| Mutex::new(None));

pub struct HandyIBusEngine;

#[interface(name = "org.freedesktop.IBus.Engine")]
impl HandyIBusEngine {
    fn process_key_event(&self, _keyval: u32, _keycode: u32, _modifiers: u32) -> bool {
        // Let all keypresses pass through transparently
        false
    }

    fn focus_in(&self) {
        debug!("Handy IBus Engine focused");
        ENGINE_FOCUSED.store(true, Ordering::SeqCst);
    }

    fn focus_out(&self) {
        debug!("Handy IBus Engine lost focus");
        ENGINE_FOCUSED.store(false, Ordering::SeqCst);
    }

    fn enable(&self) {
        debug!("Handy IBus Engine enabled");
        ENGINE_ENABLED.store(true, Ordering::SeqCst);
    }

    fn disable(&self) {
        debug!("Handy IBus Engine disabled");
        ENGINE_ENABLED.store(false, Ordering::SeqCst);
    }

    #[zbus(signal)]
    pub async fn commit_text(signal_ctxt: &SignalContext<'_>, text: Value<'_>) -> Result<()>;
}

pub struct HandyIBusFactory {
    connection: Connection,
}

#[interface(name = "org.freedesktop.IBus.Factory")]
impl HandyIBusFactory {
    fn create_engine(&self, name: &str) -> zbus::fdo::Result<OwnedObjectPath> {
        debug!("IBus Factory CreateEngine called for: {}", name);
        if name == "handy" {
            let engine = HandyIBusEngine;
            static INSTANCE_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
            let instance_id = INSTANCE_COUNTER.fetch_add(1, Ordering::SeqCst);
            let path = format!("/org/freedesktop/IBus/Engine/Handy/{}", instance_id);
            
            self.connection.object_server().at(path.as_str(), engine)
                .map_err(|e| zbus::fdo::Error::Failed(format!("Failed to register engine: {}", e)))?;
            debug!("Handy IBus Engine successfully created at {}", path);
            
            let mut active_path = ACTIVE_ENGINE_PATH.lock().unwrap();
            *active_path = Some(path.clone());
            
            OwnedObjectPath::try_from(path).map_err(|e| zbus::fdo::Error::Failed(e.to_string()))
        } else {
            Err(zbus::fdo::Error::Failed("Unsupported engine".to_string()))
        }
    }
}

pub fn is_engine_active() -> bool {
    IBUS_CONN.lock().unwrap().is_some()
}

pub fn start_ibus_engine_service() -> std::result::Result<(), String> {
    if is_engine_active() {
        info!("Handy IBus Engine service is already active. Skipping initialization.");
        return Ok(());
    }

    info!("Starting Handy IBus Input Method Engine D-Bus service...");

    let addr = crate::clipboard::get_ibus_address()
        .ok_or_else(|| "Failed to locate active IBus private socket address.".to_string())?;

    info!("Connecting to IBus private bus at {}", addr);
    let conn = zbus::blocking::connection::Builder::address(addr.as_str())
        .map_err(|e| format!("Invalid address: {}", e))?
        .build()
        .map_err(|e| format!("Failed to connect to IBus private bus: {}", e))?;

    info!("Requesting well-known name org.freedesktop.IBus.Handy");
    conn.request_name("org.freedesktop.IBus.Handy")
        .map_err(|e| format!("Failed to request D-Bus name org.freedesktop.IBus.Handy: {}", e))?;

    info!("Registering IBus Factory at /org/freedesktop/IBus/Factory");
    let factory = HandyIBusFactory {
        connection: conn.clone(),
    };
    conn.object_server()
        .at("/org/freedesktop/IBus/Factory", factory)
        .map_err(|e| format!("Failed to register IBus factory: {}", e))?;

    // Store the connection globally for future CommitText signal emissions
    let mut global_conn = IBUS_CONN.lock().unwrap();
    *global_conn = Some(conn);

    info!("Handy IBus Input Method Engine D-Bus service successfully initialized!");
    Ok(())
}

fn get_current_global_engine(conn: &Connection) -> std::result::Result<String, String> {
    let reply = conn.call_method(
        Some("org.freedesktop.IBus"),
        "/org/freedesktop/IBus",
        Some("org.freedesktop.DBus.Properties"),
        "Get",
        &("org.freedesktop.IBus", "GlobalEngine"),
    )
    .map_err(|e| format!("Failed to get GlobalEngine property: {}", e))?;

    let val: OwnedValue = reply.body().deserialize()
        .map_err(|e| format!("Failed to parse GlobalEngine reply: {}", e))?;

    // The property is a Variant containing a struct (sa{sv}ssssssssussssssss)
    let val_ref: &Value<'_> = &*val;
    if let Value::Value(inner_val) = val_ref {
        if let Value::Structure(structure) = inner_val.as_ref() {
            let fields = structure.fields();
            if fields.len() >= 3 {
                if let Value::Str(engine_name) = &fields[2] {
                    return Ok(engine_name.to_string());
                }
            }
        }
    }

    Err("Failed to extract engine name from GlobalEngine structure".to_string())
}

fn set_global_engine(conn: &Connection, engine_name: &str) -> std::result::Result<(), String> {
    conn.call_method(
        Some("org.freedesktop.IBus"),
        "/org/freedesktop/IBus",
        Some("org.freedesktop.IBus"),
        "SetGlobalEngine",
        &(engine_name,),
    )
    .map_err(|e| format!("Failed to set GlobalEngine to {}: {}", engine_name, e))?;
    Ok(())
}

pub fn commit_text_via_engine(text: &str) -> std::result::Result<(), String> {
    debug!("Attempting to commit text via IBus Engine: {}", text);

    let conn_guard = IBUS_CONN.lock().unwrap();
    let conn = conn_guard.as_ref()
        .ok_or_else(|| "IBus Engine D-Bus connection is not initialized.".to_string())?;

    // 1. Get original global engine so we can restore it later
    let original_engine = match get_current_global_engine(conn) {
        Ok(eng) => {
            debug!("Current global active IBus engine is: {}", eng);
            Some(eng)
        }
        Err(e) => {
            log::warn!("Failed to retrieve current active IBus engine: {}. Skipping auto-restore.", e);
            None
        }
    };

    // 2. Temporarily switch global engine to 'handy'
    let switch_success = if let Some(ref orig) = original_engine {
        if orig != "handy" {
            debug!("Switching IBus active engine to 'handy'");
            
            // Reset atomic flags before switching
            ENGINE_ENABLED.store(false, Ordering::SeqCst);
            ENGINE_FOCUSED.store(false, Ordering::SeqCst);

            if let Err(e) = set_global_engine(conn, "handy") {
                log::warn!("Failed to switch to handy engine: {}. Trying to commit anyway.", e);
                false
            } else {
                // Wait for the engine to be enabled and focused
                let start = std::time::Instant::now();
                let mut focused = false;
                let mut enabled = false;
                while start.elapsed() < std::time::Duration::from_millis(1500) {
                    focused = ENGINE_FOCUSED.load(Ordering::SeqCst);
                    enabled = ENGINE_ENABLED.load(Ordering::SeqCst);
                    if focused && enabled {
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                if !focused || !enabled {
                    log::warn!("Timed out waiting for Handy IBus Engine to be focused/enabled (focused: {}, enabled: {}). Proceeding anyway.", focused, enabled);
                } else {
                    debug!("Handy IBus Engine is active and focused. Sleeping 500ms for client context switch...");
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    debug!("Ready to commit text!");
                }
                true
            }
        } else {
            false
        }
    } else {
        false
    };

    // 3. Emit CommitText signal
    // Construct empty IBusAttrList: ("IBusAttrList", {}, [])
    let empty_properties = HashMap::<String, Value>::new();
    let empty_attributes = Vec::<Value>::new();
    let attr_list_struct = ("IBusAttrList", empty_properties, empty_attributes);
    let attr_list_variant = Value::new(attr_list_struct);

    // Construct IBusText: ("IBusText", {}, text, attr_list_variant)
    let empty_properties2 = HashMap::<String, Value>::new();
    let ibus_text_struct = ("IBusText", empty_properties2, text.to_string(), attr_list_variant);
    
    // Wrap in Value so zbus serializes it as a Variant containing the (sa{sv}sv) struct
    let val = Value::new(ibus_text_struct);

    let engine_path = {
        let active_path = ACTIVE_ENGINE_PATH.lock().unwrap();
        active_path.clone().unwrap_or_else(|| "/org/freedesktop/IBus/Engine/Handy".to_string())
    };
    
    debug!("Emitting CommitText signal from active engine path: {}", engine_path);

    conn.emit_signal(
        None::<&str>,
        engine_path.as_str(),
        "org.freedesktop.IBus.Engine",
        "CommitText",
        &(val,),
    )
    .map_err(|e| format!("Failed to emit CommitText signal from Engine: {}", e))?;

    debug!("Successfully committed text via IBus Engine!");

    // 4. Restore original active engine if we switched it
    if switch_success {
        if let Some(ref orig) = original_engine {
            // Sleep 1000ms to ensure the signal is processed before switching back
            std::thread::sleep(std::time::Duration::from_millis(1000));
            debug!("Restoring IBus active engine to '{}'", orig);
            let _ = set_global_engine(conn, orig);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_debug_get_current_global_engine() {
        let addr = crate::clipboard::get_ibus_address().expect("IBus address");
        let conn = zbus::blocking::connection::Builder::address(addr.as_str())
            .unwrap()
            .build()
            .unwrap();
        
        let reply = conn.call_method(
            Some("org.freedesktop.IBus"),
            "/org/freedesktop/IBus",
            Some("org.freedesktop.DBus.Properties"),
            "Get",
            &("org.freedesktop.IBus", "GlobalEngine"),
        ).unwrap();

        let val: zbus::zvariant::OwnedValue = reply.body().deserialize().unwrap();
        println!("Full zvariant value: {:?}", val);
        
        let eng = get_current_global_engine(&conn).unwrap();
        println!("Resolved engine name: {:?}", eng);
    }

    #[test]
    fn test_construct_correct_ibus_text_signature() {
        use std::collections::HashMap;
        use zbus::zvariant::Value;

        let text = "Hello IBus!";
        
        // 1. Construct empty IBusAttrList: ("IBusAttrList", {}, [])
        let empty_properties = HashMap::<String, Value>::new();
        let empty_attributes = Vec::<Value>::new();
        let attr_list_struct = ("IBusAttrList", empty_properties, empty_attributes);
        let attr_list_variant = Value::new(attr_list_struct);

        // 2. Construct IBusText: ("IBusText", {}, text, attr_list_variant)
        let empty_properties2 = HashMap::<String, Value>::new();
        let ibus_text_struct = ("IBusText", empty_properties2, text.to_string(), attr_list_variant);
        let val = Value::new(ibus_text_struct);

        println!("Rust constructed Value: {:?}", val);
        let signature = val.value_signature();
        println!("Signature: {:?}", signature);
        assert_eq!(signature.as_str(), "(sa{sv}sv)");
    }
}




