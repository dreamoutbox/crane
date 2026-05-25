# Testing

we use cargo nextest to run tests.

## Run tests

```sh
cargo nextest run --test traefik -- test_traefik_install_config --nocapture
```

## NOTE:

- please specify `--test <TEST_NAME>` everytime to build only that test, this can speed up test execution significantly.

- test specific test by specify test function name after `--`

- always add `--no-capture` to see the output of the test.

- DO NOT RUN without specifying `--test <TEST_NAME>`, it will FREEZE the machine.
