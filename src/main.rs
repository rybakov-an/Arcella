use std::{
    path::PathBuf,
    fs::File,
};
use anyhow::{anyhow,Result};
use clap::Parser;
use wasmtime;
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

    // Проверка существования файла
    if !cli.module.exists() {
        return Err(anyhow!("File {} does not exist", cli.module.display()));
    }

    // Можно добавить кастомную конфигурацию
    let mut config = wasmtime::Config::default();
    config.wasm_backtrace_details(wasmtime::WasmBacktraceDetails::Enable);
    let engine = wasmtime::Engine::new(&config)?;

    // Определяем, как получить байты модуля
    let module_bytes: Vec<u8> = if cli.module
        .extension()
        .map_or(false, |ext| ext.eq_ignore_ascii_case("wat"))
    {
        // Это .wat файл - парсим его
        let wat_content = std::fs::read_to_string(&cli.module)
            .map_err(|e| anyhow!("Failed to read .wat file: {}", e))?;

        wat::parse_str(&wat_content)
            .map_err(|e| anyhow!("Failed to parse .wat: {}", e))?
    } else {
        // Обычный .wasm файл
        std::fs::read(&cli.module)
            .map_err(|e| anyhow!("Failed to read module file: {}", e))?
    };


    let module = wasmtime::Module::new(&engine, &module_bytes)
        .map_err(|e| anyhow!("Failed to create module: {}", e))?;

    let mut store = wasmtime::Store::new(
        &engine,
        EngineState {
            name: "hello, world!".to_string(),
            count: 0,
        },
    );

    let imports = [];
    
    let instance = wasmtime::Instance::new(&mut store, &module, &imports)?;
    
    {
        let exports = instance.exports(&mut store);
        //println!("Exported functions: {:?}", exports);
    }


    let add = instance.get_typed_func::<(i32, i32), (i32)>(&mut store, "add")
        .map_err(|e| anyhow!("Failed to get 'add' function: {}", e))?;
    match add.call(&mut store, (2, 3)) {
        Ok(result ) => println!("Result of add(2, 3): {}", result),
        Err(e) => {
            eprintln!("Error calling add: {}", e);
        }
    }
    
    let state = store.data_mut();
    state.count += 1;
    println!("Engine state count: {}", state.count);

    Ok(())
}