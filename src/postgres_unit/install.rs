use crate::server_interactor::server_interactor_trait::ServerInteractor;

pub fn install_postgres(interactor: &dyn ServerInteractor, version: &str) -> anyhow::Result<()> {
    interactor.install_postgres(version)
}

// Init pgdata if not exists
// let main_dir = format!("/var/lib/postgresql/{}/main", version);
// let dir_exists = interactor
//     .cmd(&format!("test -d {}", main_dir))
//     .map(|out| out.exit_code == 0)
//     .unwrap_or(false);
// if !dir_exists {
//     println!(
//         "\tPostgreSQL {} data directory is missing, initializing it...",
//         version
//     );
//     let initdb_cmd = format!(
//         "sudo -u postgres /usr/lib/postgresql/{}/bin/initdb -D {}",
//         version, main_dir
//     );

//     let _initdb_res = interactor.cmd(&initdb_cmd)?;
//     // dbg!(initdb_res);
// }

// let status_cmd = format!(
//     "sudo -u postgres {} -D /var/lib/postgresql/{}/main status",
//     pg_ctl, version
// );

// let is_running = interactor
//     .cmd(&status_cmd)
//     .map(|out| out.exit_code == 0)
//     .unwrap_or(false);
// if !is_running {
//     println!("\tPostgreSQL {} is stopped, starting it...", version);
//     let start_cmd = format!(
//         "sudo -u postgres {} -D /var/lib/postgresql/{}/main -o \"-c config_file=/etc/postgresql/{}/main/postgresql.conf -c restore_command=false\" start > /dev/null 2>&1 < /dev/null",
//         pg_ctl, version, version
//     );
//     let _ = interactor.cmd(&start_cmd);
// }
