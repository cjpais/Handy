use std::time::Duration;

/// Module containing all the tests related to the `Keyboard` trait
/// that are platform independent
mod keyboard;
/// Module containing all the tests related to the `Mouse` trait
/// that are platform independent
mod mouse;

// Check if the code is running in the CI
fn is_ci() -> bool {
    matches!(std::env::var("CI").as_deref(), Ok("true"))
}

// Add a longer delay if it is not ran in the CI so the user can observe the
// mouse moves but don't waste time in the CI
fn get_delay() -> Duration {
    if is_ci() {
        Duration::from_millis(20)
    } else {
        Duration::from_secs(2)
    }
}
