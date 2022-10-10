use std::sync::atomic::{AtomicUsize, Ordering};

use axum::extract::ws::{Message as RawMessage, WebSocket};
use crypto_market_replay::parameter_type::{Action, SenderAction, SocketAction};
use tokio::sync::mpsc::{channel, Sender};

static SOCKET_SESSION_ID: AtomicUsize = AtomicUsize::new(1);

pub async fn handle_socket(mut socket: WebSocket, sender: SenderAction) {
    let (tx_json, mut rx_json) = channel(100);

    let my_id = get_session_id();
    tracing::debug!("new session");

    loop {
        tokio::select! {
            // 处理订阅的消息推送
            json_string = rx_json.recv() =>{
                if socket.send(RawMessage::Text(json_string.unwrap())).await.is_err() {
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
                        json_handle(json, my_id, tx_json.clone(), sender.clone()).await;
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

async fn json_handle(json: String, my_id: usize, tx_json: Sender<String>, sender: SenderAction) {
    tracing::debug!("{:?}", json);

    // 解析 前端发过来的命令 json
    let params: Action = if let Ok(json_action) = serde_json::from_str(&json) {
        json_action
    } else {
        let error_msg = r#"{"msg":"json format error"}"#.to_string();
        let _ = tx_json.send(error_msg).await;
        return;
    };

    // 订阅消息
    match params.action.as_str() {
        "subscribe" => {
            for market_data_arg in params.args.iter() {
                let symbol = market_data_arg.to_string();
                sender
                    .send((my_id, SocketAction::Subscribe(symbol, tx_json.clone())))
                    .await
                    .expect("hander error");
            }
        }
        "unsubscribe" => {
            if params.args.len() == 0 {
                sender
                    .send((my_id, SocketAction::Unsubscribe(None)))
                    .await
                    .expect("hander error");
                return;
            }

            for market_data_arg in params.args.iter() {
                let symbol = market_data_arg.to_string();
                sender
                    .send((my_id, SocketAction::Unsubscribe(Some(symbol))))
                    .await
                    .expect("hander error");
            }
        }
        _ => {
            let error_msg = r#"{"msg":"action subscribe or unsubscribe"}"#.to_string();
            let _ = tx_json.send(error_msg).await.unwrap();
        }
    }
}

fn get_session_id() -> usize {
    return SOCKET_SESSION_ID
        .fetch_add(1, Ordering::Relaxed)
        .try_into()
        .expect("SOCKET_SESSION_ID get error");
}
