use std::path::{Path, PathBuf};

use chrono::Utc;
use serde::Serialize;

use crate::config::GcsConfig;
use crate::scheduler::{ScheduleRunResult, ScheduledJob};

/// Archived output record written to disk (and optionally GCS).
#[derive(Serialize)]
struct ArchivedRun {
    schedule_id: u64,
    name: String,
    tool: String,
    prompt: String,
    cron_expr: String,
    success: bool,
    output: String,
    duration_secs: u64,
    started_at: String,
    completed_at: String,
}

/// Archive a completed run to local disk and optionally GCS.
pub async fn archive_result(
    job: &ScheduledJob,
    result: &ScheduleRunResult,
    data_dir: &Path,
    gcs_config: Option<&GcsConfig>,
) {
    let output_dir = data_dir.join("cron").join("outputs");
    if let Err(e) = std::fs::create_dir_all(&output_dir) {
        tracing::warn!("Failed to create cron output dir: {}", e);
        return;
    }

    let slug = slugify(&job.name);
    let short_id = format!("{:08x}", job.id);
    let timestamp = Utc::now().format("%Y%m%dT%H%M%SZ");
    let filename = format!("{}-{}-{}.json", timestamp, slug, short_id);
    let filepath = output_dir.join(&filename);

    let record = ArchivedRun {
        schedule_id: job.id,
        name: job.name.clone(),
        tool: job.tool.clone(),
        prompt: job.prompt.clone(),
        cron_expr: job.cron_expr.clone(),
        success: result.success,
        output: result.output.clone(),
        duration_secs: result.duration_secs,
        started_at: result
            .completed_at
            .checked_sub_signed(chrono::Duration::seconds(result.duration_secs as i64))
            .unwrap_or(result.completed_at)
            .to_rfc3339(),
        completed_at: result.completed_at.to_rfc3339(),
    };

    match serde_json::to_string_pretty(&record) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&filepath, &json) {
                tracing::warn!("Failed to write cron output file: {}", e);
                return;
            }
            tracing::debug!("Archived cron output to {}", filepath.display());

            // Upload to GCS if configured
            if let Some(gcs) = gcs_config {
                upload_to_gcs(gcs, &filename, &json).await;
            }
        }
        Err(e) => {
            tracing::warn!("Failed to serialize cron output: {}", e);
        }
    }
}

fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

async fn upload_to_gcs(gcs: &GcsConfig, filename: &str, content: &str) {
    let token = match get_gcs_token(gcs).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("GCS auth failed (skipping upload): {}", e);
            return;
        }
    };

    let prefix = gcs.prefix.as_deref().unwrap_or("agentsmith/cron");
    let object_name = format!("{}/{}", prefix, filename);
    let url = format!(
        "https://storage.googleapis.com/upload/storage/v1/b/{}/o?uploadType=media&name={}",
        gcs.bucket, object_name
    );

    let client = reqwest::Client::new();
    match client
        .post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(content.to_string())
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                tracing::debug!("Uploaded cron output to gs://{}/{}", gcs.bucket, object_name);
            } else {
                tracing::warn!(
                    "GCS upload failed with status {}: {}",
                    resp.status(),
                    resp.text().await.unwrap_or_default()
                );
            }
        }
        Err(e) => {
            tracing::warn!("GCS upload request failed: {}", e);
        }
    }
}

async fn get_gcs_token(gcs: &GcsConfig) -> anyhow::Result<String> {
    let creds_path = gcs
        .credentials_path
        .clone()
        .or_else(|| std::env::var("GOOGLE_APPLICATION_CREDENTIALS").ok().map(PathBuf::from))
        .ok_or_else(|| anyhow::anyhow!("No GCS credentials path configured"))?;

    let creds_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&creds_path)?)?;

    let client_email = creds_json["client_email"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing client_email in credentials"))?;
    let private_key = creds_json["private_key"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing private_key in credentials"))?;
    let token_uri = creds_json["token_uri"]
        .as_str()
        .unwrap_or("https://oauth2.googleapis.com/token");

    // Build JWT for service account
    let now = chrono::Utc::now().timestamp();
    let claims = serde_json::json!({
        "iss": client_email,
        "scope": "https://www.googleapis.com/auth/devstorage.read_write",
        "aud": token_uri,
        "iat": now,
        "exp": now + 3600,
    });

    let header = base64_url_encode(&serde_json::json!({"alg": "RS256", "typ": "JWT"}).to_string());
    let payload = base64_url_encode(&claims.to_string());
    let signing_input = format!("{}.{}", header, payload);

    // Sign with RSA private key
    let signature = sign_rs256(private_key, signing_input.as_bytes())?;
    let sig_b64 = base64_url_encode_bytes(&signature);
    let jwt = format!("{}.{}", signing_input, sig_b64);

    // Exchange JWT for access token
    let client = reqwest::Client::new();
    let resp = client
        .post(token_uri)
        .form(&[
            ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
            ("assertion", &jwt),
        ])
        .send()
        .await?;

    let body: serde_json::Value = resp.json().await?;
    body["access_token"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("No access_token in OAuth response"))
}

fn base64_url_encode(input: &str) -> String {
    base64_url_encode_bytes(input.as_bytes())
}

fn base64_url_encode_bytes(input: &[u8]) -> String {
    use std::io::Write;
    let mut buf = Vec::new();
    {
        let mut encoder = Base64Encoder::new(&mut buf);
        encoder.write_all(input).unwrap();
        encoder.finish().unwrap();
    }
    String::from_utf8(buf)
        .unwrap()
        .replace('+', "-")
        .replace('/', "_")
        .trim_end_matches('=')
        .to_string()
}

/// Minimal base64 encoder (no external dependency needed beyond what we have).
struct Base64Encoder<W: std::io::Write> {
    writer: W,
    buf: Vec<u8>,
}

impl<W: std::io::Write> Base64Encoder<W> {
    fn new(writer: W) -> Self {
        Self {
            writer,
            buf: Vec::new(),
        }
    }

    fn finish(mut self) -> std::io::Result<()> {
        const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let input = &self.buf;
        let mut i = 0;
        while i + 2 < input.len() {
            let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8) | (input[i + 2] as u32);
            self.writer.write_all(&[
                CHARS[((n >> 18) & 0x3f) as usize],
                CHARS[((n >> 12) & 0x3f) as usize],
                CHARS[((n >> 6) & 0x3f) as usize],
                CHARS[(n & 0x3f) as usize],
            ])?;
            i += 3;
        }
        let rem = input.len() - i;
        if rem == 2 {
            let n = ((input[i] as u32) << 16) | ((input[i + 1] as u32) << 8);
            self.writer.write_all(&[
                CHARS[((n >> 18) & 0x3f) as usize],
                CHARS[((n >> 12) & 0x3f) as usize],
                CHARS[((n >> 6) & 0x3f) as usize],
                b'=',
            ])?;
        } else if rem == 1 {
            let n = (input[i] as u32) << 16;
            self.writer.write_all(&[
                CHARS[((n >> 18) & 0x3f) as usize],
                CHARS[((n >> 12) & 0x3f) as usize],
                b'=',
                b'=',
            ])?;
        }
        Ok(())
    }
}

impl<W: std::io::Write> std::io::Write for Base64Encoder<W> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn sign_rs256(pem_key: &str, data: &[u8]) -> anyhow::Result<Vec<u8>> {
    // Shell out to openssl for RSA signing since we don't have an RSA crate
    use std::io::Write;
    use std::process::{Command, Stdio};

    // Write key to a temp file so openssl can read it
    let tmp_key = std::env::temp_dir().join("agentsmith_gcs_key.pem");
    std::fs::write(&tmp_key, pem_key)?;

    let mut child = Command::new("openssl")
        .args(["dgst", "-sha256", "-sign", tmp_key.to_str().unwrap()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    child.stdin.as_mut().unwrap().write_all(data)?;
    let output = child.wait_with_output()?;

    // Clean up temp key
    let _ = std::fs::remove_file(&tmp_key);

    if !output.status.success() {
        anyhow::bail!(
            "openssl signing failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(output.stdout)
}
