use clap::Parser;
//use wasmtime::*;
//use wasmtime_wasi::{p1, WasiCtxBuilder};
use wat;

mod cli;
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
    
    let _ = Cli::parse(); 

    let _ = alme::start().await?;

    tokio::signal::ctrl_c().await?;

    println!("\nArcella shutting down...");

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