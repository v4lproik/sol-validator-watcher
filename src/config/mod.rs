//! Self-contained TOML profile loading, normalization, and validation.
//!
//! Configuration is expanded and validated before runtime construction. Command
//! arrays are the complete ordered fallback policy for their commands.

use crate::mode::{CommandId, ModeId, ProviderFeature, SolanaRpcCompatibility, Transport};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashSet},
    env, fmt, fs,
    net::{IpAddr, Ipv4Addr},
    path::{Path, PathBuf},
    time::Duration,
};

/// Reserved label that identifies the validator used for economics collection.
pub const ECONOMICS_TARGET_LABEL: &str = "focus:economics";

const DEFAULT_RPC_RESPONSE_SIZE_LIMIT_BYTES: usize = 32 * 1024 * 1024;
const MAX_RPC_RESPONSE_SIZE_LIMIT_BYTES: usize = 128 * 1024 * 1024;

/// Ordered connector routes for health commands.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HealthCommands {
    /// Connector names used for HTTP RPC health, in fallback order.
    pub rpc_health: Vec<String>,
    /// Connector names used for WebSocket health, in fallback order.
    pub ws_health: Vec<String>,
    /// Connector names used for node-local status, in fallback order.
    pub node_status: Vec<String>,
}

/// Ordered connector routes for consensus commands.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConsensusCommands {
    /// Connector names used for epoch information, in fallback order.
    pub epoch_info: Vec<String>,
    /// Connector names used for vote accounts, in fallback order.
    pub vote_accounts: Vec<String>,
    /// Connector names used for public gossip peers and version metadata, in fallback order.
    pub cluster_nodes: Vec<String>,
    /// Connector names used for recent performance samples, in fallback order.
    pub recent_performance_samples: Vec<String>,
}

/// Ordered connector routes for leadership commands.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LeadershipCommands {
    /// Connector names used for current block production, in fallback order.
    pub current_block_production: Vec<String>,
    /// Connector names used for the current leader schedule, in fallback order.
    pub current_leader_schedule: Vec<String>,
    /// Connector names used for the next leader schedule, in fallback order.
    pub next_leader_schedule: Vec<String>,
}

/// Ordered connector routes for economics commands.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct EconomicsCommands {
    /// Connector names used for identity and vote-account balances.
    pub account_balances: Vec<String>,
    /// Connector names used for prior-epoch inflation rewards.
    pub inflation_rewards: Vec<String>,
    /// Connector names used for prior-epoch block production.
    pub reward_epoch_block_production: Vec<String>,
    /// Connector names used for recent confirmed block-fee rewards.
    pub recent_block_fee_rewards: Vec<String>,
    /// Connector names used for settled Jito MEV rewards.
    pub jito_mev_rewards: Vec<String>,
}

/// Health-mode command routes.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct HealthModeConfig {
    /// Commands owned by health mode.
    pub commands: HealthCommands,
}

/// Consensus-mode command routes.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ConsensusModeConfig {
    /// Commands owned by consensus mode.
    pub commands: ConsensusCommands,
}

/// Leadership-mode command routes.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct LeadershipModeConfig {
    /// Commands owned by leadership mode.
    pub commands: LeadershipCommands,
}

/// Optional single-validator economics mode and its command routes.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct EconomicsModeConfig {
    /// Whether economics collection is active.
    pub enabled: bool,
    /// Explicit economics target, or `None` to resolve it from the reserved label.
    pub validator: Option<WatchedValidatorConfig>,
    /// Commands owned by economics mode.
    pub commands: EconomicsCommands,
}

impl EconomicsModeConfig {
    /// Returns the stable mode label used in configuration metadata.
    pub const fn as_str(&self) -> &'static str {
        if self.enabled {
            "single-validator"
        } else {
            "disabled"
        }
    }
}

/// Complete collection-mode configuration.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModesConfig {
    /// Health-mode routes.
    pub health: HealthModeConfig,
    /// Consensus-mode routes.
    pub consensus: ConsensusModeConfig,
    /// Leadership-mode routes.
    pub leadership: LeadershipModeConfig,
    /// Economics-mode enablement, target, and routes.
    pub economics: EconomicsModeConfig,
}

impl ModesConfig {
    /// Returns the ordered connector names configured for a command.
    pub fn route(&self, command: CommandId) -> &[String] {
        match command {
            CommandId::RpcHealth => &self.health.commands.rpc_health,
            CommandId::WsHealth => &self.health.commands.ws_health,
            CommandId::NodeStatus => &self.health.commands.node_status,
            CommandId::EpochInfo => &self.consensus.commands.epoch_info,
            CommandId::VoteAccounts => &self.consensus.commands.vote_accounts,
            CommandId::ClusterNodes => &self.consensus.commands.cluster_nodes,
            CommandId::RecentPerformanceSamples => {
                &self.consensus.commands.recent_performance_samples
            }
            CommandId::CurrentBlockProduction => &self.leadership.commands.current_block_production,
            CommandId::CurrentLeaderSchedule => &self.leadership.commands.current_leader_schedule,
            CommandId::NextLeaderSchedule => &self.leadership.commands.next_leader_schedule,
            CommandId::AccountBalances => &self.economics.commands.account_balances,
            CommandId::InflationRewards => &self.economics.commands.inflation_rewards,
            CommandId::RewardEpochBlockProduction => {
                &self.economics.commands.reward_epoch_block_production
            }
            CommandId::JitoMevRewards => &self.economics.commands.jito_mev_rewards,
            CommandId::RecentBlockFeeRewards => &self.economics.commands.recent_block_fee_rewards,
        }
    }

    /// Reports whether a mode participates in collection.
    pub const fn enabled(&self, mode: ModeId) -> bool {
        match mode {
            ModeId::Health | ModeId::Consensus | ModeId::Leadership => true,
            ModeId::Economics => self.economics.enabled,
        }
    }
}

/// Validator selector and metric labels from one `[[watched_validators]]` entry.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WatchedValidatorConfig {
    /// Optional identity account address.
    pub identity: Option<String>,
    /// Optional vote-account address.
    pub vote_account: Option<String>,
    #[serde(default)]
    /// Additional validated `category:value` labels.
    pub labels: Vec<String>,
}

/// Custom request headers whose values are always redacted from debug output.
#[derive(Clone, Default, Deserialize, Eq, PartialEq)]
#[serde(transparent)]
pub struct TransportHeaders {
    values: BTreeMap<String, String>,
}

struct Redacted;

impl fmt::Debug for Redacted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

impl fmt::Debug for TransportHeaders {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut map = formatter.debug_map();
        for name in self.values.keys() {
            map.entry(name, &Redacted);
        }
        map.finish()
    }
}

impl TransportHeaders {
    /// Returns configured header names and values for transport construction.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.values
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }

    fn into_normalized(self, path: &str, reserved: &[&str]) -> Result<Self> {
        let mut values = BTreeMap::new();
        for (name, value) in self.values {
            let name = name.trim().to_ascii_lowercase();
            reqwest::header::HeaderName::from_bytes(name.as_bytes())
                .with_context(|| format!("{path} contains invalid header name {name:?}"))?;
            value
                .parse::<reqwest::header::HeaderValue>()
                .with_context(|| format!("{path}.{name} contains an invalid header value"))?;
            if reserved.contains(&name.as_str()) {
                bail!("{path} must not override protocol-owned header {name:?}");
            }
            if values.insert(name.clone(), value).is_some() {
                bail!("{path} contains duplicate header name {name:?}");
            }
        }
        Ok(Self { values })
    }
}

/// Connector-local HTTP transport settings shared by RPC and provider APIs.
#[derive(Clone, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct HttpTransportConfig {
    /// Complete HTTP(S) endpoint URL, including provider path and authentication query.
    pub url: String,
    /// Total request deadline from connection through response body completion.
    pub request_timeout_sec: u64,
    /// DNS, TCP, and TLS connection deadline.
    pub connect_timeout_sec: u64,
    /// Request headers applied to every request made by this transport.
    pub headers: TransportHeaders,
}

impl Default for HttpTransportConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            request_timeout_sec: default_http_request_timeout_sec(),
            connect_timeout_sec: default_http_connect_timeout_sec(),
            headers: TransportHeaders::default(),
        }
    }
}

impl fmt::Debug for HttpTransportConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpTransportConfig")
            .field("url", &Redacted)
            .field("request_timeout_sec", &self.request_timeout_sec)
            .field("connect_timeout_sec", &self.connect_timeout_sec)
            .field("headers", &self.headers)
            .finish()
    }
}

impl HttpTransportConfig {
    #[cfg(test)]
    /// Creates an HTTP transport with secure defaults and no custom headers.
    pub fn new(url: String) -> Self {
        Self {
            url,
            ..Self::default()
        }
    }

    /// Builds a reusable Rustls-backed client with bounded connection and request deadlines.
    pub(crate) fn build_client(&self) -> Result<reqwest::Client> {
        build_http_client(
            self.request_timeout_sec,
            self.connect_timeout_sec,
            &self.headers,
        )
    }

    fn normalized(self) -> Self {
        Self {
            url: self.url.trim().to_string(),
            ..self
        }
    }

    fn into_validated(self, path: &str) -> Result<Self> {
        validate_http_timeouts(self.request_timeout_sec, self.connect_timeout_sec, path)?;
        validate_http_url(&format!("{path}.url"), &self.url)?;
        let Self {
            url,
            request_timeout_sec,
            connect_timeout_sec,
            headers,
        } = self;
        let headers = headers.into_normalized(&format!("{path}.headers"), HTTP_RESERVED_HEADERS)?;
        Ok(Self {
            url,
            request_timeout_sec,
            connect_timeout_sec,
            headers,
        })
    }
}

/// Connector-local Solana JSON-RPC transport and protocol settings.
#[derive(Clone, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct RpcTransportConfig {
    /// Complete HTTP(S) JSON-RPC endpoint URL.
    pub url: String,
    /// Total request deadline from connection through response body completion.
    pub request_timeout_sec: u64,
    /// DNS, TCP, and TLS connection deadline.
    pub connect_timeout_sec: u64,
    /// Maximum accepted JSON-RPC response body size.
    pub response_size_limit_bytes: usize,
    /// Request headers applied to every JSON-RPC request.
    pub headers: TransportHeaders,
    /// Solana commitment level sent with compatible RPC requests.
    pub commitment: String,
}

impl Default for RpcTransportConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            request_timeout_sec: default_http_request_timeout_sec(),
            connect_timeout_sec: default_http_connect_timeout_sec(),
            response_size_limit_bytes: default_rpc_response_size_limit_bytes(),
            headers: TransportHeaders::default(),
            commitment: default_commitment(),
        }
    }
}

impl fmt::Debug for RpcTransportConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RpcTransportConfig")
            .field("url", &Redacted)
            .field("request_timeout_sec", &self.request_timeout_sec)
            .field("connect_timeout_sec", &self.connect_timeout_sec)
            .field("response_size_limit_bytes", &self.response_size_limit_bytes)
            .field("headers", &self.headers)
            .field("commitment", &self.commitment)
            .finish()
    }
}

impl RpcTransportConfig {
    #[cfg(test)]
    /// Creates an RPC transport with secure defaults and no custom headers.
    pub fn new(url: String) -> Self {
        Self {
            url,
            ..Self::default()
        }
    }

    /// Builds a reusable Rustls-backed JSON-RPC client.
    pub(crate) fn build_client(&self) -> Result<reqwest::Client> {
        build_http_client(
            self.request_timeout_sec,
            self.connect_timeout_sec,
            &self.headers,
        )
    }

    /// Returns a validated Solana commitment string.
    pub fn commitment_config(&self) -> Result<String> {
        match self.commitment.as_str() {
            "processed" | "confirmed" | "finalized" => Ok(self.commitment.clone()),
            other => bail!(
                "unsupported commitment {:?}, expected one of processed|confirmed|finalized",
                other
            ),
        }
    }

    fn normalized(self) -> Self {
        Self {
            url: self.url.trim().to_string(),
            commitment: self.commitment.trim().to_lowercase(),
            ..self
        }
    }

    fn into_validated(self, path: &str) -> Result<Self> {
        validate_http_timeouts(self.request_timeout_sec, self.connect_timeout_sec, path)?;
        if self.response_size_limit_bytes == 0
            || self.response_size_limit_bytes > MAX_RPC_RESPONSE_SIZE_LIMIT_BYTES
        {
            bail!(
                "{path}.response_size_limit_bytes must be between 1 and {MAX_RPC_RESPONSE_SIZE_LIMIT_BYTES}"
            );
        }
        validate_http_url(&format!("{path}.url"), &self.url)?;
        let Self {
            url,
            request_timeout_sec,
            connect_timeout_sec,
            response_size_limit_bytes,
            headers,
            commitment,
        } = self;
        let headers = headers.into_normalized(&format!("{path}.headers"), HTTP_RESERVED_HEADERS)?;
        let config = Self {
            url,
            request_timeout_sec,
            connect_timeout_sec,
            response_size_limit_bytes,
            headers,
            commitment,
        };
        config.commitment_config()?;
        Ok(config)
    }
}

fn build_http_client(
    request_timeout_sec: u64,
    connect_timeout_sec: u64,
    configured_headers: &TransportHeaders,
) -> Result<reqwest::Client> {
    validate_http_timeouts(request_timeout_sec, connect_timeout_sec, "HTTP transport")?;
    let mut headers = reqwest::header::HeaderMap::with_capacity(configured_headers.values.len());
    for (name, value) in configured_headers.iter() {
        let name = reqwest::header::HeaderName::from_bytes(name.trim().as_bytes())
            .context("invalid HTTP header name")?;
        if HTTP_RESERVED_HEADERS.contains(&name.as_str()) {
            bail!(
                "custom HTTP headers must not override protocol-owned header {:?}",
                name.as_str()
            );
        }
        let mut value = value
            .parse::<reqwest::header::HeaderValue>()
            .with_context(|| format!("invalid value for HTTP header {:?}", name.as_str()))?;
        value.set_sensitive(true);
        if headers.insert(name.clone(), value).is_some() {
            bail!("duplicate HTTP header name {:?}", name.as_str());
        }
    }
    reqwest::Client::builder()
        .default_headers(headers)
        .redirect(reqwest::redirect::Policy::none())
        .min_tls_version(reqwest::tls::Version::TLS_1_2)
        .connect_timeout(Duration::from_secs(connect_timeout_sec))
        .timeout(Duration::from_secs(request_timeout_sec))
        .build()
        .context("failed to build HTTP client")
}

fn validate_http_timeouts(
    request_timeout_sec: u64,
    connect_timeout_sec: u64,
    path: &str,
) -> Result<()> {
    if request_timeout_sec == 0 {
        bail!("{path}.request_timeout_sec must be greater than 0");
    }
    if connect_timeout_sec == 0 {
        bail!("{path}.connect_timeout_sec must be greater than 0");
    }
    if connect_timeout_sec > request_timeout_sec {
        bail!("{path}.connect_timeout_sec must not exceed request_timeout_sec");
    }
    Ok(())
}

/// Connector-local WebSocket handshake settings.
#[derive(Clone, Deserialize, Eq, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct WebsocketTransportConfig {
    /// Complete WS(S) endpoint URL, including provider path and authentication query.
    pub url: String,
    /// DNS, TCP, TLS, and WebSocket handshake deadline.
    pub handshake_timeout_sec: u64,
    /// Headers added to the generated WebSocket upgrade request.
    pub headers: TransportHeaders,
}

impl Default for WebsocketTransportConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            handshake_timeout_sec: default_ws_handshake_timeout_sec(),
            headers: TransportHeaders::default(),
        }
    }
}

impl fmt::Debug for WebsocketTransportConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WebsocketTransportConfig")
            .field("url", &Redacted)
            .field("handshake_timeout_sec", &self.handshake_timeout_sec)
            .field("headers", &self.headers)
            .finish()
    }
}

impl WebsocketTransportConfig {
    #[cfg(test)]
    /// Creates a WebSocket transport with secure defaults and no custom headers.
    pub fn new(url: String) -> Self {
        Self {
            url,
            ..Self::default()
        }
    }

    fn normalized(self) -> Self {
        Self {
            url: self.url.trim().to_string(),
            ..self
        }
    }

    fn into_validated(self, path: &str) -> Result<Self> {
        if self.handshake_timeout_sec == 0 {
            bail!("{path}.handshake_timeout_sec must be greater than 0");
        }
        validate_ws_url(&format!("{path}.url"), &self.url)?;
        let Self {
            url,
            handshake_timeout_sec,
            headers,
        } = self;
        let headers =
            headers.into_normalized(&format!("{path}.headers"), WEBSOCKET_RESERVED_HEADERS)?;
        Ok(Self {
            url,
            handshake_timeout_sec,
            headers,
        })
    }
}

const HTTP_RESERVED_HEADERS: &[&str] = &["content-length", "host", "transfer-encoding"];
pub(crate) const WEBSOCKET_RESERVED_HEADERS: &[&str] = &[
    "connection",
    "content-length",
    "host",
    "sec-websocket-key",
    "sec-websocket-version",
    "transfer-encoding",
    "upgrade",
];

/// One connector-local RPC, WebSocket, and/or provider-API declaration.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ConnectorConfig {
    /// Stable name referenced by command routes.
    pub name: String,
    /// Operator-facing provider name used in metrics and diagnostics.
    pub provider: String,
    /// Explicit non-standard provider capabilities.
    pub features: Vec<ProviderFeature>,
    #[serde(default)]
    /// Standard Solana RPC method profile, required when `rpc` is present.
    pub solana_rpc_compatibility: Option<SolanaRpcCompatibility>,
    #[serde(default)]
    /// Standard Solana JSON-RPC transport and protocol settings.
    pub rpc: Option<RpcTransportConfig>,
    #[serde(default)]
    /// Provider-specific HTTP API transport.
    pub provider_api: Option<HttpTransportConfig>,
    #[serde(default)]
    /// WebSocket transport used only for connectivity probes.
    pub websocket: Option<WebsocketTransportConfig>,
}

impl ConnectorConfig {
    /// Reports whether this connector has an HTTP RPC endpoint.
    pub fn has_rpc(&self) -> bool {
        self.rpc.is_some()
    }

    /// Reports whether this connector has a provider-specific HTTP API endpoint.
    pub fn has_provider_api(&self) -> bool {
        self.provider_api.is_some()
    }

    /// Reports whether this connector has a WebSocket endpoint.
    pub fn has_ws(&self) -> bool {
        self.websocket.is_some()
    }

    /// Reports whether this connector declares a provider feature.
    pub fn supports(&self, feature: ProviderFeature) -> bool {
        self.features.contains(&feature)
    }

    fn normalized(self) -> Self {
        let Self {
            name,
            provider,
            features,
            solana_rpc_compatibility,
            rpc,
            provider_api,
            websocket,
        } = self;
        Self {
            name: name.trim().to_string(),
            provider: provider.trim().to_string(),
            features,
            solana_rpc_compatibility,
            rpc: rpc.map(RpcTransportConfig::normalized),
            provider_api: provider_api.map(HttpTransportConfig::normalized),
            websocket: websocket.map(WebsocketTransportConfig::normalized),
        }
    }

    fn into_validated(self, index: usize) -> Result<Self> {
        let connector = self.normalized();
        connector.validate_declaration(index)?;
        let Self {
            name,
            provider,
            features,
            solana_rpc_compatibility,
            rpc,
            provider_api,
            websocket,
        } = connector;
        let rpc = rpc
            .map(|rpc| rpc.into_validated(&format!("connectors[{index}].rpc")))
            .transpose()?;
        let websocket = websocket
            .map(|websocket| websocket.into_validated(&format!("connectors[{index}].websocket")))
            .transpose()?;
        let provider_api = provider_api
            .map(|provider_api| {
                provider_api.into_validated(&format!("connectors[{index}].provider_api"))
            })
            .transpose()?;
        Ok(Self {
            name,
            provider,
            features,
            solana_rpc_compatibility,
            rpc,
            provider_api,
            websocket,
        })
    }

    fn validate_declaration(&self, index: usize) -> Result<()> {
        if self.name.is_empty() {
            bail!("connectors[{index}].name must not be empty");
        }
        if self.provider.is_empty() {
            bail!("connectors[{index}].provider must not be empty");
        }
        let mut seen_features = HashSet::with_capacity(self.features.len());
        for feature in &self.features {
            if !seen_features.insert(*feature) {
                bail!(
                    "connectors[{index}].features contains duplicate provider feature {:?}",
                    feature.as_str()
                );
            }
        }
        match (self.rpc.is_some(), self.solana_rpc_compatibility) {
            (true, None) => bail!(
                "connectors[{index}].solana_rpc_compatibility is required when rpc is configured"
            ),
            (false, Some(_)) => bail!(
                "connectors[{index}].solana_rpc_compatibility requires a configured rpc transport"
            ),
            _ => {}
        }
        if self.supports(ProviderFeature::HistoricalData)
            && self.solana_rpc_compatibility != Some(SolanaRpcCompatibility::Extended)
        {
            bail!(
                "connectors[{index}].features entry {:?} requires solana_rpc_compatibility = \"extended\", got {:?}",
                ProviderFeature::HistoricalData.as_str(),
                self.solana_rpc_compatibility
                    .map(SolanaRpcCompatibility::as_str)
            );
        }
        if self.supports(ProviderFeature::BlockFeeRewards)
            && self.solana_rpc_compatibility != Some(SolanaRpcCompatibility::Extended)
        {
            bail!(
                "connectors[{index}].features entry {:?} requires solana_rpc_compatibility = \"extended\", got {:?}",
                ProviderFeature::BlockFeeRewards.as_str(),
                self.solana_rpc_compatibility
                    .map(SolanaRpcCompatibility::as_str)
            );
        }
        if self.supports(ProviderFeature::BlockFeeRewards)
            && self
                .rpc
                .as_ref()
                .is_some_and(|rpc| rpc.commitment == "processed")
        {
            bail!(
                "connectors[{index}].features entry {:?} requires rpc.commitment = \"confirmed\" or \"finalized\"",
                ProviderFeature::BlockFeeRewards.as_str()
            );
        }
        if self.supports(ProviderFeature::JitoMevRewards) && !self.has_provider_api() {
            bail!(
                "connectors[{index}].features entry {:?} requires provider_api",
                ProviderFeature::JitoMevRewards.as_str()
            );
        }
        if self.rpc.is_none() && self.websocket.is_none() && self.provider_api.is_none() {
            bail!("connectors[{index}] must define rpc, websocket, and/or provider_api");
        }
        Ok(())
    }
}

/// Fully expanded, normalized, and validated watcher configuration.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Stable network label.
    pub network: String,
    /// Connector declarations in deterministic configuration order.
    pub connectors: Vec<ConnectorConfig>,
    /// Mode enablement and per-command fallback routes.
    pub modes: ModesConfig,
    /// IP address on which the metrics HTTP server listens.
    pub metrics_host: IpAddr,
    /// Port on which the metrics HTTP server listens.
    pub metrics_port: u16,
    /// Delay between completed polling rounds, in seconds.
    pub poll_interval_sec: u64,
    /// Optional cluster-specific delinquent vote distance.
    pub delinquent_slot_distance: Option<u64>,
    /// Optional file whose timestamp is refreshed after successful snapshots.
    pub liveness_file: Option<PathBuf>,
    /// Validators selected for detailed metrics and labels.
    pub watched_validators: Vec<WatchedValidatorConfig>,
    /// SHA-256 of the fully expanded profile consumed by this process.
    #[serde(skip)]
    pub(crate) fingerprint: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            network: default_network(),
            connectors: vec![],
            modes: ModesConfig::default(),
            metrics_host: default_metrics_host(),
            metrics_port: default_metrics_port(),
            poll_interval_sec: default_poll_interval_sec(),
            delinquent_slot_distance: None,
            liveness_file: None,
            fingerprint: String::new(),
            watched_validators: vec![],
        }
    }
}

fn default_network() -> String {
    "mainnet-beta".to_string()
}

fn config_fingerprint(expanded: &str) -> Result<String> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(expanded.as_bytes());
    let mut encoded = Vec::with_capacity(digest.len() * 2);
    for byte in digest {
        encoded.push(HEX[usize::from(byte >> 4)]);
        encoded.push(HEX[usize::from(byte & 0x0f)]);
    }
    String::from_utf8(encoded).context("SHA-256 fingerprint encoding was not UTF-8")
}

fn default_http_request_timeout_sec() -> u64 {
    30
}

fn default_http_connect_timeout_sec() -> u64 {
    10
}

const fn default_rpc_response_size_limit_bytes() -> usize {
    DEFAULT_RPC_RESPONSE_SIZE_LIMIT_BYTES
}

fn default_ws_handshake_timeout_sec() -> u64 {
    10
}

fn default_metrics_host() -> IpAddr {
    IpAddr::V4(Ipv4Addr::LOCALHOST)
}

fn default_metrics_port() -> u16 {
    8000
}

fn default_poll_interval_sec() -> u64 {
    15
}

fn default_commitment() -> String {
    "confirmed".to_string()
}

fn line_and_column(input: &str, offset: usize) -> (usize, usize) {
    let prefix = &input[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map_or(prefix, |(_, current_line)| current_line)
        .chars()
        .count()
        + 1;
    (line, column)
}

impl Config {
    /// Loads one TOML profile after environment expansion, normalization, and validation.
    pub fn load(path: &Path) -> Result<Self> {
        Self::load_with(path, expand_env_vars)
    }

    fn load_with<F>(path: &Path, expand: F) -> Result<Self>
    where
        F: FnOnce(&str) -> Result<String>,
    {
        if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
            bail!(
                "config file {} must use the .toml extension",
                path.display()
            );
        }

        let raw = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file {}", path.display()))?;
        let expanded = expand(&raw).with_context(|| {
            format!(
                "failed to expand environment variables in {}",
                path.display()
            )
        })?;
        let config = toml::from_str::<Self>(&expanded).map_err(|error| {
            let location = error.span().map_or_else(String::new, |span| {
                let (line, column) = line_and_column(&expanded, span.start);
                format!(" at line {line}, column {column}")
            });
            anyhow::anyhow!(
                "failed to parse TOML config {}{location}: {}",
                path.display(),
                error.message()
            )
        })?;
        Self {
            fingerprint: config_fingerprint(&expanded)?,
            ..config
        }
        .into_validated()
    }

    fn into_validated(self) -> Result<Self> {
        let Self {
            network,
            connectors,
            modes,
            metrics_host,
            metrics_port,
            poll_interval_sec,
            delinquent_slot_distance,
            liveness_file,
            watched_validators,
            fingerprint,
        } = self;
        let network = network.trim().to_string();
        if network.is_empty() {
            bail!("network must not be empty");
        }
        let connectors = validate_connectors(connectors)?;
        if metrics_port == 0 {
            bail!("metrics_port must be greater than 0");
        }
        if poll_interval_sec == 0 {
            bail!("poll_interval_sec must be greater than 0");
        }
        let watched_validators = watched_validators
            .into_iter()
            .map(|watched| normalize_watched_validator(watched, "watched_validators entry"))
            .collect::<Result<Vec<_>>>()?;
        let (modes, watched_validators) = validate_economics(modes, watched_validators)?;
        let config = Self {
            network,
            connectors,
            modes,
            metrics_host,
            metrics_port,
            poll_interval_sec,
            delinquent_slot_distance,
            liveness_file,
            watched_validators,
            fingerprint,
        };
        config.validate_routes()?;
        Ok(config)
    }

    fn validate_routes(&self) -> Result<()> {
        for command in CommandId::ALL {
            let route = self.modes.route(command);
            let path = format!(
                "modes.{}.commands.{}",
                command.mode().as_str(),
                command.as_str().replace('-', "_")
            );
            let required = match command {
                CommandId::RpcHealth | CommandId::EpochInfo | CommandId::VoteAccounts => true,
                CommandId::WsHealth
                | CommandId::NodeStatus
                | CommandId::ClusterNodes
                | CommandId::RecentPerformanceSamples
                | CommandId::CurrentBlockProduction
                | CommandId::CurrentLeaderSchedule
                | CommandId::NextLeaderSchedule
                | CommandId::JitoMevRewards
                | CommandId::RecentBlockFeeRewards => false,
                CommandId::AccountBalances
                | CommandId::InflationRewards
                | CommandId::RewardEpochBlockProduction => self.modes.economics.enabled,
            };

            if route.is_empty() {
                if required {
                    bail!("{path} must contain at least one connector name");
                }
                continue;
            }
            if command.mode() == ModeId::Economics && !self.modes.economics.enabled {
                bail!("{path} requires modes.economics.enabled = true");
            }

            let requirement = command.source_requirement();
            let mut seen = HashSet::with_capacity(route.len());
            for connector_name in route {
                if !seen.insert(connector_name) {
                    bail!("{path} contains duplicate connector name {connector_name:?}");
                }
                let connector = self
                    .connectors
                    .iter()
                    .find(|candidate| candidate.name == *connector_name)
                    .with_context(|| {
                        format!("{path} references unknown connector {connector_name:?}")
                    })?;
                match requirement.transport {
                    Transport::Rpc if !connector.has_rpc() => bail!(
                        "{path} references connector {:?}, which has no configured RPC endpoint",
                        connector.name
                    ),
                    Transport::Websocket if !connector.has_ws() => bail!(
                        "{path} references connector {:?}, which has no configured WebSocket endpoint",
                        connector.name
                    ),
                    Transport::ProviderApi if !connector.has_provider_api() => bail!(
                        "{path} references connector {:?}, which has no configured provider API endpoint",
                        connector.name
                    ),
                    _ => {}
                }
                if let Some(required_compatibility) = requirement.solana_rpc_compatibility {
                    let declared_compatibility =
                        connector.solana_rpc_compatibility.with_context(|| {
                            format!(
                                "{path} references connector {:?}, whose RPC endpoint has no declared solana_rpc_compatibility",
                                connector.name
                            )
                        })?;
                    if !declared_compatibility.supports(required_compatibility) {
                        bail!(
                            "{path} references connector {:?}, but command {:?} requires solana_rpc_compatibility = {:?} and the connector RPC endpoint declares {:?}",
                            connector.name,
                            command.as_str(),
                            required_compatibility.as_str(),
                            declared_compatibility.as_str()
                        );
                    }
                }
                if let Some(feature) = requirement.feature {
                    if !connector.supports(feature) {
                        bail!(
                            "{path} references connector {:?}, but command {:?} requires provider feature {:?}",
                            connector.name,
                            command.as_str(),
                            feature.as_str()
                        );
                    }
                }
            }
        }

        Ok(())
    }
}

fn validate_connectors(connectors: Vec<ConnectorConfig>) -> Result<Vec<ConnectorConfig>> {
    if connectors.is_empty() {
        bail!("at least one connector is required");
    }
    let connectors = connectors
        .into_iter()
        .enumerate()
        .map(|(index, connector)| connector.into_validated(index))
        .collect::<Result<Vec<_>>>()?;
    let mut seen_names = HashSet::with_capacity(connectors.len());
    for connector in &connectors {
        if !seen_names.insert(connector.name.as_str()) {
            bail!("duplicate connector name {:?}", connector.name);
        }
    }
    if !connectors.iter().any(ConnectorConfig::has_rpc) {
        bail!("at least one connector with an rpc transport is required");
    }
    Ok(connectors)
}

fn validate_economics(
    modes: ModesConfig,
    mut watched_validators: Vec<WatchedValidatorConfig>,
) -> Result<(ModesConfig, Vec<WatchedValidatorConfig>)> {
    if !modes.economics.enabled {
        if modes.economics.validator.is_some() {
            bail!("modes.economics.validator requires modes.economics.enabled = true");
        }
        return Ok((modes, watched_validators));
    }
    let ModesConfig {
        health,
        consensus,
        leadership,
        economics,
    } = modes;
    let EconomicsModeConfig {
        enabled,
        validator,
        commands,
    } = economics;
    let mut validator = normalize_watched_validator(
        validator.context("modes.economics.enabled requires modes.economics.validator")?,
        "modes.economics.validator",
    )?;
    validator.labels.push(ECONOMICS_TARGET_LABEL.to_string());
    validator.labels.sort();
    validator.labels.dedup();
    if let Some(existing) = watched_validators
        .iter_mut()
        .find(|watched| watched_validators_match(watched, &validator))
    {
        existing.labels.extend(validator.labels.iter().cloned());
        existing.labels.sort();
        existing.labels.dedup();
    } else {
        watched_validators.push(validator.clone());
    }
    Ok((
        ModesConfig {
            health,
            consensus,
            leadership,
            economics: EconomicsModeConfig {
                enabled,
                validator: Some(validator),
                commands,
            },
        },
        watched_validators,
    ))
}

fn expand_env_vars(input: &str) -> Result<String> {
    expand_env_vars_with(input, |key| match env::var(key) {
        Ok(value) => Ok(Some(value)),
        Err(env::VarError::NotPresent) => Ok(None),
        Err(env::VarError::NotUnicode(_)) => {
            bail!("environment variable {} is not valid UTF-8", key)
        }
    })
}

fn expand_env_vars_with<F>(input: &str, mut resolve: F) -> Result<String>
where
    F: FnMut(&str) -> Result<Option<String>>,
{
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '$' || chars.peek() != Some(&'{') {
            output.push(ch);
            continue;
        }

        let _ = chars.next();
        let mut expression = String::new();
        let mut closed = false;

        for next in chars.by_ref() {
            if next == '}' {
                closed = true;
                break;
            }
            expression.push(next);
        }

        if !closed {
            bail!(
                "unterminated environment variable expression starting with ${{{}}}",
                expression
            );
        }

        let (key, default) = match expression.split_once(":-") {
            Some((key, default)) => (key.trim(), Some(default)),
            None => (expression.trim(), None),
        };

        if key.is_empty() {
            bail!("environment variable expression must include a variable name");
        }

        let value = match resolve(key)? {
            Some(value) if !value.is_empty() => value,
            Some(_) | None => match default {
                Some(default) => default.to_string(),
                None => bail!("environment variable {} is required but not set", key),
            },
        };

        output.push_str(&value);
    }

    Ok(output)
}

fn normalize_pubkey(value: &str, field_name: &str) -> Result<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        bail!("{} must not be empty", field_name);
    }
    Ok(trimmed.to_string())
}

fn normalize_watched_validator(
    watched: WatchedValidatorConfig,
    field_name: &str,
) -> Result<WatchedValidatorConfig> {
    let WatchedValidatorConfig {
        identity,
        vote_account,
        mut labels,
    } = watched;
    if identity.is_none() && vote_account.is_none() {
        bail!("{field_name} must define identity and/or vote_account");
    }
    let identity = identity
        .map(|identity| normalize_pubkey(&identity, &format!("{field_name}.identity")))
        .transpose()?;
    let vote_account = vote_account
        .map(|vote_account| normalize_pubkey(&vote_account, &format!("{field_name}.vote_account")))
        .transpose()?;
    for label in &labels {
        validate_label(label)?;
    }
    labels.sort();
    labels.dedup();
    Ok(WatchedValidatorConfig {
        identity,
        vote_account,
        labels,
    })
}

fn watched_validators_match(left: &WatchedValidatorConfig, right: &WatchedValidatorConfig) -> bool {
    left.identity.is_some() && left.identity == right.identity
        || left.vote_account.is_some() && left.vote_account == right.vote_account
}

fn validate_label(label: &str) -> Result<()> {
    let trimmed = label.trim();
    let Some((category, value)) = trimmed.split_once(':') else {
        bail!("label {:?} must follow the category:value format", trimmed);
    };

    if category.is_empty() || value.is_empty() {
        bail!("label {:?} must follow the category:value format", trimmed);
    }

    Ok(())
}

/// Returns whether the endpoint text declares a non-empty authority after its scheme.
pub(crate) fn has_explicit_authority(url: &str) -> bool {
    url.split_once("://").is_some_and(|(_, remainder)| {
        remainder
            .split(['/', '?', '#'])
            .next()
            .is_some_and(|authority| !authority.is_empty())
    })
}

fn validate_http_url(field: &str, url: &str) -> Result<()> {
    if url.is_empty() {
        bail!("{field} must not be empty");
    }
    let parsed =
        reqwest::Url::parse(url).with_context(|| format!("{field} must be a valid URL"))?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().filter(|host| !host.is_empty()).is_none()
        || !has_explicit_authority(url)
    {
        bail!("{field} must use http:// or https:// with a host");
    }
    Ok(())
}

fn validate_ws_url(field: &str, url: &str) -> Result<()> {
    if url.is_empty() {
        bail!("{field} must not be empty");
    }
    let parsed =
        reqwest::Url::parse(url).with_context(|| format!("{field} must be a valid URL"))?;
    if !matches!(parsed.scheme(), "ws" | "wss")
        || parsed.host_str().filter(|host| !host.is_empty()).is_none()
        || !has_explicit_authority(url)
    {
        bail!("{field} must use ws:// or wss:// with a host");
    }
    Ok(())
}

#[cfg(test)]
mod tests;
