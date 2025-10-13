// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Arcella CLI — управление runtime'ом через ALME
#[derive(Parser)]
#[command(version, about = "Arcella CLI — управление через ALME", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Проверить доступность ALME
    Ping,
    /// Получить статус runtime'а
    Status,
    /// Вывести последние N строк лога
    #[command(name = "log:tail")]
    LogTail {
        /// Количество строк (по умолчанию: 100)
        #[arg(short, long, default_value_t = 100)]
        n: usize,
    },
    /// Список установленных модулей
    #[command(name = "module:list")]
    ModuleList,
    /// Интерактивная консоль
    Shell,
}

// --- ALME Protocol (скопировано из arcella/alme/protocol.rs) ---
#[derive(Serialize, Deserialize, Debug)]
struct AlmeRequest {
    cmd: String,
    #[serde(default)]
    args: serde_json::Value,
}

#[derive(Serialize, Deserialize, Debug)]
struct AlmeResponse {
    success: bool,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<serde_json::Value>,
}
// --- Конец протокола ---

async fn send_alme_request(
    socket_path: &PathBuf,
    request: AlmeRequest,
) -> anyhow::Result<AlmeResponse> {
    let stream = UnixStream::connect(socket_path).await?;
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);

    let request_json = serde_json::to_vec(&request)?;
    writer.write_all(&request_json).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await?;

    let mut response_line = String::new();
    reader.read_line(&mut response_line).await?;

    if response_line.is_empty() {
        anyhow::bail!("ALME server closed connection unexpectedly");
    }

    let response: AlmeResponse = serde_json::from_str(&response_line)?;
    Ok(response)
}

fn get_default_socket_path() -> PathBuf {
    let base = dirs::home_dir().unwrap().join(".arcella");
    base.join("alme")
}

async fn handle_command(cmd: Commands) -> anyhow::Result<()> {
    let socket_path = get_default_socket_path();

    match cmd {
        Commands::Ping => {
            let req = AlmeRequest {
                cmd: "ping".to_string(),
                args: serde_json::Value::Null,
            };
            let resp = send_alme_request(&socket_path, req).await?;
            if resp.success {
                println!("pong");
            } else {
                eprintln!("Error: {}", resp.message);
                std::process::exit(1);
            }
        }
        _ => {}
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse(); 
    handle_command(cli.command).await
}
