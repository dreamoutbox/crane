pub struct ServerPath {
    pub app_dir: String,
    pub app_config_dir: String,
    pub pg_dir: String,
    pub pg_data_dir: String,
    pub pg_bin_dir: String,
    pub pg_pass_path: String,
    // PATRONI
    pub patroni_config_path: String,
    // HAPROXY
    pub haproxy_var_lib_dir: String,
    pub haproxy_config_path: String,
    pub haproxy_log_dir: String,
}
