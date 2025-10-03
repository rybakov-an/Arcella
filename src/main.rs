use std::{
    path::PathBuf,
};
use anyhow::{Context, Result};
use clap::Parser;

/// Arcella: Modular WebAssembly Runtime
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Path to the WebAssembly module (.wasm file)
    #[arg(value_name = "MODULE", value_parser)]
    module: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Читаем WASM-файл
    let wasm_bytes = std::fs::read(&cli.module)
        .with_context(|| format!("failed to read module: {}", cli.module.display()))?;

    // Создаём WASI-контекст
    let wasi_ctx = wasmtime::wasi::WasiCtxBuilder::new()
        .inherit_stdio()   // stdout/stderr → терминал
        .inherit_args()?   // передаём аргументы (пусто, но безопасно)
        .build();

    // Движок и Store с WASI-контекстом
    let engine = wasmtime::Engine::default();
    let mut store = wasmtime::Store::new(&engine, wasi_ctx);

    // Компилируем модуль
    let module = wasmtime::Module::from_binary(&engine, &wasm_bytes)
        .context("failed to compile WebAssembly module")?;

    // Линкер + WASI
    let mut linker = wasmtime::Linker::new(&engine);
    wasmtime::wasi::add_to_linker(&mut linker, |ctx: &mut wasmtime::wasi::WasiCtx| ctx)
        .context("failed to add WASI to linker")?;

    // Инстанцируем
    let instance = linker
        .instantiate(&mut store, &module)
        .context("failed to instantiate module")?;

    // Вызываем _start — точка входа для WASI-приложений
    let start = instance
        .get_typed_func::<(), ()>(&mut store, "_start")
        .map_err(|_| anyhow::anyhow!("module does not export _start function"))?;

    println!("Running module: {}", cli.module.display());
    start.call(&mut store, ())?;
    println!("Module finished successfully");

    Ok(())
}