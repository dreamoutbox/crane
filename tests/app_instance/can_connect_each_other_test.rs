use std::path::Path;
use std::process::Command;

// RUN:
// cargo nextest run --test app_instance --nocapture

#[test]
fn test_app_instances_can_connect_each_other() {
    // 1. Build Go demo app
    let go_build = Command::new("go")
        .arg("build")
        .current_dir("demo")
        .output()
        .expect("failed to run go build");
    assert!(
        go_build.status.success(),
        "go build failed: {}",
        String::from_utf8_lossy(&go_build.stderr)
    );

    // 2. Deploy app configuration to VPS nodes
    let config_path = Path::new("demo/crane.toml");
    crane::commands::deploy::run(config_path, crane::server_interactor::get_interactor)
        .expect("deploy failed");

    // 3. Test myapp to myapp2 connectivity
    let curl1 = Command::new("curl")
        .args([
            "-w",
            "\\n",
            "-L",
            "-k",
            "-i",
            "--resolve",
            "myapp.localhost:80:127.0.0.1",
            "--resolve",
            "myapp.localhost:443:127.0.0.1",
            "--resolve",
            "myapp2.localhost:80:127.0.0.1",
            "--resolve",
            "myapp2.localhost:443:127.0.0.1",
            "http://myapp.localhost/curl?to=myapp2",
        ])
        .output()
        .expect("failed to execute curl myapp -> myapp2");

    let stdout1 = String::from_utf8_lossy(&curl1.stdout);
    assert!(curl1.status.success(), "curl myapp -> myapp2 failed");
    assert!(
        stdout1.contains("Hello, myapp2!"),
        "expected 'Hello, myapp2!' in response, got: {}",
        stdout1
    );

    // 4. Test myapp2 to myapp connectivity
    let curl2 = Command::new("curl")
        .args([
            "-w",
            "\\n",
            "-L",
            "-k",
            "-i",
            "--resolve",
            "myapp.localhost:80:127.0.0.1",
            "--resolve",
            "myapp.localhost:443:127.0.0.1",
            "--resolve",
            "myapp2.localhost:80:127.0.0.1",
            "--resolve",
            "myapp2.localhost:443:127.0.0.1",
            "http://myapp2.localhost/curl?to=myapp",
        ])
        .output()
        .expect("failed to execute curl myapp2 -> myapp");

    let stdout2 = String::from_utf8_lossy(&curl2.stdout);
    assert!(curl2.status.success(), "curl myapp2 -> myapp failed");
    assert!(
        stdout2.contains("Hello, myapp!"),
        "expected 'Hello, myapp!' in response, got: {}",
        stdout2
    );
}
