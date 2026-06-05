use crane::config::read_config_toml_file;

#[test]
fn test_parse_config() {
    let config_file = "demo/crane.toml";
    let config_path = std::path::Path::new(config_file);
    let parse_config_result = read_config_toml_file(config_path);

    assert!(
        parse_config_result.is_ok(),
        "should parse config ok, but got error: {:?}",
        parse_config_result.unwrap_err()
    );
}
