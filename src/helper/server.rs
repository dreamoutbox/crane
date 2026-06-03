use crate::server_interactor::server_interactor_trait::ServerInteractor;

pub fn wait_for_service_status(
    interactor: &dyn ServerInteractor,
    service_name: &str,
    service_status: &str,
    timeout: u64,
) -> anyhow::Result<bool> {
    let start_time = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(timeout);

    let mut met_status = false;

    while start_time.elapsed() < timeout {
        let status = interactor.cmd(&format!("sudo systemctl is-active {}", service_name))?;

        // dbg!(&service_name, &status);

        if status.stdout.trim() == service_status {
            met_status = true;
            break;
        }

        std::thread::sleep(std::time::Duration::from_millis(1000));
    }

    Ok(met_status)
}
