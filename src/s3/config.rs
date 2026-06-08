#[derive(Debug, Clone)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    pub endpoint: Option<String>,
    pub access_key: String,
    pub secret_key: String,
}

pub fn get_s3_config(config: &crate::config::Config) -> anyhow::Result<S3Config> {
    let s3_config = config
        .backup
        .as_ref()
        .and_then(|b| b.s3.as_ref())
        .ok_or_else(|| {
            anyhow::anyhow!("S3 backup configuration [backup.s3] is missing in crane.toml")
        })?;

    let bucket = s3_config.bucket.clone();

    let region = s3_config
        .region
        .clone()
        .unwrap_or_else(|| "us-east-1".to_string());

    let endpoint = s3_config.endpoint.clone();
    let access_key = s3_config.access_key_id.clone();
    let secret_key = s3_config.secret_access_key.clone();

    Ok(S3Config {
        bucket,
        region,
        endpoint,
        access_key,
        secret_key,
    })
}
