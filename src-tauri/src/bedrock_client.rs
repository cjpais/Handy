use crate::settings::AppSettings;
use aws_config::BehaviorVersion;
use aws_sdk_bedrockruntime::types::{ContentBlock, ConversationRole, Message};
use log::debug;

async fn build_config(settings: &AppSettings) -> aws_config::SdkConfig {
    let region = aws_config::Region::new(settings.bedrock_region.clone());
    let mut loader = aws_config::defaults(BehaviorVersion::latest()).region(region);

    if settings.bedrock_use_profile {
        if !settings.bedrock_profile.is_empty() {
            loader = loader.profile_name(&settings.bedrock_profile);
        }
    } else if !settings.bedrock_access_key_id.is_empty()
        && !settings.bedrock_secret_access_key.is_empty()
    {
        let creds = aws_sdk_bedrock::config::Credentials::new(
            &settings.bedrock_access_key_id,
            &settings.bedrock_secret_access_key,
            if settings.bedrock_session_token.is_empty() {
                None
            } else {
                Some(settings.bedrock_session_token.clone())
            },
            None,
            "handy",
        );
        loader = loader.credentials_provider(creds);
    }

    loader.load().await
}

/// Map common AWS SDK error strings to user-friendly messages.
fn friendly_aws_error(msg: &str, fallback_prefix: &str) -> String {
    if msg.contains("dispatch failure") || msg.contains("connector error") {
        "Could not connect to AWS. Check your credentials, profile name, and network connection."
            .to_string()
    } else if msg.contains("AccessDenied") || msg.contains("not authorized") {
        "Access denied. Check your AWS credentials and Bedrock model permissions.".to_string()
    } else if msg.contains("expired") {
        "AWS credentials have expired. Please refresh your credentials.".to_string()
    } else if msg.contains("Could not find profile") {
        "AWS profile not found. Check that the profile name matches one in ~/.aws/credentials."
            .to_string()
    } else if msg.contains("ValidationException") {
        "Invalid model. Select a valid model from the dropdown.".to_string()
    } else {
        format!("{}: {}", fallback_prefix, msg)
    }
}

pub async fn list_models(settings: &AppSettings) -> Result<Vec<String>, String> {
    let config = build_config(settings).await;
    let client = aws_sdk_bedrock::Client::new(&config);

    let mut models: Vec<String> = Vec::new();

    // List inference profiles (includes cross-region and all available models)
    let mut next_token: Option<String> = None;
    loop {
        let mut req = client.list_inference_profiles().max_results(100);
        if let Some(token) = &next_token {
            req = req.next_token(token);
        }
        match req.send().await {
            Ok(resp) => {
                for p in resp.inference_profile_summaries() {
                    models.push(p.inference_profile_id().to_string());
                }
                next_token = resp.next_token().map(|s| s.to_string());
                if next_token.is_none() {
                    break;
                }
            }
            Err(e) => {
                return Err(friendly_aws_error(
                    &format!("{}", e),
                    "Failed to list models",
                ))
            }
        }
    }

    models.sort();
    Ok(models)
}

fn cross_region_model_id(model_id: &str, region: &str) -> String {
    // Don't double-prefix if model already has a region prefix (from inference profiles)
    let known_prefixes = ["us.", "eu.", "apac.", "global."];
    if known_prefixes.iter().any(|p| model_id.starts_with(p)) {
        return model_id.to_string();
    }
    let prefix = region.split('-').next().unwrap_or(region);
    match prefix {
        "us" => format!("us.{}", model_id),
        "eu" => format!("eu.{}", model_id),
        "ap" => format!("apac.{}", model_id),
        _ => model_id.to_string(),
    }
}

pub async fn send_converse(
    settings: &AppSettings,
    model_id: &str,
    prompt: String,
) -> Result<Option<String>, String> {
    let config = build_config(settings).await;
    let client = aws_sdk_bedrockruntime::Client::new(&config);

    let effective_model_id = if settings.bedrock_use_cross_region {
        cross_region_model_id(model_id, &settings.bedrock_region)
    } else {
        model_id.to_string()
    };

    debug!(
        "Sending Bedrock Converse request to model: {}",
        effective_model_id
    );

    let message = Message::builder()
        .role(ConversationRole::User)
        .content(ContentBlock::Text(prompt))
        .build()
        .map_err(|e| format!("Failed to build message: {}", e))?;

    let resp = client
        .converse()
        .model_id(&effective_model_id)
        .messages(message)
        .send()
        .await
        .map_err(|e| friendly_aws_error(&format!("{}", e), "Bedrock request failed"))?;

    let text = resp
        .output()
        .and_then(|o| o.as_message().ok())
        .and_then(|m| m.content().first())
        .and_then(|c| c.as_text().ok())
        .map(|t| t.to_string());

    Ok(text)
}

pub async fn test_connection(settings: &AppSettings, model_id: &str) -> Result<String, String> {
    send_converse(settings, model_id, "Reply with exactly: OK".to_string())
        .await
        .and_then(|opt| opt.ok_or_else(|| "Model returned empty response".to_string()))
}
