use std::path::PathBuf;
use anyhow::{anyhow, Context, Result};
use clap::Parser;
use wasmtime::*;
use wasmtime_wasi::WasiCtxBuilder;
use wasmtime_wasi::p1;
use wat;

/// Arcella: Modular WebAssembly Runtime
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    /// Path to the WebAssembly module (.wasm file)
    #[arg(value_name = "MODULE", value_parser)]
    module: PathBuf,
}

struct EngineState {
    name: String,
    count: usize,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Можно добавить кастомную конфигурацию
    let mut config = Config::default();
    config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
    config.wasm_multi_memory(false);
    config.wasm_threads(false);
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;

    let module_bytes = load_module_bytes(&cli.module)?;

    let module = Module::from_binary(&engine, &module_bytes)
        .map_err(|e| anyhow!("Failed to compile module: {}", e))?;

    let mut wasi_ctx = WasiCtxBuilder::new()
        .inherit_stderr()
        .inherit_stdout()
        .build_p1();

    let mut store = Store::new(&engine, wasi_ctx);
    let _ = store.set_fuel(1_000_000);

    let mut linker: Linker<p1::WasiP1Ctx> = Linker::new(&engine);
    p1::add_to_linker_sync(&mut linker, |t| t)?;

    let instance = linker.instantiate(&mut store, &module)?;
    linker.instance(&mut store, "add_func", instance)?;

    let add = instance.get_typed_func::<(i32, i32), (i32)>(&mut store, "add")
        .map_err(|e| anyhow!("Failed to get 'add' function: {}", e))?;
    match add.call(&mut store, (2, 3)) {
        Ok(result ) => println!("Result of add(2, 3): {}", result),
        Err(e) => {
            eprintln!("Error calling add: {}", e);
        }
    }
    
    Ok(())
}

fn load_module_bytes(path: &PathBuf) -> Result<Vec<u8>> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .ok_or_else(|| anyhow::anyhow!("Unsupported file type: {}. Only .wat and .wasm are supported.", path.display()))?;

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
}