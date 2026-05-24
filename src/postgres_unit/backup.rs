use crate::server_interactor::server_interactor_trait::ServerInteractor;
use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct BackupMetadata {
    pub id: String,
    pub date: String,
    pub time: String,
    pub backup_type: String, // "FULL" or "INCR"
    pub base: Option<String>,
    pub local_path: String,
    pub s3_path: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct BackupRegistry {
    pub backups: Vec<BackupMetadata>,
}

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
                Region::Custom {
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

pub fn run_backup(
    interactor: &dyn ServerInteractor,
    s3_client: &dyn S3Client,
    pg_version: &str,
    backup_type: &str,
    replica_pass: &str,
    bucket_name: &str,
    last_backup: Option<&BackupMetadata>,
) -> anyhow::Result<BackupMetadata> {
    // 1. Get Date and Time from DB Node
    let date_output = interactor.cmd("date +'%Y%m%d%H%M%S%3N %Y-%m-%d %H:%M:%S'")?;
    let parts: Vec<&str> = date_output.trim().split_whitespace().collect();
    if parts.len() < 3 {
        anyhow::bail!("Failed to parse date output from server: '{}'", date_output);
    }
    let id = parts[0].to_string();
    let date = parts[1].to_string();
    let time = parts[2].to_string();

    let local_path = format!("/var/lib/postgresql/backups/{}", id);
    let pg_basebackup = format!("/usr/lib/postgresql/{}/bin/pg_basebackup", pg_version);
    let pg_verifybackup = format!("/usr/lib/postgresql/{}/bin/pg_verifybackup", pg_version);

    // 2. Ensure Backup Directories exist
    interactor.cmd("sudo mkdir -p /var/lib/postgresql/backups")?;
    interactor.cmd("sudo chown postgres:postgres /var/lib/postgresql/backups")?;
    interactor.cmd("sudo chmod 755 /var/lib/postgresql/backups")?;
    interactor.cmd(&format!("sudo -u postgres mkdir -p {}", local_path))?;

    // 3. Grant pg_read_server_files to replicator (idempotent)
    interactor.cmd(r#"sudo -u postgres psql -c "GRANT pg_read_server_files TO replicator;""#)?;

    // 4. Build pg_basebackup command
    let is_incr =
        backup_type.to_lowercase() == "incr" || backup_type.to_lowercase() == "incremental";
    let mut pgbasebackup_cmd = format!(
        "sudo -u postgres PGPASSWORD='{}' {} -h localhost -U replicator -D {} -F t -X s -c fast --manifest-checksums=sha256",
        replica_pass, pg_basebackup, local_path
    );

    let mut base_id = None;
    if is_incr {
        if let Some(parent) = last_backup {
            base_id = Some(parent.id.clone());
            let parent_manifest =
                format!("/var/lib/postgresql/backups/{}/backup_manifest", parent.id);

            // Check if parent manifest is present locally
            let test_manifest = interactor.cmd(&format!(
                "test -f {} && echo 'yes' || echo 'no'",
                parent_manifest
            ))?;
            if test_manifest.trim() != "yes" {
                // Recreate parent directory and restore manifest from S3
                interactor.cmd(&format!(
                    "sudo -u postgres mkdir -p /var/lib/postgresql/backups/{}",
                    parent.id
                ))?;
                interactor.cmd(&format!(
                    "sudo chmod 755 /var/lib/postgresql/backups/{}",
                    parent.id
                ))?;
                let s3_key = format!("backups/{}/backup_manifest", parent.id);
                let manifest_data = s3_client.get_object(&s3_key)?;

                // Write back on VPS
                let temp_path = format!("/tmp/manifest_{}", parent.id);
                let content = String::from_utf8_lossy(&manifest_data);
                interactor.create_file(&temp_path, &content)?;
                interactor.cmd(&format!("sudo mv {} {}", temp_path, parent_manifest))?;
                interactor.cmd(&format!("sudo chown postgres:postgres {}", parent_manifest))?;
                interactor.cmd(&format!("sudo chmod 644 {}", parent_manifest))?;
            }
            pgbasebackup_cmd.push_str(&format!(" --incremental={}", parent_manifest));
        } else {
            anyhow::bail!("Cannot perform incremental backup: no previous backup found.");
        }
    }

    // 5. Run pg_basebackup
    println!("Running pg_basebackup command: {}", pgbasebackup_cmd);
    if let Err(e) = interactor.cmd(&pgbasebackup_cmd) {
        let _ = interactor.cmd(&format!("sudo rm -rf {}", local_path));
        anyhow::bail!("pg_basebackup failed: {}", e);
    }

    // 6. Verify Backup (requires extracting tar to a temp directory first)
    let verify_dir = format!("/var/lib/postgresql/backups/{}_verify", id);
    interactor.cmd(&format!("sudo -u postgres mkdir -p {}", verify_dir))?;
    interactor.cmd(&format!(
        "sudo -u postgres tar -xf {}/base.tar -C {}",
        local_path, verify_dir
    ))?;

    // Check if pg_wal.tar exists and extract it
    let test_wal = interactor.cmd(&format!(
        "test -f {}/pg_wal.tar && echo 'yes' || echo 'no'",
        local_path
    ))?;
    if test_wal.trim() == "yes" {
        interactor.cmd(&format!("sudo -u postgres mkdir -p {}/pg_wal", verify_dir))?;
        interactor.cmd(&format!(
            "sudo -u postgres tar -xf {}/pg_wal.tar -C {}/pg_wal/",
            local_path, verify_dir
        ))?;
    }

    // Copy backup_manifest to verify_dir
    interactor.cmd(&format!(
        "sudo cp {}/backup_manifest {}/",
        local_path, verify_dir
    ))?;
    interactor.cmd(&format!("sudo chown -R postgres:postgres {}", verify_dir))?;

    let verify_cmd = format!("sudo -u postgres {} {}", pg_verifybackup, verify_dir);
    println!("Running verifybackup command: {}", verify_cmd);
    let verify_result = interactor.cmd(&verify_cmd);

    // Clean up verify directory
    let _ = interactor.cmd(&format!("sudo rm -rf {}", verify_dir));

    if let Err(e) = verify_result {
        let _ = interactor.cmd(&format!("sudo rm -rf {}", local_path));
        anyhow::bail!("pg_verifybackup verification failed: {}", e);
    }

    // Adjust permissions so that the SSH user can read and download the backup files
    interactor.cmd(&format!("sudo chmod -R 755 {}", local_path))?;

    // 7. Get List of generated backup files to upload
    let files_list = interactor.ls(&local_path)?;
    for file in &files_list {
        let remote_filepath = format!("{}/{}", local_path, file);
        let temp_local_file = std::env::temp_dir().join(format!("crane-backup-{}-{}", id, file));

        // Download from VPS
        interactor.download(temp_local_file.to_str().unwrap(), &remote_filepath)?;

        // Read bytes and upload to S3
        let file_bytes = std::fs::read(&temp_local_file)?;
        let s3_key = format!("backups/{}/{}", id, file);
        s3_client.put_object(&s3_key, &file_bytes)?;

        // Clean up local temp file
        let _ = std::fs::remove_file(temp_local_file);
    }

    // 8. Create Backup Metadata
    let s3_path = format!("{}/backups/{}", bucket_name, id);
    let meta = BackupMetadata {
        id: id.clone(),
        date,
        time,
        backup_type: if is_incr {
            "INCR".to_string()
        } else {
            "FULL".to_string()
        },
        base: base_id,
        local_path: local_path.clone(),
        s3_path,
    };

    // 9. Write metadata descriptor file locally and upload to S3
    let meta_toml = toml::to_string(&meta)?;
    let temp_meta_path = format!("/tmp/metadata_{}.toml", id);
    interactor.create_file(&temp_meta_path, &meta_toml)?;
    interactor.cmd(&format!(
        "sudo mv {} {}/metadata.toml",
        temp_meta_path, local_path
    ))?;
    interactor.cmd(&format!(
        "sudo chown postgres:postgres {}/metadata.toml",
        local_path
    ))?;
    s3_client.put_object(
        &format!("backups/{}/metadata.toml", id),
        meta_toml.as_bytes(),
    )?;

    // 10. Update backup registry on S3 and local
    let registry_key = "backups/registry.toml";
    let mut registry = match s3_client.get_object(registry_key) {
        Ok(data) => {
            let content = String::from_utf8_lossy(&data).to_string();
            toml::from_str::<BackupRegistry>(&content).unwrap_or_default()
        }
        Err(_) => BackupRegistry::default(),
    };

    registry.backups.push(meta.clone());
    let registry_toml = toml::to_string(&registry)?;
    s3_client.put_object(registry_key, registry_toml.as_bytes())?;

    let temp_reg_path = format!("/tmp/registry_{}.toml", id);
    interactor.create_file(&temp_reg_path, &registry_toml)?;
    interactor.cmd(&format!(
        "sudo mv {} /var/lib/postgresql/backups/registry.toml",
        temp_reg_path
    ))?;
    interactor.cmd("sudo chown postgres:postgres /var/lib/postgresql/backups/registry.toml")?;
    interactor.cmd("sudo chmod 644 /var/lib/postgresql/backups/registry.toml")?;

    Ok(meta)
}

pub fn run_restore(
    interactor: &dyn ServerInteractor,
    s3_client: &dyn S3Client,
    pg_version: &str,
    backup: &BackupMetadata,
    chain: &[BackupMetadata],
) -> anyhow::Result<()> {
    let pg_ctl = format!("/usr/lib/postgresql/{}/bin/pg_ctl", pg_version);
    let pg_combinebackup = format!("/usr/lib/postgresql/{}/bin/pg_combinebackup", pg_version);
    let pg_verifybackup = format!("/usr/lib/postgresql/{}/bin/pg_verifybackup", pg_version);
    let pgdata_dir = format!("/var/lib/postgresql/{}/main", pg_version);

    // 1. Stop PostgreSQL service
    let _ = interactor.cmd("sudo systemctl stop postgresql --no-block");
    let _ = interactor.cmd(&format!(
        "sudo systemctl stop postgresql@{}-main --no-block",
        pg_version
    ));
    let _ = interactor.cmd(&format!(
        "sudo -u postgres {} -D {} stop -m immediate",
        pg_ctl, pgdata_dir
    ));

    // 2. Download all backups in the chain from S3 to VPS local backups dir
    interactor.cmd("sudo mkdir -p /var/lib/postgresql/backups")?;
    interactor.cmd("sudo chown postgres:postgres /var/lib/postgresql/backups")?;
    interactor.cmd("sudo chmod 755 /var/lib/postgresql/backups")?;

    for item in chain {
        let remote_dir = format!("/var/lib/postgresql/backups/{}", item.id);
        interactor.cmd(&format!("sudo -u postgres mkdir -p {}", remote_dir))?;
        interactor.cmd(&format!("sudo chmod 755 {}", remote_dir))?;

        let files = vec!["base.tar", "backup_manifest", "pg_wal.tar"];
        for file in files {
            let s3_key = format!("backups/{}/{}", item.id, file);
            if let Ok(data) = s3_client.get_object(&s3_key) {
                let temp_path =
                    std::env::temp_dir().join(format!("crane-restore-{}-{}", item.id, file));
                std::fs::write(&temp_path, &data)?;

                let remote_temp_file = format!("/tmp/crane-restore-{}-{}", item.id, file);
                interactor.upload(temp_path.to_str().unwrap(), &remote_temp_file)?;
                let _ = std::fs::remove_file(temp_path);

                let remote_file = format!("{}/{}", remote_dir, file);
                interactor.cmd(&format!("sudo mv {} {}", remote_temp_file, remote_file))?;
                interactor.cmd(&format!("sudo chown postgres:postgres {}", remote_file))?;
                interactor.cmd(&format!("sudo chmod 644 {}", remote_file))?;
            }
        }
    }

    if chain.len() <= 1 {
        // 3. Clear data directory
        interactor.cmd(&format!("sudo rm -rf {}", pgdata_dir))?;
        interactor.cmd(&format!("sudo -u postgres mkdir -p {}", pgdata_dir))?;
        interactor.cmd(&format!("sudo chmod 700 {}", pgdata_dir))?;

        // 4. Extract base.tar
        interactor.cmd(&format!(
            "sudo -u postgres tar -xf /var/lib/postgresql/backups/{}/base.tar -C {}",
            backup.id, pgdata_dir
        ))?;

        // 5. Extract pg_wal.tar if present
        let test_wal = interactor.cmd(&format!(
            "test -f /var/lib/postgresql/backups/{}/pg_wal.tar && echo 'yes' || echo 'no'",
            backup.id
        ))?;
        if test_wal.trim() == "yes" {
            interactor.cmd(&format!("sudo -u postgres mkdir -p {}/pg_wal", pgdata_dir))?;
            interactor.cmd(&format!(
                "sudo -u postgres tar -xf /var/lib/postgresql/backups/{}/pg_wal.tar -C {}/pg_wal/",
                backup.id, pgdata_dir
            ))?;
        }
    } else {
        // 3. Extract all backups in the chain to separate folders
        for item in chain {
            let extracted_dir = format!("/var/lib/postgresql/backups/{}_extracted", item.id);
            interactor.cmd(&format!("sudo rm -rf {}", extracted_dir))?;
            interactor.cmd(&format!("sudo -u postgres mkdir -p {}", extracted_dir))?;
            interactor.cmd(&format!(
                "sudo -u postgres tar -xf /var/lib/postgresql/backups/{}/base.tar -C {}",
                item.id, extracted_dir
            ))?;

            // Copy backup_manifest to extracted directory so pg_combinebackup can find it
            interactor.cmd(&format!(
                "sudo cp /var/lib/postgresql/backups/{}/backup_manifest {}/",
                item.id, extracted_dir
            ))?;
            interactor.cmd(&format!(
                "sudo chown postgres:postgres {}/backup_manifest",
                extracted_dir
            ))?;
            interactor.cmd(&format!("sudo chmod 644 {}/backup_manifest", extracted_dir))?;
        }

        // 4. Combine backups
        let combined_dir = "/var/lib/postgresql/backups/combined";
        interactor.cmd(&format!("sudo rm -rf {}", combined_dir))?;

        let mut combine_cmd = format!("sudo -u postgres {} ", pg_combinebackup);
        for item in chain {
            combine_cmd.push_str(&format!(
                "/var/lib/postgresql/backups/{}_extracted ",
                item.id
            ));
        }
        combine_cmd.push_str(&format!("-o {}", combined_dir));
        interactor.cmd(&combine_cmd)?;

        // Extract target backup's pg_wal.tar to combined_dir/pg_wal if present
        let test_wal = interactor.cmd(&format!(
            "test -f /var/lib/postgresql/backups/{}/pg_wal.tar && echo 'yes' || echo 'no'",
            backup.id
        ))?;
        if test_wal.trim() == "yes" {
            interactor.cmd(&format!(
                "sudo -u postgres mkdir -p {}/pg_wal",
                combined_dir
            ))?;
            interactor.cmd(&format!(
                "sudo -u postgres tar -xf /var/lib/postgresql/backups/{}/pg_wal.tar -C {}/pg_wal/",
                backup.id, combined_dir
            ))?;
        }

        // 5. Verify the combined backup
        let verify_cmd = format!("sudo -u postgres {} {}", pg_verifybackup, combined_dir);
        interactor.cmd(&verify_cmd)?;

        // 6. Clear and replace data directory with combined backup
        interactor.cmd(&format!("sudo rm -rf {}", pgdata_dir))?;
        interactor.cmd(&format!("sudo mv {} {}", combined_dir, pgdata_dir))?;

        // Clean up extracted directories
        for item in chain {
            let extracted_dir = format!("/var/lib/postgresql/backups/{}_extracted", item.id);
            let _ = interactor.cmd(&format!("sudo rm -rf {}", extracted_dir));
        }
    }

    // 6. Set ownership and start service
    interactor.cmd(&format!("sudo chown -R postgres:postgres {}", pgdata_dir))?;
    interactor.cmd(&format!("sudo chmod 700 {}", pgdata_dir))?;

    // Restart postgres service using direct pg_ctl command (matching setup.rs follower setup)
    let start_cmd = format!(
        "sudo -u postgres {} -D {} -o \"-c config_file=/etc/postgresql/{}/main/postgresql.conf\" start > /dev/null 2>&1 < /dev/null",
        pg_ctl, pgdata_dir, pg_version
    );
    interactor.cmd(&start_cmd)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server_interactor::server_interactor_trait::{ServiceRegister, UserRegister};
    use std::cell::RefCell;
    use std::collections::HashMap;

    struct MockInteractor {
        commands: RefCell<Vec<String>>,
        files: RefCell<HashMap<String, String>>,
        simulated_dates: Vec<String>,
    }

    impl MockInteractor {
        fn new(simulated_dates: Vec<String>) -> Self {
            Self {
                commands: RefCell::new(Vec::new()),
                files: RefCell::new(HashMap::new()),
                simulated_dates,
            }
        }
    }

    impl ServerInteractor for MockInteractor {
        fn whoami(&self) -> anyhow::Result<String> {
            Ok("postgres".to_string())
        }
        fn cmd(&self, command: &str) -> anyhow::Result<String> {
            self.commands.borrow_mut().push(command.to_string());
            if command.contains("date") {
                let count = self
                    .commands
                    .borrow()
                    .iter()
                    .filter(|c| c.contains("date"))
                    .count();
                let date_str = self
                    .simulated_dates
                    .get(count - 1)
                    .cloned()
                    .unwrap_or_else(|| "20251211152749155 2025-12-11 15:27:49".to_string());
                Ok(date_str)
            } else if command.contains("pg_is_in_recovery") {
                Ok("f".to_string())
            } else if command.contains("lsb_release") {
                Ok("distro=debian".to_string())
            } else if command.contains("test -f") {
                if command.contains("registry.toml") {
                    Ok("yes".to_string())
                } else {
                    Ok("no".to_string())
                }
            } else {
                Ok("".to_string())
            }
        }
        fn get_os_info(&self) -> anyhow::Result<String> {
            Ok("Linux".to_string())
        }
        fn create_file(&self, path: &str, content: &str) -> anyhow::Result<()> {
            self.files
                .borrow_mut()
                .insert(path.to_string(), content.to_string());
            Ok(())
        }
        fn read_file(&self, path: &str) -> anyhow::Result<String> {
            self.files
                .borrow_mut()
                .get(path)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("File not found"))
        }
        fn upload(&self, _local_path: &str, _remote_path: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn download(&self, local_path: &str, _remote_path: &str) -> anyhow::Result<()> {
            // Write a dummy file to local_path so std::fs::read works in testing
            std::fs::write(local_path, b"dummy data")?;
            Ok(())
        }
        fn chmod(&self, _path: &str, _permission: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn chown(&self, _path: &str, _user: &str, _group: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn mkdir(&self, _path: &str, _user: &str, _group: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn ls(&self, _path: &str) -> anyhow::Result<Vec<String>> {
            Ok(vec!["base.tar".to_string(), "backup_manifest".to_string()])
        }
        fn install_dependencies(&self, _dependencies: Vec<String>) -> anyhow::Result<()> {
            Ok(())
        }
        fn register_service(&self, _service_register: ServiceRegister) -> anyhow::Result<()> {
            Ok(())
        }
        fn restart_service(&self, _service_name: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn stop_service(&self, _service_name: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn start_service(&self, _service_name: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn status_service(&self, _service_name: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn delete_service(&self, _service_name: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn create_user(&self, _user_register: UserRegister) -> anyhow::Result<()> {
            Ok(())
        }
        fn delete_user(&self, _username: &str) -> anyhow::Result<()> {
            Ok(())
        }
        fn add_user_to_groups(&self, _username: &str, _groups: Vec<String>) -> anyhow::Result<()> {
            Ok(())
        }
        fn remove_user_from_groups(
            &self,
            _username: &str,
            _groups: Vec<String>,
        ) -> anyhow::Result<()> {
            Ok(())
        }
        fn list_users(&self) -> anyhow::Result<Vec<String>> {
            Ok(vec![])
        }
    }

    struct MockS3Client {
        objects: RefCell<HashMap<String, Vec<u8>>>,
    }

    impl MockS3Client {
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

    #[test]
    fn test_backup_full() {
        let interactor =
            MockInteractor::new(vec!["20251211152749155 2025-12-11 15:27:49".to_string()]);
        let s3_client = MockS3Client::new();
        let result = run_backup(
            &interactor,
            &s3_client,
            "17",
            "full",
            "password123",
            "crane1",
            None,
        );
        assert!(result.is_ok());
        let meta = result.unwrap();
        assert_eq!(meta.backup_type, "FULL");
        assert!(meta.base.is_none());
        assert_eq!(meta.id, "20251211152749155");
        assert_eq!(meta.s3_path, "crane1/backups/20251211152749155");

        // Verify that pg_basebackup command was executed with the expected arguments
        let cmds = interactor.commands.borrow();
        assert!(cmds.iter().any(|c| {
            c.contains("pg_basebackup")
                && c.contains("-F t")
                && c.contains("-X s")
                && c.contains("-U replicator")
                && !c.contains("--incremental")
        }));

        // Verify that objects were uploaded to MockS3
        let objects = s3_client.objects.borrow();
        assert!(objects.contains_key("backups/20251211152749155/base.tar"));
        assert!(objects.contains_key("backups/20251211152749155/backup_manifest"));
        assert!(objects.contains_key("backups/20251211152749155/metadata.toml"));
        assert!(objects.contains_key("backups/registry.toml"));
    }

    #[test]
    fn test_backup_incremental() {
        let interactor = MockInteractor::new(vec![
            "20251211152949394 2025-12-11 15:29:49".to_string(),
            "20251211153049555 2025-12-11 15:30:49".to_string(),
        ]);
        let s3_client = MockS3Client::new();

        // 1. Run full backup
        let result_full = run_backup(
            &interactor,
            &s3_client,
            "17",
            "full",
            "password123",
            "crane1",
            None,
        );
        assert!(
            result_full.is_ok(),
            "full backup failed: {:?}",
            result_full.err()
        );
        let meta_full = result_full.unwrap();

        // 2. Run incremental backup
        let result_incr = run_backup(
            &interactor,
            &s3_client,
            "17",
            "incr",
            "password123",
            "crane1",
            Some(&meta_full),
        );
        assert!(result_incr.is_ok());
        let meta_incr = result_incr.unwrap();

        assert_eq!(meta_incr.backup_type, "INCR");
        assert_eq!(meta_incr.base, Some(meta_full.id.clone()));
        assert_eq!(meta_incr.id, "20251211153049555");
        assert_eq!(meta_incr.s3_path, "crane1/backups/20251211153049555");

        // Verify that pg_basebackup was called with --incremental pointing to parent manifest
        let cmds = interactor.commands.borrow();
        assert!(cmds.iter().any(|c| {
            c.contains("pg_basebackup")
                && c.contains(
                    "--incremental=/var/lib/postgresql/backups/20251211152949394/backup_manifest",
                )
        }));

        // Verify that since test -f returned 'no', it downloaded the parent manifest from S3 and uploaded it to VPS
        assert!(cmds.iter().any(|c| {
            c.contains("mv /tmp/manifest_20251211152949394")
                && c.contains("/var/lib/postgresql/backups/20251211152949394/backup_manifest")
        }));
    }

    #[test]
    fn test_restore_full() {
        let interactor = MockInteractor::new(vec![]);
        let s3_client = MockS3Client::new();

        let backup = BackupMetadata {
            id: "20251211152749155".to_string(),
            date: "2025-12-11".to_string(),
            time: "15:27:49".to_string(),
            backup_type: "FULL".to_string(),
            base: None,
            local_path: "/var/lib/postgresql/backups/20251211152749155".to_string(),
            s3_path: "crane1/backups/20251211152749155".to_string(),
        };

        // Populate MockS3 with files
        s3_client
            .put_object("backups/20251211152749155/base.tar", b"dummy base")
            .unwrap();
        s3_client
            .put_object(
                "backups/20251211152749155/backup_manifest",
                b"dummy manifest",
            )
            .unwrap();

        let result = run_restore(&interactor, &s3_client, "17", &backup, &[backup.clone()]);
        assert!(result.is_ok());

        let cmds = interactor.commands.borrow();
        // Verify stop postgres
        assert!(cmds.iter().any(|c| c.contains("systemctl stop postgresql")));
        // Verify rm main data dir
        assert!(
            cmds.iter()
                .any(|c| c.contains("rm -rf /var/lib/postgresql/17/main"))
        );
        // Verify tar extraction
        assert!(
            cmds.iter()
                .any(|c| c.contains("tar -xf") && c.contains("/var/lib/postgresql/17/main"))
        );

        // Verify start postgres
        assert!(
            cmds.iter()
                .any(|c| c.contains("start") && c.contains("/var/lib/postgresql/17/main"))
        );
    }

    #[test]
    fn test_restore_incremental() {
        let interactor = MockInteractor::new(vec![]);
        let s3_client = MockS3Client::new();

        let full_backup = BackupMetadata {
            id: "20251211152749155".to_string(),
            date: "2025-12-11".to_string(),
            time: "15:27:49".to_string(),
            backup_type: "FULL".to_string(),
            base: None,
            local_path: "/var/lib/postgresql/backups/20251211152749155".to_string(),
            s3_path: "crane1/backups/20251211152749155".to_string(),
        };

        let incr_backup = BackupMetadata {
            id: "20251211152849281".to_string(),
            date: "2025-12-11".to_string(),
            time: "15:28:49".to_string(),
            backup_type: "INCR".to_string(),
            base: Some("20251211152749155".to_string()),
            local_path: "/var/lib/postgresql/backups/20251211152849281".to_string(),
            s3_path: "crane1/backups/20251211152849281".to_string(),
        };

        // Populate MockS3 with files
        s3_client
            .put_object("backups/20251211152749155/base.tar", b"dummy base")
            .unwrap();
        s3_client
            .put_object(
                "backups/20251211152749155/backup_manifest",
                b"dummy manifest",
            )
            .unwrap();
        s3_client
            .put_object("backups/20251211152849281/base.tar", b"dummy base incr")
            .unwrap();
        s3_client
            .put_object(
                "backups/20251211152849281/backup_manifest",
                b"dummy manifest incr",
            )
            .unwrap();

        let result = run_restore(
            &interactor,
            &s3_client,
            "17",
            &incr_backup,
            &[full_backup.clone(), incr_backup.clone()],
        );
        assert!(result.is_ok());

        let cmds = interactor.commands.borrow();
        // Verify stop postgres
        assert!(cmds.iter().any(|c| c.contains("systemctl stop postgresql")));
        // Verify pg_combinebackup command is executed
        assert!(cmds.iter().any(|c| {
            c.contains("pg_combinebackup")
                && c.contains("/var/lib/postgresql/backups/20251211152749155_extracted")
                && c.contains("/var/lib/postgresql/backups/20251211152849281_extracted")
                && c.contains("-o /var/lib/postgresql/backups/combined")
        }));

        // Verify start postgres
        assert!(
            cmds.iter()
                .any(|c| c.contains("start") && c.contains("/var/lib/postgresql/17/main"))
        );
    }
}
