use std::path::Path;

/// Check that all required paths exist. Returns `false` (and logs which path
/// is missing) if any path is absent, so the caller can skip the test.
#[allow(dead_code)]
pub fn require_paths(paths: &[&Path]) -> bool {
    for path in paths {
        if !path.exists() {
            eprintln!("Skipping test: {:?} not found", path);
            return false;
        }
    }
    true
}
