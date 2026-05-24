use s3::{Bucket, creds::Credentials};

#[derive(Debug, Clone)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    pub endpoint: Option<String>,
    pub access_key: String,
    pub secret_key: String,
}

pub trait S3Client {
    fn put_object(&self, key: &str, data: &[u8]) -> anyhow::Result<()>;
    fn get_object(&self, key: &str) -> anyhow::Result<Vec<u8>>;
}

pub struct RealS3Client {
    pub bucket: Box<Bucket>,
}

impl RealS3Client {
    pub fn new(s3_config: &S3Config) -> anyhow::Result<Self> {
        let credentials = Credentials::new(
            Some(&s3_config.access_key),
            Some(&s3_config.secret_key),
            None,
            None,
            None,
        )
        .map_err(|e| anyhow::anyhow!("Failed to create S3 credentials: {}", e))?;

        let region = if let Some(ref ep) = s3_config.endpoint {
            if !ep.is_empty() {
                s3::Region::Custom {
                    region: s3_config.region.clone(),
                    endpoint: ep.clone(),
                }
            } else {
                s3_config
                    .region
                    .parse()
                    .map_err(|e| anyhow::anyhow!("Failed to parse S3 region: {}", e))?
            }
        } else {
            s3_config
                .region
                .parse()
                .map_err(|e| anyhow::anyhow!("Failed to parse S3 region: {}", e))?
        };

        let mut bucket = Bucket::new(&s3_config.bucket, region, credentials)
            .map_err(|e| anyhow::anyhow!("Failed to create S3 bucket client: {}", e))?;

        if s3_config.endpoint.is_some() {
            bucket = bucket.with_path_style();
        }

        Ok(Self { bucket })
    }
}

impl S3Client for RealS3Client {
    fn put_object(&self, key: &str, data: &[u8]) -> anyhow::Result<()> {
        self.bucket
            .put_object(key, data)
            .map_err(|e| anyhow::anyhow!("S3 upload failed: {}", e))?;
        Ok(())
    }

    fn get_object(&self, key: &str) -> anyhow::Result<Vec<u8>> {
        let data = self
            .bucket
            .get_object(key)
            .map_err(|e| anyhow::anyhow!("S3 download failed: {}", e))?;
        Ok(data.to_vec())
    }
}
