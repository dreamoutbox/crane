use crate::{config, s3::s3_client::S3Config};

pub mod s3_client;

pub fn get_s3_config(
    config: &config::Config,
    dot_env: &std::collections::HashMap<String, String>,
) -> anyhow::Result<S3Config> {
    let s3_section = config
        .backup
        .as_ref()
        .and_then(|b| b.s3.as_ref())
        .ok_or_else(|| {
            anyhow::anyhow!("S3 backup configuration [backup.s3] is missing in crane.toml")
        })?;

    let bucket = s3_section
        .get("bucket")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("S3 bucket is not specified in crane.toml"))?
        .to_string();

    let region = s3_section
        .get("region")
        .and_then(|v| v.as_str())
        .unwrap_or("us-east-1")
        .to_string();

    let endpoint = s3_section
        .get("endpoint")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let access_key = dot_env.get("S3_ACCESS_KEY_ID")
        .cloned()
        .or_else(|| s3_section.get("access_key_id").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .ok_or_else(|| anyhow::anyhow!("S3 access key id is not configured (set S3_ACCESS_KEY_ID in .env or access_key_id in crane.toml)"))?;

    let secret_key = dot_env.get("S3_SECRET_ACCESS_KEY")
        .cloned()
        .or_else(|| s3_section.get("secret_access_key").and_then(|v| v.as_str()).map(|s| s.to_string()))
        .ok_or_else(|| anyhow::anyhow!("S3 secret access key is not configured (set S3_SECRET_ACCESS_KEY in .env or secret_access_key in crane.toml)"))?;

    Ok(S3Config {
        bucket,
        region,
        endpoint,
        access_key,
        secret_key,
    })
}
