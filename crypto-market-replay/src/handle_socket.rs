use std::sync::atomic::{AtomicUsize, Ordering};

use axum::extract::ws::{Message as RawMessage, WebSocket};
use crypto_market_replay::parameter_type::{Action, SenderAction, SocketAction, MarketDataArg};
use serde_json::json;
use tokio::sync::mpsc::{channel, Sender};
use wmjtyd_libstock::file::reader::FileReader;

static SOCKET_SESSION_ID: AtomicUsize = AtomicUsize::new(1);

pub async fn handle_socket(mut socket: WebSocket, sender: SenderAction) {
    let (tx_json, mut rx_json) = channel(100);

    let my_id = get_session_id();
    tracing::debug!("new session");

    loop {
        tokio::select! {
            // 处理订阅的消息推送
            json_string = rx_json.recv() =>{
                let json = json_string.unwrap();
                tracing::debug!("json {}", json);
                if socket.send(RawMessage::Text(json)).await.is_err() {
                    tracing::error!("websocket sent binary data");
                    // 发送失败
                    break;
                }
            }
            // 处理 websocket 中接收到的数据处理
            Some(msg) = socket.recv()=>{
                // 处理 数据接收的时候出现的问题
                if let Err(msg) = msg {
                    println!("client disconnected {:?}", msg);
                    break;
                }

                // 消息处理
                match msg.unwrap() {
                    // 这里的 json 是前端发送过来的数据
                    RawMessage::Text(json) => {
                        let r = json_handle(json, my_id, tx_json.clone(), sender.clone()).await;
                        if r.is_err() {
                            break;
                        }
                    }
                    RawMessage::Ping(_) => {
                        tracing::warn!("socket ping");
                    }
                    RawMessage::Pong(_) => {
                        tracing::warn!("socket pong");
                    }
                    RawMessage::Close(_) => {
                        break;
                    }
                    RawMessage::Binary(_) => {
                        tracing::warn!("client sent binary data");
                        let error_msg = r#"{"msg":"Binary data is not accepted"}"#;
                        socket.send(RawMessage::Text(error_msg.to_string())).await.unwrap();
                        continue;
                    }
                }
            }
        }
    }
    sender
        .send((my_id, SocketAction::Unsubscribe(None)))
        .await
        .expect("hander error");
    tracing::info!("websocket not session");
}

async fn json_handle(json: String, my_id: usize, tx_json: Sender<String>, sender: SenderAction) -> anyhow::Result<()>{
    tracing::debug!("{:?}", json);

    // 解析 前端发过来的命令 json
    let params: Action = serde_json::from_str(&json)?;
    let mut action = params.action.split("@");
    let action_name = action.next().unwrap_or("");

    // 订阅消息
    match action_name {
        "subscribe" => {
            for market_data_arg in params.args.iter() {
                let symbol = market_data_arg.to_string();
                sender
                    .send((my_id, SocketAction::Subscribe(symbol, tx_json.clone())))
                    .await?;
            }
        }
        "unsubscribe" => {
            if params.args.len() == 0 {
                sender
                    .send((my_id, SocketAction::Unsubscribe(None)))
                    .await?;
                return Ok(());
            }

            for market_data_arg in params.args.iter() {
                let symbol = market_data_arg.to_string();
                sender
                    .send((my_id, SocketAction::Unsubscribe(Some(symbol))))
                    .await?;
            }
        }
        "history" => {
            let mut time_range = action.next().unwrap_or("0-0").split("-");
            let start = time_range.next().unwrap_or("0");
            let end = time_range.next().unwrap_or(start);

            let start: i64 = start.parse()?;
            let end: i64 = end.parse()?;

            if 0 <= end && start < end {
                let msg = json!({
                    "code": 500, 
                    "msg": "history time range error"
                }).to_string();
                tx_json.send(msg).await?;
            }
            
            tokio::task::spawn(history_handle(start, end, tx_json, params.args));
        }
        _ => {
            let error_msg = json!({
                "code": 5001,
                "msg":"action subscribe or unsubscribe"
            }).to_string();
            let _ = tx_json.send(error_msg).await?;
        }
    }
    Ok(())
}

async fn history_handle(start: i64, end: i64, tx_json: Sender<String>, args: Vec<MarketDataArg>) {
    for market_data_arg in args.iter() {
        let mut i = start;
        // start = 6, end = 1
        // 6-1
        // 这里的时间是过去的时间
        // 过去的 6 天前的数据
        // 过去 1 天前的数据
        while i >= end {
            let symbol = market_data_arg.to_string();
            let read = if let Ok(v) = FileReader::new(symbol, i) {
                v
            } else {
                let msg = json!({
                    "code": 5002,
                    "msg": format!("{} not {} day history",market_data_arg.to_string(), i)
                }).to_string();
                tx_json.send(msg).await.expect("tex_json send error");
                break;
            };

            tracing::debug!("day {}", i);

            for data in read {
                let json = json!({
                    "messageType": market_data_arg.msg_type.to_owned(),
                    "payload": base64::encode(data)
                }).to_string();
                tx_json.send(json).await.expect("tex_json send error");
            }
            i += 1;
        }
    }
}

fn get_session_id() -> usize {
    return SOCKET_SESSION_ID
        .fetch_add(1, Ordering::Relaxed)
        .try_into()
        .expect("SOCKET_SESSION_ID get error");
}
