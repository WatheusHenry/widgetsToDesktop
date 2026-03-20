use tauri::{LogicalPosition, Manager, Emitter};
use std::process::Command;
use std::collections::HashMap;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::ClientOptions;

const CLIENT_ID:     &str = "1484342268960968907";
const CLIENT_SECRET: &str = "Ktra5iMCna07u1VDq_8aCp6rJEs2s0fg"; // substitua pelo seu secret
const CHANNEL_ID:    &str = "1483243850846834751";

// ── Opcodes do protocolo IPC do Discord ───────────────────────────────────────
const OP_HANDSHAKE: u32 = 0;
const OP_FRAME:     u32 = 1;

// ── Escreve um frame IPC: [op: u32 LE][len: u32 LE][json] ────────────────────
async fn write_frame<W: AsyncWriteExt + Unpin>(w: &mut W, op: u32, json: &str) -> std::io::Result<()> {
    let bytes = json.as_bytes();
    let mut buf = Vec::with_capacity(8 + bytes.len());
    buf.extend_from_slice(&op.to_le_bytes());
    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(bytes);
    w.write_all(&buf).await
}

// ── Lê um frame IPC ──────────────────────────────────────────────────────────
async fn read_frame<R: AsyncReadExt + Unpin>(r: &mut R) -> std::io::Result<(u32, String)> {
    let mut header = [0u8; 8];
    r.read_exact(&mut header).await?;
    let op  = u32::from_le_bytes(header[0..4].try_into().unwrap());
    let len = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    Ok((op, String::from_utf8_lossy(&body).to_string()))
}

// ── Troca code por access_token via HTTP ─────────────────────────────────────
async fn exchange_token(code: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let mut params = HashMap::new();
    params.insert("client_id",     CLIENT_ID);
    params.insert("client_secret", CLIENT_SECRET);
    params.insert("grant_type",    "authorization_code");
    params.insert("code",          code);

    let res = client
        .post("https://discord.com/api/oauth2/token")
        .form(&params)
        .send().await
        .map_err(|e| format!("Requisição falhou: {e}"))?;

    if !res.status().is_success() {
        return Err(format!("Discord erro: {}", res.text().await.unwrap_or_default()));
    }

    let json: serde_json::Value = res.json().await.map_err(|e| format!("JSON: {e}"))?;
    json["access_token"].as_str().map(|s| s.to_string())
        .ok_or_else(|| "access_token ausente".to_string())
}

fn nonce() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    format!("{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().subsec_nanos())
}

// ── Loop RPC via IPC ──────────────────────────────────────────────────────────
async fn rpc_loop(app: tauri::AppHandle) {
    loop {
        let mut connected = false;

        for i in 0u32..10 {
            let pipe_name = format!(r"\\.\pipe\discord-ipc-{}", i);
            match ClientOptions::new().open(&pipe_name) {
                Ok(pipe) => {
                    println!("[RPC] Conectado via IPC: {}", pipe_name);
                    let _ = app.emit("rpc-status", "Conectado via IPC...");
                    handle_ipc(pipe, app.clone()).await;
                    connected = true;
                    break;
                }
                Err(_) => continue,
            }
        }

        if !connected {
            let _ = app.emit("rpc-status", "Discord Fechado");
            println!("[RPC] Discord não encontrado");
        }

        tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
    }
}

// ── Gerencia sessão IPC ───────────────────────────────────────────────────────
async fn handle_ipc(mut pipe: tokio::net::windows::named_pipe::NamedPipeClient, app: tauri::AppHandle) {
    // 1. Handshake
    let handshake = serde_json::json!({ "v": 1, "client_id": CLIENT_ID }).to_string();
    if write_frame(&mut pipe, OP_HANDSHAKE, &handshake).await.is_err() {
        println!("[RPC] Falha no handshake");
        return;
    }

    loop {
        match read_frame(&mut pipe).await {
            Err(e) => {
                println!("[RPC] Erro leitura: {}", e);
                break;
            }
            Ok((op, text)) => {
                if op == 2 { println!("[RPC] Close frame"); break; } // OP_CLOSE

                let payload: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => continue,
                };

                let cmd = payload["cmd"].as_str().unwrap_or("");
                let evt = payload["evt"].as_str().unwrap_or("");
                println!("[RPC] cmd={} evt={}", cmd, evt);

                // READY → AUTHORIZE
                if cmd == "DISPATCH" && evt == "READY" {
                    let _ = app.emit("rpc-status", "Aguardando autorização...");
                    let msg = serde_json::json!({
                        "nonce": nonce(), "cmd": "AUTHORIZE",
                        "args": { "client_id": CLIENT_ID, "scopes": ["rpc", "identify"] }
                    }).to_string();
                    let _ = write_frame(&mut pipe, OP_FRAME, &msg).await;
                }

                // AUTHORIZE → troca token
                if cmd == "AUTHORIZE" {
                    if let Some(code) = payload["data"]["code"].as_str() {
                        let _ = app.emit("rpc-status", "Obtendo token...");
                        match exchange_token(code).await {
                            Ok(token) => {
                                let _ = app.emit("rpc-status", "Autenticando...");
                                let msg = serde_json::json!({
                                    "nonce": nonce(), "cmd": "AUTHENTICATE",
                                    "args": { "access_token": token }
                                }).to_string();
                                let _ = write_frame(&mut pipe, OP_FRAME, &msg).await;
                            }
                            Err(e) => {
                                let _ = app.emit("rpc-status", format!("Erro token: {e}"));
                                break;
                            }
                        }
                    }
                }

                // AUTHENTICATE → assina e busca canal
                if cmd == "AUTHENTICATE" && payload["data"]["user"].is_object() {
                    let _ = app.emit("rpc-status", "");
                    let subscribe = serde_json::json!({
                        "nonce": nonce(), "cmd": "SUBSCRIBE", "evt": "VOICE_STATE_UPDATE",
                        "args": { "channel_id": CHANNEL_ID }
                    }).to_string();
                    let get_ch = serde_json::json!({
                        "nonce": nonce(), "cmd": "GET_CHANNEL",
                        "args": { "channel_id": CHANNEL_ID }
                    }).to_string();
                    let _ = write_frame(&mut pipe, OP_FRAME, &subscribe).await;
                    let _ = write_frame(&mut pipe, OP_FRAME, &get_ch).await;
                }

                // GET_CHANNEL → emite membros
                if cmd == "GET_CHANNEL" && payload["data"].is_object() {
                    let members: Vec<serde_json::Value> = payload["data"]["voice_states"]
                        .as_array().unwrap_or(&vec![])
                        .iter().map(|vs| vs["user"].clone()).collect();
                    let _ = app.emit("rpc-members", members);
                }

                // VOICE_STATE_UPDATE → re-busca
                if evt == "VOICE_STATE_UPDATE" {
                    let get_ch = serde_json::json!({
                        "nonce": nonce(), "cmd": "GET_CHANNEL",
                        "args": { "channel_id": CHANNEL_ID }
                    }).to_string();
                    let _ = write_frame(&mut pipe, OP_FRAME, &get_ch).await;
                }

                // Erro RPC
                if evt == "ERROR" {
                    let msg = payload["data"]["message"].as_str().unwrap_or("?");
                    let _ = app.emit("rpc-status", format!("Erro RPC: {msg}"));
                    break;
                }
            }
        }
    }

    let _ = app.emit("rpc-status", "RPC Offline");
    let _ = app.emit("rpc-members", Vec::<serde_json::Value>::new());
    println!("[RPC] Desconectado");
}

// ── Comando para abrir Discord no canal ──────────────────────────────────────
#[tauri::command]
fn join_discord() {
    let url = "discord://-/channels/1046592383699132446/1483243850846834751";
    let _ = Command::new("cmd").args(["/c", "start", "", url]).spawn();
}

// ── Entry point ───────────────────────────────────────────────────────────────
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![join_discord])
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(Some(monitor)) = window.primary_monitor() {
                    let logical_size = monitor.size().to_logical::<f64>(monitor.scale_factor());
                    let x = logical_size.width - 450.0 - 20.0;
                    let _ = window.set_position(tauri::Position::Logical(LogicalPosition::new(x, 20.0)));
                }
            }
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move { rpc_loop(handle).await; });
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}