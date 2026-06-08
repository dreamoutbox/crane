pub fn debug_get_pg_logs(interactor: &dyn ServerInteractor, pg_version: &str) -> String {
    let log_dir = format!("/var/lib/postgresql/{}/main/log", pg_version);
    let find_logs_cmd = format!(
        "sudo find {} -maxdepth 1 -type f \\( -name \"*.log\" -o -name \"*.csv\" \\) -printf \"%T@ %p\\n\" 2>/dev/null | sort -n -r | head -n 5 | cut -d' ' -f2-",
        log_dir
    );

    let mut extra_logs = String::new();
    let mut file_paths = Vec::new();

    if let Ok(find_out) = interactor.cmd(&find_logs_cmd) {
        for line in find_out.stdout.lines() {
            let p = line.trim();
            if !p.is_empty() {
                file_paths.push(p.to_string());
            }
        }
    }

    // Fallback if find output is empty
    if file_paths.is_empty() {
        let fallback_cmd = format!(
            "sudo ls -t {}/*.log {}/*.csv 2>/dev/null | head -n 5",
            log_dir, log_dir
        );
        if let Ok(fb_out) = interactor.cmd(&fallback_cmd) {
            for line in fb_out.stdout.lines() {
                let p = line.trim();
                if !p.is_empty() {
                    file_paths.push(p.to_string());
                }
            }
        }
    }

    for file_path in file_paths {
        extra_logs.push_str(&format!("\n--- Last 50 lines of {} ---\n", file_path));
        let cat_cmd = format!("sudo tail -n 50 '{}'", file_path);

        if let Ok(cat_out) = interactor.cmd(&cat_cmd) {
            extra_logs.push_str(&cat_out.stdout);
        }
    }

    extra_logs
}
