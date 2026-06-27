//! Minimal Hugging Face Hub REST client for the bits hf-hub doesn't cover:
//! listing a collection's models and a repo's files. Downloads still go through
//! hf-hub; this is only metadata.
//!
//! Auth uses the *stock* hf-hub token (whatever `huggingface-cli login` wrote,
//! or `HF_HOME/token`) — no custom environment wiring. Calls work anonymously
//! for public repos and gracefully return empty/err for private ones without a
//! token.

use anyhow::Result;
use serde::Deserialize;

const HF_ENDPOINT: &str = "https://huggingface.co";

/// A downloadable file within a repo.
pub struct RepoFile {
    pub path: String,
    pub size: u64,
}

/// The stock Hugging Face token, if the user has one configured. Same source
/// hf-hub itself reads, so downloads and metadata stay consistent.
fn stock_token() -> Option<String> {
    hf_hub::Cache::from_env().token()
}

async fn authed_get(url: &str) -> Result<reqwest::Response> {
    let mut req = reqwest::Client::new().get(url);
    if let Some(token) = stock_token() {
        req = req.bearer_auth(token);
    }
    Ok(req.send().await?.error_for_status()?)
}

#[derive(Deserialize)]
struct Collection {
    #[serde(default)]
    items: Vec<CollectionItem>,
}

#[derive(Deserialize)]
struct CollectionItem {
    #[serde(rename = "type", default)]
    item_type: String,
    #[serde(default)]
    id: Option<String>,
}

/// Fetch the model repo-ids in a Hugging Face collection (by full slug). The
/// collection may be public while its members are private; in that case an
/// unauthenticated request returns no items.
pub async fn fetch_collection_models(slug: &str) -> Result<Vec<String>> {
    let url = format!("{HF_ENDPOINT}/api/collections/{slug}");
    let collection: Collection = authed_get(&url).await?.json().await?;
    Ok(collection
        .items
        .into_iter()
        .filter(|i| i.item_type == "model")
        .filter_map(|i| i.id)
        .collect())
}

#[derive(Deserialize)]
struct TreeEntry {
    #[serde(rename = "type", default)]
    entry_type: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    lfs: Option<Lfs>,
}

#[derive(Deserialize)]
struct Lfs {
    #[serde(default)]
    size: u64,
}

/// Pull the real file size: GGUFs are LFS, so prefer `lfs.size` when present.
fn tree_entry_to_gguf(entry: TreeEntry) -> Option<RepoFile> {
    if entry.entry_type == "file" && entry.path.ends_with(".gguf") {
        let size = entry
            .lfs
            .map(|l| l.size)
            .filter(|s| *s > 0)
            .unwrap_or(entry.size);
        Some(RepoFile {
            path: entry.path,
            size,
        })
    } else {
        None
    }
}

/// List the GGUF files (path + size) in a repo's `main` revision.
pub async fn fetch_repo_gguf_files(repo_id: &str) -> Result<Vec<RepoFile>> {
    let url = format!("{HF_ENDPOINT}/api/models/{repo_id}/tree/main?recursive=true");
    let entries: Vec<TreeEntry> = authed_get(&url).await?.json().await?;
    Ok(entries.into_iter().filter_map(tree_entry_to_gguf).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_collection_models_only() {
        let json = r#"{"items":[
            {"type":"model","id":"handy-computer/parakeet"},
            {"type":"dataset","id":"handy-computer/some-data"},
            {"type":"paper"},
            {"type":"model","id":"handy-computer/whisper"}
        ]}"#;
        let collection: Collection = serde_json::from_str(json).unwrap();
        let ids: Vec<String> = collection
            .items
            .into_iter()
            .filter(|i| i.item_type == "model")
            .filter_map(|i| i.id)
            .collect();
        assert_eq!(
            ids,
            vec![
                "handy-computer/parakeet".to_string(),
                "handy-computer/whisper".to_string()
            ]
        );
    }

    #[test]
    fn picks_gguf_files_with_lfs_size() {
        let json = r#"[
            {"type":"file","path":"model-q8.gguf","size":135,"lfs":{"size":123456789}},
            {"type":"file","path":"README.md","size":100},
            {"type":"directory","path":"sub"},
            {"type":"file","path":"model-q4.gguf","size":50000000}
        ]"#;
        let entries: Vec<TreeEntry> = serde_json::from_str(json).unwrap();
        let files: Vec<RepoFile> = entries.into_iter().filter_map(tree_entry_to_gguf).collect();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "model-q8.gguf");
        assert_eq!(files[0].size, 123_456_789); // lfs size preferred over pointer size
        assert_eq!(files[1].path, "model-q4.gguf");
        assert_eq!(files[1].size, 50_000_000);
    }
}
