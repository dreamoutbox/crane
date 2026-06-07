use crane::s3::S3Client;
#[allow(unused)]
use std::cell::RefCell;
use std::collections::HashMap;

struct MockS3Client {
    objects: RefCell<HashMap<String, Vec<u8>>>,
}

impl MockS3Client {
    #[allow(dead_code)]
    fn new() -> Self {
        Self {
            objects: RefCell::new(HashMap::new()),
        }
    }
}

impl S3Client for MockS3Client {
    fn put_object(&self, key: &str, data: &[u8]) -> anyhow::Result<()> {
        self.objects
            .borrow_mut()
            .insert(key.to_string(), data.to_vec());
        Ok(())
    }

    fn get_object(&self, key: &str) -> anyhow::Result<Vec<u8>> {
        self.objects
            .borrow_mut()
            .get(key)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Key not found: {}", key))
    }
}
