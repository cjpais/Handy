use crate::ports::Logger;

#[derive(Debug, Default, Clone, Copy)]
pub struct StdoutLogger;

impl Logger for StdoutLogger {
    fn debug(&self, message: &str) {
        println!("[DEBUG] {message}");
    }

    fn info(&self, message: &str) {
        println!("[INFO] {message}");
    }

    fn error(&self, message: &str) {
        eprintln!("[ERROR] {message}");
    }
}
