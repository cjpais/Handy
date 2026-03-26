use anyhow::Result;
use std::fs;
use std::path::Path;

pub fn write(path: &Path, transcript_text: &str) -> Result<()> {
    fs::write(path, transcript_text)?;
    Ok(())
}
