use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use specta::Type;

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct HFModelResult {
    pub id: String,
    pub author: Option<String>,
    pub downloads: u32,
    pub likes: u32,
    pub tags: Vec<String>,
    pub pipeline_tag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct HFFile {
    pub rfilename: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct HFModelFull {
    pub id: String,
    pub siblings: Vec<HFFile>, // siblings contains the files
    #[serde(default)]
    pub tags: Vec<String>,
}

pub struct HFClient {
    client: Client,
}

impl HFClient {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    pub async fn search_whisper_models(
        &self,
        query: &str,
        sort: Option<&str>,
    ) -> Result<Vec<HFModelResult>> {
        let mut final_query = query.to_string();
        let query_lower = query.to_lowercase();

        if !query.contains('/') {
            if !query_lower.contains("whisper") {
                final_query.push_str(" whisper");
            }
            if !query_lower.contains("ggml") {
                final_query.push_str(" ggml");
            }
        }

        let sort_param = sort.unwrap_or("downloads");

        let url = format!(
            "https://huggingface.co/api/models?search={}&sort={}&direction=-1&limit=20&full=false",
            urlencoding::encode(&final_query),
            sort_param
        );

        let response = self.client.get(url).send().await?;
        let models: Vec<HFModelResult> = response.json().await?;

        let filtered = models
            .into_iter()
            .filter(|m| {
                let id_lower = m.id.to_lowercase();
                let has_whisper = id_lower.contains("whisper");
                let has_ggml = id_lower.contains("ggml")
                    || m.tags.iter().any(|t| t.to_lowercase().contains("ggml"));

                has_whisper || has_ggml
            })
            .collect();

        Ok(filtered)
    }

    pub async fn get_model_details(&self, model_id: &str) -> Result<HFModelFull> {
        let url = format!("https://huggingface.co/api/models/{}", model_id);
        let response = self.client.get(url).send().await?;
        let model: HFModelFull = response.json().await?;
        Ok(model)
    }

    pub fn get_download_url(&self, model_id: &str, filename: &str) -> String {
        format!(
            "https://huggingface.co/{}/resolve/main/{}",
            model_id, filename
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_search_whisper_models() {
        let client = HFClient::new();
        let results = client
            .search_whisper_models("distil-medium", None)
            .await
            .unwrap();
        assert!(!results.is_empty());
        println!("Found {} models for 'distil-medium'", results.len());
    }

    #[tokio::test]
    async fn test_search_untagged_model() {
        let client = HFClient::new();
        let results = client
            .search_whisper_models("sBPOH/whisper-small", None)
            .await
            .unwrap();
        assert!(results
            .iter()
            .any(|m| m.id == "sBPOH/whisper-small-ru-1k-steps-ggml"));
    }

    #[tokio::test]
    async fn test_get_model_details() {
        let client = HFClient::new();
        let details = client
            .get_model_details("ggerganov/whisper.cpp")
            .await
            .unwrap();
        assert_eq!(details.id, "ggerganov/whisper.cpp");
        assert!(!details.siblings.is_empty());
    }
}
