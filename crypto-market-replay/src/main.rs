use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        TypedHeader,
    },
    response::{IntoResponse, Headers},
    routing::{get, post},
    Router, http::{StatusCode, header}, body::StreamBody, Json,
};
use std::net::SocketAddr;
use tower_http::{
    trace::{DefaultMakeSpan, TraceLayer},
};
use tokio_util::io::ReaderStream;
use serde::{Deserialize};


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
        );

    
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
    if let Some(msg) = socket.recv().await {
        if let Ok(msg) = msg {
            println!("Client says: {:?}", msg);
            //客户端发什么，服务端就回什么（只是演示而已）
            if socket
                .send(Message::Text(format!("{:?}", msg)))
                .await
                .is_err()
            {
                println!("client disconnected");
                return;
            }
        } else {
            println!("client disconnected");
            return;
        }
    }
}

async fn handler(
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
struct Params {
    exchange: String,
    market_type:String,
    msg_type:String,
    symbols:String,
    begin_datetime:i64,
    end_datetime:i64
}
