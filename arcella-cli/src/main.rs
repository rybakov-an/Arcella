// arcella/arcella-cli/src/main.rs
//
// Copyright (c) 2025 Arcella Team
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE>
// or the MIT license <LICENSE-MIT>, at your option.
// This file may not be copied, modified, or distributed
// except according to those terms.

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

use arcella_types::alme::proto::{AlmeRequest, AlmeResponse};

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
        },
        Commands::Status => {
            let req = AlmeRequest {
                cmd: "status".to_string(),
                args: serde_json::Value::Null,
            };
            let resp = send_alme_request(&socket_path, req).await?;
            if resp.success {
                println!("Status: {}", resp.message);
                if let Some(data) = resp.data {
                    println!("Data: {:#}", data);
                }
            } else {
                eprintln!("Error: {}", resp.message);
                std::process::exit(1);
            }
        },
        Commands::LogTail { n } => {
            let args = serde_json::json!({ "n": n });
            let req = AlmeRequest {
                cmd: "log:tail".to_string(),
                args,
            };
            let resp = send_alme_request(&socket_path, req).await?;
            if resp.success {
                if let Some(data) = resp.data {
                    if let Some(lines) = data.get("lines").and_then(|v| v.as_array()) {
                        for line in lines {
                            if let Some(s) = line.as_str() {
                                println!("{}", s);
                            }
                        }
                    }
                }
            } else {
                eprintln!("Error: {}", resp.message);
                std::process::exit(1);
            }
        },
        Commands::ModuleList => {
            let req = AlmeRequest {
                cmd: "module:list".to_string(),
                args: serde_json::Value::Null,
            };
            let resp = send_alme_request(&socket_path, req).await?;
            if resp.success {
                if let Some(data) = resp.data {
                    println!("{:#}", data);
                }
            } else {
                eprintln!("Error: {}", resp.message);
                std::process::exit(1);
            }
        },
        Commands::Shell => {
            eprintln!("Interactive shell not implemented yet (use single commands)");
            std::process::exit(1);
        },
        //_ => {}
    }
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse(); 
    handle_command(cli.command).await
}
