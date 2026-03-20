use std::collections::HashMap;
use std::process::Command;
use std::sync::Mutex;
use tauri::{Emitter, LogicalPosition, Manager, State};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::ClientOptions;

const CLIENT_ID: &str = "1484342268960968907";
const CLIENT_SECRET: &str = "Ktra5iMCna07u1VDq_8aCp6rJEs2s0fg";
const CHANNEL_ID: &str = "1483243850846834751";

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;

// ── Estado compartilhado: retângulos dos widgets em CSS pixels ────────────────
// Cada rect = [left, top, right, bottom] relativo à janela
struct WidgetRects(Mutex<Vec<[f64; 4]>>);

// ── Comando chamado pelo JS ao montar e ao reorganizar widgets ────────────────
#[tauri::command]
fn set_widget_rects(rects: Vec<[f64; 4]>, state: State<WidgetRects>) {
    *state.0.lock().unwrap() = rects;
}

// ── FFI para GetCursorPos sem dependência extra ───────────────────────────────
fn get_cursor_pos() -> Option<(i32, i32)> {
    #[cfg(target_os = "windows")]
    {
        #[repr(C)]
        struct POINT { x: i32, y: i32 }
        extern "system" { fn GetCursorPos(pt: *mut POINT) -> i32; }
        let mut pt = POINT { x: 0, y: 0 };
        unsafe {
            if GetCursorPos(&mut pt) != 0 {
                return Some((pt.x, pt.y));
            }
        }
    }
    None
}

// ─────────────────────────────────────────────────────────────────────────────

async fn write_frame<W: AsyncWriteExt + Unpin>(w: &mut W, op: u32, json: &str) -> std::io::Result<()> {
    let bytes = json.as_bytes();
    let mut buf = Vec::with_capacity(8 + bytes.len());
    buf.extend_from_slice(&op.to_le_bytes());
    buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    buf.extend_from_slice(bytes);
    w.write_all(&buf).await
}

async fn read_frame<R: AsyncReadExt + Unpin>(r: &mut R) -> std::io::Result<(u32, String)> {
    let mut header = [0u8; 8];
    r.read_exact(&mut header).await?;
    let op  = u32::from_le_bytes(header[0..4].try_into().unwrap());
    let len = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    Ok((op, String::from_utf8_lossy(&body).to_string()))
}

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

async fn handle_ipc(mut pipe: tokio::net::windows::named_pipe::NamedPipeClient, app: tauri::AppHandle) {
    let handshake = serde_json::json!({ "v": 1, "client_id": CLIENT_ID }).to_string();
    if write_frame(&mut pipe, OP_HANDSHAKE, &handshake).await.is_err() {
        println!("[RPC] Falha no handshake");
        return;
    }

    loop {
        match read_frame(&mut pipe).await {
            Err(e) => { println!("[RPC] Erro leitura: {}", e); break; }
            Ok((op, text)) => {
                if op == 2 { println!("[RPC] Close frame"); break; }

                let payload: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(v) => v, Err(_) => continue,
                };

                let cmd = payload["cmd"].as_str().unwrap_or("");
                let evt = payload["evt"].as_str().unwrap_or("");

                if cmd == "DISPATCH" && evt == "READY" {
                    let _ = app.emit("rpc-status", "Aguardando autorização...");
                    let msg = serde_json::json!({
                        "nonce": nonce(), "cmd": "AUTHORIZE",
                        "args": { "client_id": CLIENT_ID, "scopes": ["rpc", "identify"] }
                    }).to_string();
                    let _ = write_frame(&mut pipe, OP_FRAME, &msg).await;
                }

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

                if cmd == "GET_CHANNEL" && payload["data"].is_object() {
                    let members: Vec<serde_json::Value> = payload["data"]["voice_states"]
                        .as_array().unwrap_or(&vec![])
                        .iter().map(|vs| vs["user"].clone()).collect();
                    let _ = app.emit("rpc-members", members);
                }

                if evt == "VOICE_STATE_UPDATE" {
                    let get_ch = serde_json::json!({
                        "nonce": nonce(), "cmd": "GET_CHANNEL",
                        "args": { "channel_id": CHANNEL_ID }
                    }).to_string();
                    let _ = write_frame(&mut pipe, OP_FRAME, &get_ch).await;
                }

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

#[tauri::command]
fn join_discord() {
    let url = "discord://-/channels/1046592383699132446/1483243850846834751";
    let _ = Command::new("cmd").args(["/c", "start", "", url]).spawn();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(WidgetRects(Mutex::new(vec![])))
        .invoke_handler(tauri::generate_handler![join_discord, set_widget_rects])
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(Some(monitor)) = window.primary_monitor() {
                    let logical_size = monitor.size().to_logical::<f64>(monitor.scale_factor());
                    let x = logical_size.width - 450.0 - 20.0;
                    let _ = window.set_position(tauri::Position::Logical(LogicalPosition::new(x, 20.0)));
                }

                // Começa ignorando cursor (click-through ativo)
                let _ = window.set_ignore_cursor_events(true);

                // Obtém o estado gerenciado para o loop
                let rects_state = app.state::<WidgetRects>();
                // Como não podemos clonar State diretamente para a thread, 
                // para esse caso simples no run() podemos acessar o ponteiro ou 
                // simplesmente fazer o loop em um runtime que tenha acesso.
                
                let win_ct = window.clone();
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    let state = handle.state::<WidgetRects>();
                    let mut last_ignore: Option<bool> = None;
                    loop {
                        tokio::time::sleep(tokio::time::Duration::from_millis(16)).await;
                        let outer_pos  = match win_ct.outer_position() { Ok(p) => p, Err(_) => continue };
                        let outer_size = match win_ct.outer_size()      { Ok(s) => s, Err(_) => continue };
                        let scale      = match win_ct.scale_factor()    { Ok(s) => s, Err(_) => continue };
                        let (cx, cy)   = match get_cursor_pos()         { Some(p) => p, None => continue };

                        let in_window = cx >= outer_pos.x
                            && cy >= outer_pos.y
                            && cx < outer_pos.x + outer_size.width  as i32
                            && cy < outer_pos.y + outer_size.height as i32;

                        let ignore = if !in_window {
                            true
                        } else {
                            let lx = (cx - outer_pos.x) as f64 / scale;
                            let ly = (cy - outer_pos.y) as f64 / scale;
                            let rects = state.0.lock().unwrap();
                            let over_widget = rects.iter().any(|r| {
                                lx >= r[0] && ly >= r[1] && lx < r[2] && ly < r[3]
                            });
                            !over_widget
                        };

                        if last_ignore != Some(ignore) {
                            let _ = win_ct.set_ignore_cursor_events(ignore);
                            last_ignore = Some(ignore);
                        }
                    }
                });
            }

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move { rpc_loop(handle).await; });
            Ok(())
        })
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}