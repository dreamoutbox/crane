use crane::postgres_unit::backup::run_backup;
use crane::postgres_unit::entity::BackupMetadata;
use crane::s3::s3_client::S3Client;
use crane::server_interactor::server_interactor_trait::ServerInteractor;
use crane::ssh::CmdOutput;

mod postgres {
    use super::*;
    use crane::server_interactor::server_interactor_trait::{ServiceRegister, UserRegister};
    use std::cell::RefCell;
    use std::collections::HashMap;

    include!("../common/mock_interactor.rs");
    include!("../common/mock_s3.rs");
    include!("helper.rs");

    include!("backup_test.rs");
    include!("restore_test.rs");
    include!("config_test.rs");
    include!("logs_test.rs");
    include!("restore_integrated_test.rs");
    include!("user_change_password_test.rs");
    include!("user_state_test.rs");
}
