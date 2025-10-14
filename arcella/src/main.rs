// arcella/arcella/src/main.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

use clap::Parser;
use std::sync::Arc;
use tokio::sync::RwLock;
//use wasmtime::*;
//use wasmtime_wasi::{p1, WasiCtxBuilder};
use wat;

mod alme;
mod runtime;
mod config;
mod storage;
mod cache;
mod manifest;
mod error;
mod log;

use error::{ArcellaError, Result as ArcellaResult};

/// Arcella: Modular WebAssembly Runtime
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {}

#[tokio::main]
async fn main() -> ArcellaResult<()> {

    // 1. Load configuration (e.g., paths, runtime options)
    let _ = Cli::parse(); 
    let config = Arc::new(config::load().await?);

    // 2. Initialize logging (should be the first side effect)
    let _log_guard = log::init(&config)?;
    tracing::info!("Starting up (v{})", env!("CARGO_PKG_VERSION"));

    // 3. Initialize core subsystems: storage and module cache
    let storage = Arc::new(storage::StorageManager::new(&config).await?);
    tracing::debug!("Initialize storage");
    let cache = Arc::new(cache::ModuleCache::new(&config).await?);
    tracing::debug!("Initialize cache");

    let runtime = Arc::new(RwLock::new(
        runtime::ArcellaRuntime::new(config.clone(), storage.clone(), cache.clone()).await?,
    ));
    tracing::debug!("Initialize core runtime");


    let alme_handle = alme::start(runtime.clone()).await?;
    tracing::info!("Starting ALME server");

    tokio::signal::ctrl_c().await?;
    tracing::info!("Received Ctrl+C, shutting down...");

    runtime.write().await.shutdown().await?;
    alme_handle.shutdown().await?;

    tracing::info!("Shutting down");
        
    // Configure the engine
    /*let mut config = Config::default();
    config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
    config.wasm_multi_memory(false);
    config.wasm_threads(false);
    config.consume_fuel(true);

    // Initialize the engine
    let engine = Engine::new(&config)?;
    let mut linker: Linker<p1::WasiP1Ctx> = Linker::new(&engine);

    p1::add_to_linker_sync(&mut linker, |t| t)?;
    let wasi_ctx = WasiCtxBuilder::new()
        .inherit_stderr()
        .inherit_stdout()
        .build_p1();

    let mut store = Store::new(&engine, wasi_ctx);
    let _ = store.set_fuel(1_000_000);

    // Load the module

    let module_bytes = load_module_bytes(&cli.module)?;
    let module = Module::from_binary(&engine, &module_bytes)
        .map_err(|e| anyhow!("Failed to compile module: {}", e))?;

    linker.module(&mut store, "default", &module)?;

    match linker.get_default(&mut store, "default") {
        Ok(func) => {
            if let Err(e) = func.typed::<(), ()>(&store)?.call(&mut store, ()) {
                if e.is::<Trap>() {
                    eprintln!("WASM module exited with trap: {}", e);
                } else {
                    return Err(e.into());
                }
            }
        }
        Err(_) => {
            eprintln!("No default function found — nothing to run.");
        }
    }*/

    drop(_log_guard);

    Ok(())
    
}

/*fn load_module_bytes(path: &PathBuf) -> ArcellaResult<Vec<u8>> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .ok_or_else(|| ArcellaError::InvalidModulePath(path.clone()))?;

    match extension {
        "wat" => {
            let wat_content = std::fs::read_to_string(path)
                .with_context(|| format!("Failed to read .wat file: '{}'", path.display()))?;
            wat::parse_str(&wat_content)
                .with_context(|| format!("Failed to parse .wat file: '{}'", path.display()))
        }
        "wasm" => {
            std::fs::read(path)
                .with_context(|| format!("Failed to read .wasm file: '{}'", path.display()))
        }
        _ => Err(anyhow::anyhow!(
            "Unsupported file type: '{}'. Only .wat and .wasm are supported.",
            path.display()
        )),
    }
}*/