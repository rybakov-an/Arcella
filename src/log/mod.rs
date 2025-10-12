// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.


//! Logging and tracing for the Arcella Runtime.
//!
//! This module implements a flexible, multi-channel logging system based on the [`tracing`] crate.
//! It supports three output channels:
//! - **File** (`arcella.log`) — with optional structured (JSON) or plain-text formatting;
//! - **stderr** — for convenient debugging when running in foreground mode;
//! - **In-memory ring buffer** — to expose recent logs via ALME (e.g., through the CLI).
//!
//! Configuration is read from `tracing.cfg` in Arcella’s config directory and allows:
//! - Setting a global log level;
//! - Configuring per-module or per-target log levels;
//! - Enabling/disabling individual output channels;
//! - Limiting the in-memory buffer size for ALME.
//!
//! The system is thread-safe and uses non-blocking I/O for file writes.

use std::collections::{VecDeque, HashMap};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use time::OffsetDateTime;

use serde::{Deserialize, Deserializer};
use tracing_subscriber::{
    filter::{EnvFilter, LevelFilter},
    fmt,
    layer::SubscriberExt,
    registry::LookupSpan,
    util::SubscriberInitExt,
    Layer,
};

use crate::error::{ArcellaError, Result as ArcellaResult};
use crate::config::ArcellaConfig;

// Global resources for logger

/// In-memory ring buffer storing the most recent log entries.
/// Used to serve logs via ALME (e.g., for CLI queries).
// TODO: use a lock-free buffer for high-throughput scenarios
static LOG_BUFFER: std::sync::OnceLock<Arc<Mutex<VecDeque<String>>>> = std::sync::OnceLock::new();

fn get_log_buffer() -> Option<&'static Arc<Mutex<VecDeque<String>>>> {
    LOG_BUFFER.get()
}

/// Initializes the global `tracing` subscriber based on the provided configuration.
///
/// This function must be called exactly once during daemon startup. It:
/// - Creates the log directory if it doesn’t exist;
/// - Loads or falls back to default settings from `tracing.cfg`;
/// - Configures logging layers: file, stderr, and in-memory buffer for ALME;
/// - Installs a global subscriber for `tracing`.
///
/// # Arguments
///
/// * `config` — reference to the main Arcella configuration, which includes paths to logs and config files.
///
/// # Returns
///
/// A `WorkerGuard` from `tracing_appender`, which must be kept alive until shutdown
/// to ensure buffered log entries are flushed to disk. Returns `None` if file logging is disabled.
											  
///
/// # Errors
///
/// Returns an error if:
/// - The log directory cannot be created;
/// - `tracing.cfg` is malformed or contains invalid values;
/// - The global subscriber has already been initialized;
/// - The ALME in-memory buffer fails to initialize (when enabled).
pub fn init(config: &ArcellaConfig) -> ArcellaResult<Option<tracing_appender::non_blocking::WorkerGuard>> {

    let mut file_guard: Option<tracing_appender::non_blocking::WorkerGuard> = None;

    let tracing_cfg_path = config.config_dir.join("tracing.cfg");

    let tracing_cfg = load_tracing_config(&tracing_cfg_path)?;

    // Ensure log directory exists
    fs::create_dir_all(&config.log_dir)
        .map_err(|e| ArcellaError::Io(e))?;

    let log_file_path = config.log_dir.join("arcella.log");

    // Initialize ALME in-memory buffer
    if tracing_cfg.alme_buffer_size > 0 {
        let buffer = Arc::new(Mutex::new(VecDeque::with_capacity(tracing_cfg.alme_buffer_size)));
        LOG_BUFFER.set(buffer).map_err(|_| ArcellaError::Internal("LOG_BUFFER already set".into()))?;
    }

    // Build filter directives
    let mut directives = vec![format!("arcella={}", tracing_cfg.default_level)];

    // Override per-module levels
    for (target, level) in &tracing_cfg.modules {
        directives.push(format!("{}={}", target, level));
    }

    let filter = directives.join(",");

    let env_filter = EnvFilter::try_new(filter)
        .map_err(|e| ArcellaError::Config(format!("invalid log filter: {}", e)))?;

    let mut layers = Vec::new();

    // 1. File layer
    if tracing_cfg.file {

        // Use `never` rolling (single file: arcella.log)
        let file_appender = tracing_appender::rolling::never(&config.log_dir, "arcella.log");
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        let file_layer = if tracing_cfg.structured {
            fmt::layer()
                .json()
                .with_writer(non_blocking)
                .with_ansi(false)
                .boxed()
        } else {
            fmt::layer()
                .with_writer(non_blocking)
                .with_ansi(false)
                .boxed()
        };
        layers.push(file_layer);
        file_guard = Some(guard);
    }

    // 2. StdErr layer
    if tracing_cfg.stderr {
        let console_layer = fmt::layer()
            .with_writer(std::io::stderr)
            .with_ansi(true)
            .boxed();
        layers.push(console_layer);
    }

    // 3. In-memory ALME layer
    if tracing_cfg.alme_buffer_size > 0 {
        let alme_layer = AlmeBufferLayer::new(tracing_cfg.alme_buffer_size);
        layers.push(Box::new(alme_layer));
    }    

    let subscriber = tracing_subscriber::registry()
        .with(layers)
        .with(env_filter);

    subscriber
        .try_init()
        .map_err(|e| ArcellaError::Internal(format!("failed to init tracing: {}", e)))?;

    Ok(file_guard)
}

/// Loads tracing configuration from a TOML file.
///
/// If the file does not exist, default settings are returned.
fn load_tracing_config(path: &PathBuf) -> ArcellaResult<TracingConfig> {
    if !path.exists() {
        tracing::debug!("tracing.cfg not found, using defaults");
        return Ok(TracingConfig::default());
    }

    let contents = fs::read_to_string(path)
        .map_err(|e| ArcellaError::IoWithPath(e, path.clone()))?;
    toml::from_str(&contents)
        .map_err(|e| ArcellaError::Config(format!("tracing.cfg: {}", e)))
}


/// Helper: deserialize LevelFilter from string (e.g., "info", "debug")
fn deserialize_level_filter<'de, D>(deserializer: D) -> Result<LevelFilter, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse::<LevelFilter>().map_err(serde::de::Error::custom)
}

/// Helper: deserialize a map of per-module log levels.
fn deserialize_module_levels<'de, D>(deserializer: D) -> Result<HashMap<String, LevelFilter>, D::Error>
where
    D: Deserializer<'de>,
{
    let map: HashMap<String, String> = Deserialize::deserialize(deserializer)?;
    let mut result = HashMap::new();
    for (target, level_str) in map {
        let level = level_str
            .parse::<LevelFilter>()
            .map_err(serde::de::Error::custom)?;
        result.insert(target, level);
    }
    Ok(result)
}

/// Arcella tracing configuration.
#[derive(Deserialize, Debug, Clone)]
pub struct TracingConfig {
    /// Default log level for targets under the `arcella` namespace.
    ///
    /// Valid values: `"trace"`, `"debug"`, `"info"`, `"warn"`, `"error"`.
    #[serde(default = "default_log_level", deserialize_with = "deserialize_level_filter")]
    pub default_level: LevelFilter,

    /// Use structured (JSON) format for file logging.
    ///
    /// If `true`, entries in `arcella.log` will be JSON-encoded, suitable for machine processing
    /// (e.g., ingestion into ELK or Loki). If `false`, human-readable text is used.
    #[serde(default = "default_structured")]
    pub structured: bool,

    /// Output logs to stderr.
    ///
    /// Useful when running Arcella in foreground mode or inside a container where stderr is
    /// captured by an orchestrator (e.g., systemd or Kubernetes).
    #[serde(default = "default_stderr")]
    pub stderr: bool,

    /// Write logs to the `arcella.log` file.
    ///
    /// The file is created in the directory specified by `ArcellaConfig::log_dir`.
    #[serde(default = "default_file")]
    pub file: bool,

    /// Maximum size of the in-memory ring buffer for ALME (in log entries).
    ///
    /// A value of `0` disables the buffer. Used by the `alme logs` command to retrieve
    /// the most recent `N` lines without reading the log file.
    #[serde(default = "default_alme_buffer_size")]
    pub alme_buffer_size: usize,

    /// Per-module/target log levels.
    ///
    /// Keys are target names (e.g., `"arcella::runtime"`), values are log levels.
    /// These override `default_level` for the specified targets.
    ///
    /// Example:
    /// ```toml
    /// [modules]
    /// "arcella::runtime" = "info"
    /// "arcella::alme" = "debug"
    /// ```
    #[serde(default, deserialize_with = "deserialize_module_levels")]
    pub modules: HashMap<String, LevelFilter>,
}

fn default_log_level() -> LevelFilter { LevelFilter::INFO }
fn default_structured() -> bool { false }
fn default_stderr() -> bool { true }
fn default_file() -> bool { true }
fn default_alme_buffer_size() -> usize { 100 }

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            default_level: default_log_level(),
            structured: default_structured(),
            stderr: default_stderr(),
            file: default_file(),
            alme_buffer_size: default_alme_buffer_size(),
            modules: HashMap::new(),
        }
    }
}

// === Layer for ALME ===

/// A tracing layer that writes events to an in-memory ring buffer for later retrieval via ALME.
struct AlmeBufferLayer {
    max_size: usize,
}

impl AlmeBufferLayer {
    fn new(max_size: usize) -> Self {
        Self { max_size }
    }
}

impl<S> Layer<S> for AlmeBufferLayer
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if let Some(buffer) = get_log_buffer() {
            let meta = event.metadata();

            let now: OffsetDateTime = OffsetDateTime::now_utc();
            let now_rfc3339 = now.format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| "<invalid-timestamp>".to_string());

            let mut visitor = EventVisitor::default();
            event.record(&mut visitor);

            let message = if visitor.message.is_empty() {
                "no message".to_string()
            } else {
                visitor.message
            };

            let fields = if visitor.fields.is_empty() {
                String::new()
            } else {
                format!(" {{{}}}", visitor.fields.join(", "))
            };

            let line = format!(
                "{} {} {}: {}{}",
                now_rfc3339,
                meta.level(),
                meta.target(),
                message,
                fields
            ); 

            let mut buf = buffer.lock().unwrap();
            match buffer.lock() {
                Ok(mut buf) => {
                    if buf.len() >= self.max_size {
                        buf.pop_front();
                    }
                    buf.push_back(line);
                }
                Err(e) => {
                    // Avoid panicking in a tracing handler; silently ignore if poisoned
                    eprintln!("ALME log buffer poisoned: {}", e);
                }
            }

        }
    }
}

/// Returns up to `n` most recent log entries from the in-memory buffer.
///
/// The buffer is only populated if `alme_buffer_size > 0` in `tracing.cfg`.
/// Entries are returned in reverse chronological order (most recent first).
///
/// # Arguments
///
/// * `n` — maximum number of log lines to return.
///
/// # Returns
///
/// A vector of log strings. Returns an empty vector if the buffer is uninitialized or disabled.
pub fn get_recent_logs(n: usize) -> Vec<String> {
    if let Some(buffer) = get_log_buffer() {
        match buffer.lock() {
            Ok(buf) => {
                buf.iter().rev().take(n).cloned().collect()
            }
            Err(e) => {
                eprintln!("Failed to lock ALME log buffer: {}", e);
                vec![]
            }
        }
    } else {
        vec![]
    }
}

#[derive(Default)]
struct EventVisitor {
    message: String,
    fields: Vec<String>,
}

impl tracing::field::Visit for EventVisitor {
    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields.push(format!("{}={}", field.name(), value));
        }
    }

    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        } else {
            self.fields.push(format!("{}={:?}", field.name(), value));
        }
    }
}