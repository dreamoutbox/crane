use crane::postgres_unit::helper::get_postgres_configs;

#[test]
fn test_get_postgres_configs() {
    let toml_str = r#"
            [[nodes]]
            name = "vps1"
            host = "localhost"
            public_ip = "localhost"
            internal_ip = "10.0.0.11"
            port = 2221
            user = "admin"
            roles = ["postgres"]
            private_key = "dummy"

            [app.myapp]
            name = "myapp"
            deploy_dir = "./demo"
            entrypoint = "./myapp"
            deploy_user = "deployman"
            port_start = 3000
            instances = 1

            [db.postgres]
            enabled = true
            version = "17"
            replica_pass = "replica"

            [[db.postgres.users]]
            user = "app1"
            password = "app1"
            databases = ["myapp"]

            [db.postgres.myapp]
            name = "myapp"
        "#;
    let config: crane::config::Config = toml::from_str(toml_str).unwrap();
    let (dbs, users) = get_postgres_configs(&config);

    assert_eq!(dbs.len(), 1);
    assert_eq!(dbs[0].name, "myapp");

    assert_eq!(users.len(), 1);
    assert_eq!(users[0].user, "app1");
    assert_eq!(users[0].password, Some("app1".to_string()));
    assert_eq!(users[0].databases, vec!["myapp".to_string()]);
}

#[test]
fn test_interval_to_cron() {
    use crane::helper::cron::interval_to_cron;

    assert_eq!(interval_to_cron("1m"), "* * * * *");
    assert_eq!(interval_to_cron("5m"), "*/5 * * * *");
    assert_eq!(interval_to_cron("1h"), "0 * * * *");
    assert_eq!(interval_to_cron("2h"), "0 */2 * * *");
    assert_eq!(interval_to_cron("1d"), "0 0 * * *");
    assert_eq!(interval_to_cron("3d"), "0 0 */3 * *");
    assert_eq!(interval_to_cron("invalid"), "0 0 * * *");
}
