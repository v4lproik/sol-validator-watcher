use super::*;
#[test]
fn expands_env_variables_with_defaults() -> Result<()> {
    let expanded = expand_env_vars_with(
        r#"
network = "${WATCHER_NETWORK:-localnet}"
poll_interval_sec = ${WATCHER_POLL_INTERVAL_SEC:-15}
rpc_url = "${WATCHER_RPC_URL}"
"#,
        |key| {
            let value = match key {
                "WATCHER_RPC_URL" => Some("http://validator:8899".to_string()),
                _ => None,
            };
            Ok(value)
        },
    )?;

    assert!(expanded.contains("network = \"localnet\""));
    assert!(expanded.contains("poll_interval_sec = 15"));
    assert!(expanded.contains("rpc_url = \"http://validator:8899\""));
    Ok(())
}

#[test]
fn rejects_missing_env_variables_without_defaults() -> Result<()> {
    let error = match expand_env_vars_with("rpc_url = \"${WATCHER_RPC_URL}\"", |_| Ok(None)) {
        Ok(_) => anyhow::bail!("expansion should fail"),
        Err(error) => error,
    };

    assert!(error
        .to_string()
        .contains("environment variable WATCHER_RPC_URL is required but not set"));
    Ok(())
}

#[test]
fn uses_default_for_empty_env_values() -> Result<()> {
    let expanded = expand_env_vars_with(
        "poll_interval_sec = ${WATCHER_POLL_INTERVAL_SEC:-15}",
        |_| Ok(Some(String::new())),
    )?;

    assert_eq!(expanded, "poll_interval_sec = 15");
    Ok(())
}

#[test]
fn rejects_unterminated_env_expressions() -> Result<()> {
    let error = match expand_env_vars_with("rpc_url = \"${WATCHER_RPC_URL\"", |_| Ok(None)) {
        Ok(_) => anyhow::bail!("expansion should fail"),
        Err(error) => error,
    };

    assert!(error
        .to_string()
        .contains("unterminated environment variable expression"));
    Ok(())
}
