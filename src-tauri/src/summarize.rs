#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
use crate::apple_intelligence;
use crate::llm_client::{self, ReasoningConfig};
use crate::managers::history::ActionItem;
use crate::settings::{AppSettings, APPLE_INTELLIGENCE_PROVIDER_ID};
use log::{debug, error, warn};
use serde::Deserialize;

/// Structured result of summarising a transcript.
pub struct SummaryResult {
    pub title: Option<String>,
    pub summary: String,
    pub actions: Vec<ActionItem>,
    /// The prompt template used, so it can be stored alongside the entry.
    pub prompt: String,
}

#[derive(Debug, Deserialize)]
struct RawSummary {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    summary: Option<String>,
    #[serde(default)]
    actions: Option<Vec<RawAction>>,
}

#[derive(Debug, Deserialize)]
struct RawAction {
    description: String,
    #[serde(default)]
    assignee: Option<String>,
    #[serde(default)]
    due: Option<String>,
}

/// Strip invisible Unicode characters that some LLMs may insert.
fn strip_invisible_chars(s: &str) -> String {
    s.replace(['\u{200B}', '\u{200C}', '\u{200D}', '\u{FEFF}'], "")
}

fn clean_opt(value: Option<String>) -> Option<String> {
    value
        .map(|v| strip_invisible_chars(v.trim()))
        .filter(|v| !v.is_empty())
}

impl RawSummary {
    fn into_result(self, prompt: String) -> SummaryResult {
        let actions = self
            .actions
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| {
                let description = strip_invisible_chars(a.description.trim());
                if description.is_empty() {
                    return None;
                }
                Some(ActionItem {
                    description,
                    assignee: clean_opt(a.assignee),
                    due: clean_opt(a.due),
                })
            })
            .collect();

        SummaryResult {
            title: clean_opt(self.title),
            summary: clean_opt(self.summary).unwrap_or_default(),
            actions,
            prompt,
        }
    }
}

/// JSON schema describing the structured summary output.
fn summary_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "title": { "type": "string", "description": "A short title, a few words, no trailing punctuation" },
            "summary": { "type": "string", "description": "A concise summary written in the second person" },
            "actions": {
                "type": "array",
                "description": "Discrete action items extracted from the note",
                "items": {
                    "type": "object",
                    "properties": {
                        "description": { "type": "string" },
                        "assignee": { "type": ["string", "null"] },
                        "due": { "type": ["string", "null"] }
                    },
                    "required": ["description", "assignee", "due"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["title", "summary", "actions"],
        "additionalProperties": false
    })
}

/// Build a system prompt from the user's prompt template. Removes the
/// `${output}` placeholder since the transcript is sent as the user message.
fn build_system_prompt(prompt_template: &str) -> String {
    prompt_template.replace("${output}", "").trim().to_string()
}

/// Parse a model response that should contain the summary JSON. Falls back to
/// treating the whole response as the summary body if it isn't valid JSON.
fn parse_summary_content(content: &str, prompt: &str) -> SummaryResult {
    // Models in legacy mode sometimes wrap JSON in prose or code fences; try to
    // locate the outermost object before giving up.
    let candidate = extract_json_object(content).unwrap_or(content);

    match serde_json::from_str::<RawSummary>(candidate) {
        Ok(raw) => raw.into_result(prompt.to_string()),
        Err(e) => {
            warn!(
                "Failed to parse summary JSON ({}); using raw content as summary",
                e
            );
            SummaryResult {
                title: None,
                summary: strip_invisible_chars(content.trim()),
                actions: Vec::new(),
                prompt: prompt.to_string(),
            }
        }
    }
}

/// Best-effort extraction of the outermost `{...}` object from a string.
fn extract_json_object(content: &str) -> Option<&str> {
    let start = content.find('{')?;
    let end = content.rfind('}')?;
    if end > start {
        Some(&content[start..=end])
    } else {
        None
    }
}

/// Summarise a transcript and extract action items via the configured LLM
/// provider (shared with post-processing). Returns an error string describing
/// why summarisation could not run or failed.
pub async fn summarize_text(settings: &AppSettings, text: &str) -> Result<SummaryResult, String> {
    if text.trim().is_empty() {
        return Err("Nothing to summarise: text is empty".to_string());
    }

    let provider = settings
        .summarize_provider()
        .cloned()
        .ok_or_else(|| "No summarisation provider is configured".to_string())?;

    let model = settings
        .summarize_model(&provider.id)
        .cloned()
        .unwrap_or_default();
    if model.trim().is_empty() {
        return Err(format!(
            "No summarisation model configured for provider '{}'",
            provider.id
        ));
    }

    let prompt = {
        let selected_id = settings
            .summarize_selected_prompt_id
            .as_ref()
            .ok_or_else(|| "No summarisation prompt is selected".to_string())?;
        settings
            .summarize_prompts
            .iter()
            .find(|p| &p.id == selected_id)
            .map(|p| p.prompt.clone())
            .ok_or_else(|| format!("Summarisation prompt '{}' was not found", selected_id))?
    };
    if prompt.trim().is_empty() {
        return Err("The selected summarisation prompt is empty".to_string());
    }

    let api_key = settings.summarize_api_key(&provider.id);

    debug!(
        "Starting summarisation with provider '{}' (model: {})",
        provider.id, model
    );

    let system_prompt = build_system_prompt(&prompt);

    // Apple Intelligence uses native Swift APIs rather than an HTTP endpoint.
    if provider.id == APPLE_INTELLIGENCE_PROVIDER_ID {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            if !apple_intelligence::check_apple_intelligence_availability() {
                return Err("Apple Intelligence is not currently available".to_string());
            }
            let token_limit = model.trim().parse::<i32>().unwrap_or(0);
            let instructed = format!(
                "{}\n\nReturn ONLY a JSON object with keys \"title\" (string), \"summary\" (string), and \"actions\" (array of objects with \"description\", optional \"assignee\", optional \"due\").",
                system_prompt
            );
            return match apple_intelligence::process_text_with_system_prompt(
                &instructed,
                text,
                token_limit,
            ) {
                Ok(result) => Ok(parse_summary_content(&result, &prompt)),
                Err(err) => Err(format!("Apple Intelligence summarisation failed: {}", err)),
            };
        }
        #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
        {
            return Err("Apple Intelligence is only available on Apple silicon Macs".to_string());
        }
    }

    // Disable reasoning where it rarely helps and can pollute JSON output.
    let (reasoning_effort, reasoning) = match provider.id.as_str() {
        "custom" => (Some("none".to_string()), None),
        "openrouter" => (
            None,
            Some(ReasoningConfig {
                effort: Some("none".to_string()),
                exclude: Some(true),
            }),
        ),
        _ => (None, None),
    };

    if provider.supports_structured_output {
        match llm_client::send_chat_completion_with_schema(
            &provider,
            api_key.clone(),
            &model,
            text.to_string(),
            Some(system_prompt.clone()),
            Some(summary_schema()),
            reasoning_effort.clone(),
            reasoning.clone(),
        )
        .await
        {
            Ok(Some(content)) => return Ok(parse_summary_content(&content, &prompt)),
            Ok(None) => return Err("Summarisation response had no content".to_string()),
            Err(e) => {
                warn!(
                    "Structured summarisation failed for provider '{}': {}. Falling back to legacy mode.",
                    provider.id, e
                );
            }
        }
    }

    // Legacy mode: ask for JSON in the prompt and parse leniently.
    let legacy_prompt = format!(
        "{}\n\nReturn ONLY a JSON object with keys \"title\" (string), \"summary\" (string), and \"actions\" (array of objects with \"description\", and optional \"assignee\" and \"due\").\n\nTranscript:\n{}",
        system_prompt, text
    );

    match llm_client::send_chat_completion(
        &provider,
        api_key,
        &model,
        legacy_prompt,
        reasoning_effort,
        reasoning,
    )
    .await
    {
        Ok(Some(content)) => Ok(parse_summary_content(&content, &prompt)),
        Ok(None) => Err("Summarisation response had no content".to_string()),
        Err(e) => {
            error!("Summarisation failed for provider '{}': {}", provider.id, e);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_structured_summary() {
        let content = r#"{
            "title": "Problem refinement",
            "summary": "You need to discuss the problem with [name].",
            "actions": [
                { "description": "Set up a meeting with [name]", "assignee": "[name]", "due": null }
            ]
        }"#;
        let result = parse_summary_content(content, "prompt");
        assert_eq!(result.title.as_deref(), Some("Problem refinement"));
        assert_eq!(
            result.summary,
            "You need to discuss the problem with [name]."
        );
        assert_eq!(result.actions.len(), 1);
        assert_eq!(result.actions[0].assignee.as_deref(), Some("[name]"));
        assert!(result.actions[0].due.is_none());
    }

    #[test]
    fn extracts_json_wrapped_in_prose() {
        let content =
            "Sure! Here you go:\n```json\n{\"title\":\"T\",\"summary\":\"S\",\"actions\":[]}\n```";
        let result = parse_summary_content(content, "prompt");
        assert_eq!(result.title.as_deref(), Some("T"));
        assert_eq!(result.summary, "S");
        assert!(result.actions.is_empty());
    }

    #[test]
    fn falls_back_to_raw_content_on_invalid_json() {
        let content = "This is just a plain summary, no JSON here.";
        let result = parse_summary_content(content, "prompt");
        assert!(result.title.is_none());
        assert_eq!(
            result.summary,
            "This is just a plain summary, no JSON here."
        );
        assert!(result.actions.is_empty());
    }

    #[test]
    fn drops_empty_action_descriptions() {
        let content = r#"{"title":"T","summary":"S","actions":[{"description":"  "},{"description":"Real"}]}"#;
        let result = parse_summary_content(content, "prompt");
        assert_eq!(result.actions.len(), 1);
        assert_eq!(result.actions[0].description, "Real");
    }
}
