use axum::{
    body::StreamBody,
    extract::{
        ws::{Message as RawMessage, WebSocket, WebSocketUpgrade},
        Extension, TypedHeader,
    },
    http::{header, StatusCode},
    response::{Response, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use crypto_crawler::{MarketType, Message};

use chrono::{Duration, TimeZone, Utc};
use crypto_msg_parser::parse_l2_topk;
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::Value;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{mpsc::Receiver, Arc},
};
use std::{ops::Add, sync::Mutex};
use axum::http::HeaderValue;
use tokio_util::io::ReaderStream;
use tower_http::trace::{DefaultMakeSpan, TraceLayer};
use websocket::{config::config::ApplicationConfig, init_config};

#[tokio::main]
async fn main() {
    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "example_websockets=debug,tower_http=debug")
    }
    tracing_subscriber::fmt::init();
    let application_config = init_config().await;
    let port = application_config.port;
    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/file", post(handler))
        //绑定websocket路由
        .route("/ws", get(ws_handler))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(true)),
        )
        .layer(Extension(Arc::new(AppState::default())))
        .layer(Extension(application_config));

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
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
    Extension(application_config): Extension<ApplicationConfig>,
) -> impl IntoResponse {
    if let Some(TypedHeader(user_agent)) = user_agent {
        println!("`{}` connected", user_agent.as_str());
    }

    ws.on_upgrade(|socket| handle_socket(socket, state, application_config))
}

async fn handle_socket(
    mut socket: WebSocket,
    state: Arc<AppState>,
    _application_config: ApplicationConfig,
) {
    loop {
        if let Some(msg) = socket.recv().await {
            if let Ok(msg) = msg {
                match msg {
                    RawMessage::Text(t) => {
                        let actions = processing_requests(&t, &state).await;
                        socket.send(RawMessage::Text(actions)).await.unwrap();
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
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}

pub async fn handler(
    Json(params): Json<Params>,
    Extension(application_config): Extension<ApplicationConfig>,
) -> impl IntoResponse {
    println!("1111");
    //Todo: 从配置文件读取配置
    let src = application_config.record_dir + &filename(&params) + ".csv";
    println!("{}", src);
    // let  mut stream = ReaderStream::new(File::open("./data.csv").await.unwrap());

    // //Todo: 同时返回多个文件
    // let path = application_config.record_dir;
    // for file in filename(&params) {
    //     let src = path.clone() + "/" + &file;
    //     let file = match tokio::fs::File::open(src).await {
    //         Ok(file) => file,
    //         Err(err) => return Err((StatusCode::NOT_FOUND, format!("File not found: {}", err))),
    //     };
    //     let stream = ReaderStream::new(file);
    //     let body = StreamBody::new(stream);

    // }
    // let headers = Headers([
    //     (header::CONTENT_TYPE, "text/toml; charset=utf-8"),
    //     (header::CONTENT_DISPOSITION, "attachment; filename=\"data\""),
    // ]);
    // let result: StreamBody<ReaderStream<File>> = StreamBody::new(stream);

    let file = match tokio::fs::File::open(src).await {
        Ok(file) => file,
        Err(err) => return Err((StatusCode::NOT_FOUND, format!("File not found: {}", err))),
    };
    let stream = ReaderStream::new(file);
    let body = StreamBody::new(stream);
    // let headers = Response::Headers([
    //     (header::CONTENT_TYPE, "text/toml; charset=utf-8"),
    //     (header::CONTENT_DISPOSITION, "attachment; filename=\"data\""),
    // ]);
    // let header= Response::headers();
    let mut headers: Response<()> = Response::default();
    headers.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("text/toml; charset=utf-8"));
    headers.headers_mut().insert(header::CONTENT_DISPOSITION, HeaderValue::from_static("attachment; filename=\"data\""));
    Ok((headers, body))
}

pub fn filename(params: &Params) -> String{
    // let mut files = Vec::new();
    let exchange = &params.exchange;
    let market_type = &params.market_type;
    let msg_type = &params.msg_type;
    let symbol = &params.symbols;
    let date = &params.date;
    // date+ "/" + exchange+market_type+msg_type+symbol
    // format!("{}/{}_{}_{}_{}", date,exchange, market_type, msg_type, symbol);
    // let mut begin_datetime = Utc.timestamp(params.begin_datetime, 0);
    // let end_datetime = Utc.timestamp(params.end_datetime, 0);
    // let days = (end_datetime - begin_datetime).num_days();
    let fileName = if let Some(period) = &params.period {
        if period.is_empty() {
            format!("/{}/{}_{}_{}_{}",date,exchange, market_type, msg_type, symbol)
        }else {
            format!("/{}/{}_{}_{}_{}_{}", date,exchange, market_type, msg_type, symbol,period)
        }
        // format!("/{}/{}_{}_{}_{}_{}", date,exchange, market_type, msg_type, symbol,period)
    } else {
        format!("/{}/{}_{}_{}_{}",date,exchange, market_type, msg_type, symbol)
    };
    fileName
    // let mut datetime = format!("{}", begin_datetime.format("%Y%m%d"));
    //循环
    // for _i in 0..days {
    //     let ipc = if let Some(period) = &params.period {
    //         format!(
    //             "{}_{}_{}_{}_{}",
    //             exchange, market_type, msg_type, symbol, period
    //         )
    //     } else {
    //         format!("{}_{}_{}_{}", exchange, market_type, msg_type, symbol)
    //     };
    //     let filename = format!("{}/{}", datetime, ipc);
    //     files.push(filename);
    //     begin_datetime = begin_datetime.add(Duration::days(1));
    //     datetime = format!("{}", begin_datetime.format("%Y%m%d"));
    // }
    // files
}

#[derive(Deserialize)]
pub struct Params {
    pub exchange: String,
    pub market_type: String,
    pub msg_type: String,
    pub symbols: String,
    pub period: Option<String>,
    // pub begin_datetime: i64,
    // pub end_datetime: i64,
    pub date:String,
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
        let result = String::new();
        let receiver = state.receiver.clone();
        tokio::task::spawn_blocking(move || {
            let locked = receiver.lock().unwrap();
            let receiver = locked.get(&echo).unwrap();
            for msg in receiver {
                let msg: Message = msg;
                let received_at = msg.received_at as i64;

                let orderbook = parse_l2_topk(
                    params.params["symbol"].as_str().unwrap(),
                    MarketType::Spot,
                    &(msg as Message).json,
                    Some(received_at),
                )
                .unwrap();
                let orderbook = &orderbook[0];
                result.clone().push_str(&json!(orderbook).to_string());
                break;
            }
        })
        .await
        .unwrap();
    } else {
        if params.action == "subscribe" {
            let mut receiver = state.receiver.lock().unwrap();
            let (_tx, rx) = std::sync::mpsc::channel();
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
