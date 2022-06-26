use axum::{
    body::StreamBody,
    extract::{
        ws::{Message as RawMessage, WebSocket, WebSocketUpgrade},
        Extension, TypedHeader,
    },
    http::{header, StatusCode},
    response::{Headers, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use crypto_crawler::{MarketType, Message, MessageType};

use crypto_msg_parser::parse_l2_topk;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use std::sync::Mutex;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{mpsc::Receiver, Arc},
};
use tokio_util::io::ReaderStream;
use tower_http::trace::{DefaultMakeSpan, TraceLayer};
use chrono::{DateTime, Local, Utc, TimeZone, Timelike};
#[tokio::main]
async fn main() {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "example_websockets=debug,tower_http=debug")
    }
    tracing_subscriber::fmt::init();

    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/file", post(handler))
        //绑定websocket路由
        .route("/ws", get(ws_handler))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(true)),
        )
        .layer(Extension(Arc::new(AppState::default())));

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    tracing::debug!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    user_agent: Option<TypedHeader<headers::UserAgent>>,
    Extension(state): Extension<Arc<AppState>>,
) -> impl IntoResponse {
    if let Some(TypedHeader(user_agent)) = user_agent {
        println!("`{}` connected", user_agent.as_str());
    }

    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    loop {
        if let Some(msg) = socket.recv().await {
            if let Ok(msg) = msg {
                match msg {
                    RawMessage::Text(t) => {
                        let actions = processing_requests(&t, &state).await;
                    }
                    RawMessage::Binary(_) => {
                        println!("client sent binary data");
                    }
                    RawMessage::Ping(_) => {
                        println!("socket ping");
                    }
                    RawMessage::Pong(_) => {
                        println!("socket pong");
                    }
                    RawMessage::Close(_) => {
                        println!("client disconnected");
                        return;
                    }
                }
            } else {
                println!("client disconnected");
                return;
            }
        }
    }
}

pub async fn handler(Json(params): Json<Params>) -> impl IntoResponse {
    //Todo: 从配置文件读取配置
    let src = "../".to_string() + &filename(&params);
    let file = match tokio::fs::File::open("data.txt").await {
        Ok(file) => file,
        Err(err) => return Err((StatusCode::NOT_FOUND, format!("File not found: {}", err))),
    };
    let stream = ReaderStream::new(file);
    let body = StreamBody::new(stream);

    let headers = Headers([
        (header::CONTENT_TYPE, "text/toml; charset=utf-8"),
        (
            header::CONTENT_DISPOSITION,
            "attachment; filename=\"Cargo.toml\"",
        ),
    ]);

    Ok((headers, body))
}

pub fn filename(params: &Params) -> String {
    let exchange = &params.exchange;
    let market_type = &params.market_type;
    let msg_type = &params.msg_type;
    let symbol = &params.symbols;
    let dt = Utc.timestamp(params.begin_datetime, 0);
    let datetime =format!("{}", dt.format("%Y%m%d"));
    let ipc = if let Some(period) = &params.period {
        format!(
            "{}_{}_{}_{}_{}",
            exchange, market_type, msg_type, symbol, period
        )
    } else {
        format!("{}_{}_{}_{}", exchange, market_type, msg_type, symbol)
    };
    format!("{}/{}", datetime, ipc)
}

#[derive(Deserialize)]
pub struct Params {
    pub exchange: String,
    pub market_type: String,
    pub msg_type: String,
    pub symbols: String,
    pub period: Option<String>,
    pub begin_datetime: i64,
    pub end_datetime: i64,
}
#[derive(Serialize, Deserialize)]
pub struct Action {
    pub action: String,
    pub params: Value,
    pub echo: Option<i64>,
}

#[derive(Default)]
pub struct AppState {
    pub receiver: Arc<Mutex<HashMap<i64, Receiver<Message>>>>,
}
pub async fn processing_requests(str: &str, state: &AppState) -> String {
    let params: Action = serde_json::from_str(str).unwrap();
    if let Some(echo) = params.echo {
        let receiver = state.receiver.clone();
        tokio::task::spawn_blocking(move || {
            let locked = receiver.lock().unwrap();
            let receiver = locked.get(&echo).unwrap();
            for msg in receiver {
                let msg: Message = msg;
                let received_at = msg.received_at as i64;

                let orderbook = parse_l2_topk(
                    "binance",
                    MarketType::Spot,
                    &(msg as Message).json,
                    Some(received_at),
                )
                .unwrap();
                let orderbook = &orderbook[0];
            }
        })
        .await
        .unwrap();
    } else {
        if params.action == "subscribe" {
            let mut receiver = state.receiver.lock().unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            let mut rng = rand::thread_rng();
            let y = rng.gen::<i64>();
            receiver.insert(y, rx);
            return "{\"echo\":".to_string() + &y.to_string() + "}";
        }
        if params.action == "unsubscribe" {
            let mut receiver = state.receiver.lock().unwrap();
            let echo = params.params["echo"].as_i64().unwrap();
            receiver.remove(&echo);
            return "{\"echo\":".to_string() + &echo.to_string() + "}";
        }
    }

    return "".to_string();
}
