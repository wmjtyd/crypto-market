use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        TypedHeader, Extension,
    },
    response::{IntoResponse, Headers},
    routing::{get, post},
    Router, http::{StatusCode, header}, body::StreamBody, Json,
};
use crypto_msg_parser::parse_l2_topk;
use serde_json::Value;
use tokio::sync::Mutex;
use std::{net::SocketAddr, collections::HashMap, sync::{mpsc::Receiver, Arc}};
use tower_http::{
    trace::{DefaultMakeSpan, TraceLayer},
};
use tokio_util::io::ReaderStream;
use serde::{Deserialize,Serialize};
use serde_json::{json};
use crypto_crawler::{crawl_l2_event, MarketType, crawl_l2_topk, Message as CryptoMessage , crawl_bbo, crawl_candlestick, crawl_funding_rate, MessageType};

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
) -> impl IntoResponse {
    if let Some(TypedHeader(user_agent)) = user_agent {
        println!("`{}` connected", user_agent.as_str());
    }

    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    loop {
        if let Some(msg) = socket.recv().await {
            if let Ok(msg) = msg {
                match msg {
                    Message::Text(t) => {
                       let actions= processing_requests(&t).await;
 
                    }
                    Message::Binary(_) => {
                        println!("client sent binary data");
                    }
                    Message::Ping(_) => {
                        println!("socket ping");
                    }
                    Message::Pong(_) => {
                        println!("socket pong");
                    }
                    Message::Close(_) => {
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

pub async fn handler(
    Json(params): Json<Params>,
) -> impl IntoResponse {

    //Todo: 要换成根据参数对应的数据
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



#[derive(Deserialize)]
pub struct Params {
    pub exchange: String,
    pub market_type:String,
    pub msg_type:String,
    pub symbols:String,
    pub begin_datetime:i64,
    pub end_datetime:i64
}
#[derive(Serialize,Deserialize)]
pub struct Action {
    pub action: String,
    pub params: Value,
    pub echo: String,
}

#[derive(Default)]
pub struct AppState {
    pub receiver: Mutex<HashMap<i64,Receiver<Message>>>,
}
pub  async fn processing_requests(str: &str){
    let params:Action = serde_json::from_str(str).unwrap();

    // let (tx, rx) = std::sync::mpsc::channel();
    // tokio::task::spawn(async move {
    //     for msg in rx {
    //         let msg:CryptoMessage = msg;
    //         println!("{:#}", msg);
    //         println!("2");
    //         let received_at = msg.received_at as i64;
    //         let orderbook = tokio::task::spawn_blocking(move || parse_l2_topk("binance", MarketType::Spot, &(msg as CryptoMessage).json, Some(received_at)).unwrap()).await.unwrap();
    //         let orderbook = &orderbook[0];
    //         }
    // });

}