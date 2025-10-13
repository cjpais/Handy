use anyhow::Result;
use log::{debug, error};
use std::fs;
use std::path::PathBuf;
use tauri::{App, Manager};

use crate::audio_toolkit::save_wav_file;

pub struct AudioBackupManager {
    backup_dir: PathBuf,
}

impl AudioBackupManager {
    pub fn new(app: &App) -> Result<Self> {
        let app_data_dir = app.path().app_data_dir()?;
        let backup_dir = app_data_dir.join("audio_backups");
        
        // Create backup directory if it doesn't exist
        if !backup_dir.exists() {
            fs::create_dir_all(&backup_dir)?;
            debug!("Created audio backup directory: {:?}", backup_dir);
        }
        
        Ok(AudioBackupManager {
            backup_dir,
        })
    }
    
    /// Save audio samples as backup before transcription
    pub async fn save_backup_audio(&self, audio_samples: &[f32]) -> Result<PathBuf> {
        let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
        let filename = format!("backup_{}.wav", timestamp);
        let file_path = self.backup_dir.join(&filename);
        
        // Save the audio file
        save_wav_file(&file_path, audio_samples).await?;
        
        debug!("Saved backup audio: {:?}", file_path);
        Ok(file_path)
    }
    
    /// Get the most recent backup audio file
    pub fn get_latest_backup(&self) -> Result<Option<PathBuf>> {
        if !self.backup_dir.exists() {
            return Ok(None);
        }
        
        let mut entries: Vec<_> = fs::read_dir(&self.backup_dir)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.path().extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("wav"))
                    .unwrap_or(false)
            })
            .collect();
        
        // Sort by modification time, newest first
        entries.sort_by_key(|entry| {
            entry.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });
        entries.reverse();
        
        Ok(entries.first().map(|entry| entry.path()))
    }
    
    /// Clean up old backup files, keeping only the most recent one
    pub fn cleanup_old_backups(&self) -> Result<()> {
        if !self.backup_dir.exists() {
            return Ok(());
        }
        
        let mut entries: Vec<_> = fs::read_dir(&self.backup_dir)?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.path().extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext.eq_ignore_ascii_case("wav"))
                    .unwrap_or(false)
            })
            .collect();
        
        // Sort by modification time, newest first
        entries.sort_by_key(|entry| {
            entry.metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });
        entries.reverse();
        
        // Keep only the most recent file, delete the rest
        for entry in entries.iter().skip(1) {
            let path = entry.path();
            if let Err(e) = fs::remove_file(&path) {
                error!("Failed to remove old backup file {:?}: {}", path, e);
            } else {
                debug!("Removed old backup file: {:?}", path);
            }
        }
        
        Ok(())
    }
}