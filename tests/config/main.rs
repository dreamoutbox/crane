use crane::config::resolve_placeholders;
use std::collections::HashMap;

#[test]
fn test_resolve_placeholders_success() {
    let mut env = HashMap::new();
    env.insert("FOO".to_string(), "bar".to_string());
    env.insert("BAZ".to_string(), "qux".to_string());

    // Test resolving from dot_env
    let input = "key = \"${FOO}\" and another_key = \"${BAZ}\"";
    let output = resolve_placeholders(input, &env).unwrap();
    assert_eq!(output, "key = \"bar\" and another_key = \"qux\"");

    // Test resolving from shell env
    unsafe {
        std::env::set_var("SHELL_VAR", "shell_val");
    }
    let input2 = "key = \"${SHELL_VAR}\"";
    let output2 = resolve_placeholders(input2, &env).unwrap();
    assert_eq!(output2, "key = \"shell_val\"");

    // Test shell env overrides dot_env
    env.insert("OVERRIDE_VAR".to_string(), "file_val".to_string());
    unsafe {
        std::env::set_var("OVERRIDE_VAR", "env_val");
    }
    let input3 = "override = \"${OVERRIDE_VAR}\"";
    let output3 = resolve_placeholders(input3, &env).unwrap();
    assert_eq!(output3, "override = \"env_val\"");
}

#[test]
fn test_resolve_placeholders_missing_error() {
    let env = HashMap::new();
    let input = "missing = \"${NOT_DEFINED}\"";
    let res = resolve_placeholders(input, &env);
    assert!(res.is_err());
    let err_msg = res.err().unwrap().to_string();
    assert!(err_msg.contains("Environment variable 'NOT_DEFINED' not found"));
}

#[test]
fn test_resolve_placeholders_unclosed_error() {
    let env = HashMap::new();
    let input = "unclosed = \"${FOO\"";
    let res = resolve_placeholders(input, &env);
    assert!(res.is_err());
    assert!(
        res.err()
            .unwrap()
            .to_string()
            .contains("Unclosed placeholder")
    );
}
