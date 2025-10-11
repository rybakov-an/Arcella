// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

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

/// Buffer for ALME log access (in-memory ring buffer)
// TODO: use lock-free buffer for high-throughput scenarios 
static LOG_BUFFER: std::sync::OnceLock<Arc<Mutex<VecDeque<String>>>> = std::sync::OnceLock::new();

fn get_log_buffer() -> Option<&'static Arc<Mutex<VecDeque<String>>>> {
    LOG_BUFFER.get()
}

/// Loads and initializes the global tracing subscriber.
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

    // 3. In memory ALME layer
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

fn load_tracing_config(path: &PathBuf) -> ArcellaResult<TracingConfig> {
    if !path.exists() {
        //create_default_tracing_config(path)?;
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

#[derive(Deserialize, Debug, Clone)]
pub struct TracingConfig {
    #[serde(default = "default_log_level", deserialize_with = "deserialize_level_filter")]
    pub default_level: LevelFilter,

    #[serde(default = "default_structured")]
    pub structured: bool,

    #[serde(default = "default_stderr")]
    pub stderr: bool,

    #[serde(default = "default_file")]
    pub file: bool,

    #[serde(default = "default_alme_buffer_size")]
    pub alme_buffer_size: usize,

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
            if buf.len() >= self.max_size {
                buf.pop_front();
            }
            buf.push_back(line);
        }
    }
}

pub fn get_recent_logs(n: usize) -> Vec<String> {
    if let Some(buffer) = get_log_buffer() {
        let buf = buffer.lock().unwrap();
        buf.iter().rev().take(n).cloned().collect()
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