use crane::postgres_unit::backup::{run_backup, run_restore};
use crane::postgres_unit::entity::BackupMetadata;
use crane::s3::s3_client::S3Client;
use crane::server_interactor::server_interactor_trait::ServerInteractor;

mod postgres_backup {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use crane::server_interactor::server_interactor_trait::{ServiceRegister, UserRegister};

    include!("common/mock_interactor.rs");
    include!("common/mock_s3.rs");
    include!("postgres_backup/backup_test.rs");
}
