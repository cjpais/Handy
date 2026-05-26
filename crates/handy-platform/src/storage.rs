use std::path::PathBuf;

pub trait AppStorage: Send + Sync {
    fn models_dir(&self) -> PathBuf;
    fn db_path(&self) -> PathBuf;
    fn settings_path(&self) -> PathBuf;
    fn cache_dir(&self) -> PathBuf;
}
