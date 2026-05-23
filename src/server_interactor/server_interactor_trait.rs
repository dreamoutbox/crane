pub trait ServerInteractor {
    // TEST
    fn whoami(&self) -> anyhow::Result<String>;

    // BASIC
    fn cmd(&self, command: &str) -> anyhow::Result<String>;
    fn get_os_info(&self) -> anyhow::Result<String>;

    // FILES
    fn create_file(&self, path: &str, content: &str) -> anyhow::Result<()>;
    fn read_file(&self, path: &str) -> anyhow::Result<String>;
    fn upload(&self, local_path: &str, remote_path: &str) -> anyhow::Result<()>;
    fn download(&self, local_path: &str, remote_path: &str) -> anyhow::Result<()>;
    fn chmod(&self, path: &str, permission: &str) -> anyhow::Result<()>;
    fn chown(&self, path: &str, user: &str, group: &str) -> anyhow::Result<()>;
}
