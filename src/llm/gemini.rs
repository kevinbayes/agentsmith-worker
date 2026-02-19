use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Google Gemini API provider.
pub struct GeminiProvider {
    client: reqwest::Client,
    api_key: String,
    model: String,
    base_url: String,
    max_tokens: u32,
}

#[derive(Serialize)]
struct GeminiRequest {
    contents: Vec<GeminiContent>,
    #[serde(rename = "generationConfig")]
    generation_config: GenerationConfig,
}

#[derive(Serialize)]
struct GeminiContent {
    parts: Vec<GeminiPart>,
}

#[derive(Serialize)]
struct GeminiPart {
    text: String,
}

#[derive(Serialize)]
struct GenerationConfig {
    #[serde(rename = "maxOutputTokens")]
    max_output_tokens: u32,
}

#[derive(Deserialize)]
struct GeminiResponse {
    candidates: Option<Vec<Candidate>>,
}

#[derive(Deserialize)]
struct Candidate {
    content: Option<CandidateContent>,
}

#[derive(Deserialize)]
struct CandidateContent {
    parts: Option<Vec<ResponsePart>>,
}

#[derive(Deserialize)]
struct ResponsePart {
    text: Option<String>,
}

impl GeminiProvider {
    pub fn new(api_key: String, model: String, base_url: String, max_tokens: u32) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default();
        Self {
            client,
            api_key,
            model,
            base_url,
            max_tokens,
        }
    }

    pub async fn generate(&self, system: &str, user: &str) -> Result<String> {
        // Combine system + user into a single content block
        let combined = format!("{}\n\n{}", system, user);

        let request = GeminiRequest {
            contents: vec![GeminiContent {
                parts: vec![GeminiPart { text: combined }],
            }],
            generation_config: GenerationConfig {
                max_output_tokens: self.max_tokens,
            },
        };

        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.base_url, self.model, self.api_key
        );

        let response = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Gemini API error {}: {}", status, body);
        }

        let api_resp: GeminiResponse = response.json().await?;
        let text = api_resp
            .candidates
            .unwrap_or_default()
            .into_iter()
            .filter_map(|c| c.content)
            .flat_map(|c| c.parts.unwrap_or_default())
            .filter_map(|p| p.text)
            .collect::<Vec<_>>()
            .join("");

        Ok(text)
    }
}
