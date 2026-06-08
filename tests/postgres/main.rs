#[path = "../common/mock_interactor.rs"]
pub mod mock_interactor;

#[path = "../common/MockServerInteractorLogsRecorder.rs"]
pub mod mock_interactor_log_recorder;

#[path = "../common/mock_s3.rs"]
pub mod mock_s3;

#[path = "../common/MockServerInteractorUserNotExist.rs"]
pub mod mock_interactor_user_not_exist;

#[path = "../common/helper.rs"]
pub mod common_helper;

pub mod helper;

mod backup_restore_extend_test;
mod backup_restore_test;
mod config_test;
mod database_persist_test;
mod failover_test;
mod logs_test;
mod promote_test;
mod python_backup_script_test;
mod user_change_password_test;
mod user_state_test;
