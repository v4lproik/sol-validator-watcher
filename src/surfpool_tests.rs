use crate::{
    collector::{collect_snapshot, RpcClient},
    config::Config,
    metrics::Metrics,
};
use anyhow::{bail, Context, Result};
use std::{
    env, fs,
    fs::File,
    net::TcpListener,
    path::PathBuf,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[test]
fn surfpool_smoke_test_collects_snapshot_and_renders_metrics() -> Result<()> {
    if !surfpool_e2e_enabled() {
        eprintln!("skipping surfpool e2e; set SURFPOOL_E2E=1 to run it");
        return Ok(());
    }

    let harness = SurfpoolHarness::spawn()?;
    let config = Config {
        network: "localnet".to_string(),
        rpc_url: harness.rpc_url(),
        rpc_timeout_sec: 5,
        metrics_port: 8000,
        poll_interval_sec: 1,
        commitment: "confirmed".to_string(),
        delinquent_slot_distance: Some(128),
        liveness_file: None,
        watched_validators: vec![],
    };
    let client = RpcClient::new(
        config.rpc_url.clone(),
        Duration::from_secs(config.rpc_timeout_sec),
        config.commitment_config()?,
    )?;

    let runtime = tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;
    let snapshot = runtime
        .block_on(collect_snapshot(&client, &config))
        .context("failed to collect snapshot from surfpool")?;

    let metrics = Metrics::new()?;
    metrics.set_watcher_up(&config.network, true);
    metrics.set_rpc_up(&config.network, true);
    metrics.update_snapshot(&snapshot);
    let rendered = metrics.render()?;

    assert_eq!(snapshot.network, "localnet");
    assert!(snapshot.slot >= snapshot.epoch);
    assert!(rendered.contains("sol_slot"));
    assert!(rendered.contains("sol_epoch"));
    assert!(rendered.contains("sol_watcher_up"));
    assert!(rendered.contains("sol_rpc_up"));

    drop(runtime);
    drop(harness);
    Ok(())
}

fn surfpool_e2e_enabled() -> bool {
    matches!(
        env::var("SURFPOOL_E2E").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

struct SurfpoolHarness {
    child: Child,
    workdir: PathBuf,
    rpc_port: u16,
    stdout_path: PathBuf,
    stderr_path: PathBuf,
}

impl SurfpoolHarness {
    fn spawn() -> Result<Self> {
        ensure_surfpool_installed()?;

        let rpc_port = find_free_port()?;
        let ws_port = find_free_port()?;
        let studio_port = find_free_port()?;
        let workdir = make_temp_workdir()?;
        let manifest_path = workdir.join("txtx.yml");
        let stdout_path = workdir.join("surfpool.stdout.log");
        let stderr_path = workdir.join("surfpool.stderr.log");

        let stdout = File::create(&stdout_path)
            .with_context(|| format!("failed to create {}", stdout_path.display()))?;
        let stderr = File::create(&stderr_path)
            .with_context(|| format!("failed to create {}", stderr_path.display()))?;

        let child = Command::new("surfpool")
            .arg("start")
            .arg("--offline")
            .arg("--ci")
            .arg("--no-deploy")
            .arg("--manifest-file-path")
            .arg(&manifest_path)
            .arg("--port")
            .arg(rpc_port.to_string())
            .arg("--ws-port")
            .arg(ws_port.to_string())
            .arg("--studio-port")
            .arg(studio_port.to_string())
            .current_dir(&workdir)
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr))
            .spawn()
            .context("failed to spawn surfpool")?;

        let mut harness = Self {
            child,
            workdir,
            rpc_port,
            stdout_path,
            stderr_path,
        };
        harness.wait_until_ready(Duration::from_secs(20))?;
        Ok(harness)
    }

    fn rpc_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.rpc_port)
    }

    fn wait_until_ready(&mut self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        let runtime = tokio::runtime::Runtime::new().context("failed to create tokio runtime")?;
        let client = RpcClient::new(
            self.rpc_url(),
            Duration::from_secs(2),
            "confirmed".to_string(),
        )?;

        loop {
            if let Some(status) = self
                .child
                .try_wait()
                .context("failed checking surfpool process state")?
            {
                bail!(
                    "surfpool exited before becoming ready with status {}\n{}",
                    status,
                    self.log_output()
                );
            }

            if runtime.block_on(client.probe()).is_ok() {
                return Ok(());
            }

            if Instant::now() >= deadline {
                bail!(
                    "surfpool did not become ready within {:?}\n{}",
                    timeout,
                    self.log_output()
                );
            }

            thread::sleep(Duration::from_millis(250));
        }
    }

    fn log_output(&self) -> String {
        let stdout = fs::read_to_string(&self.stdout_path).unwrap_or_default();
        let stderr = fs::read_to_string(&self.stderr_path).unwrap_or_default();
        format!(
            "stdout:\n{}\n\nstderr:\n{}\nworkdir: {}",
            stdout,
            stderr,
            self.workdir.display()
        )
    }
}

impl Drop for SurfpoolHarness {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.workdir);
    }
}

fn ensure_surfpool_installed() -> Result<()> {
    let status = Command::new("surfpool")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("failed to execute surfpool --version")?;

    if !status.success() {
        bail!("surfpool CLI is not available");
    }

    Ok(())
}

fn find_free_port() -> Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0").context("failed to allocate local port")?;
    let port = listener
        .local_addr()
        .context("failed to read local port")?
        .port();
    drop(listener);
    Ok(port)
}

fn make_temp_workdir() -> Result<PathBuf> {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before UNIX_EPOCH")?
        .as_nanos();
    let workdir = env::temp_dir().join(format!(
        "sol-validator-watcher-surfpool-{}-{}",
        std::process::id(),
        unique
    ));
    fs::create_dir_all(&workdir)
        .with_context(|| format!("failed to create {}", workdir.display()))?;
    Ok(workdir)
}
