mod config;
pub mod s3_client;

pub use config::*;

pub trait S3Client {
    fn put_object(&self, key: &str, data: &[u8]) -> anyhow::Result<()>;
    fn get_object(&self, key: &str) -> anyhow::Result<Vec<u8>>;
}
