use std::path::PathBuf;
use wasmtime::{Engine, Module, Store};
use wat::parse_str;
use anyhow::Result;

// Подключаем ваш основной код (если нужен)
//mod main;

/*#[test]
fn test_wasm_files() -> Result<()> {
    let engine = Engine::default();
    let test_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("resources")
        .join("wasm");

    for entry in std::fs::read_dir(test_dir)? {
        let path = entry?.path();
        if path.extension().map_or(false, |ext| ext == "wasm") {
            let module = Module::from_file(&engine, &path)?;
            let mut store = Store::new(&engine, ());
            let _instance = wasmtime::Instance::new(&mut store, &module, &[])?;
            println!("✅ Тест успешно выполнен для: {}", path.display());
        }
    }

    Ok(())
}

#[test]
fn test_wat_files() -> Result<()> {
    let engine = Engine::default();
    let test_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("resources")
        .join("wat");

    for entry in std::fs::read_dir(test_dir)? {
        let path = entry?.path();
        if path.extension().map_or(false, |ext| ext == "wat") {
            let wat_content = std::fs::read_to_string(&path)?;
            let wasm_bytes = parse_str(&wat_content)?;
            let module = Module::new(&engine, &wasm_bytes)?;
            let mut store = Store::new(&engine, ());
            let _instance = wasmtime::Instance::new(&mut store, &module, &[])?;
            println!("✅ Тест успешно выполнен для: {}", path.display());
        }
    }

    Ok(())
}*/