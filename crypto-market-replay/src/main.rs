use axum::extract::{Extension, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Router;
use clap::clap_app;
use crypto_market_replay::config::appliaction::ApplicationConfig;
use crypto_market_replay::parameter_type::SenderAction;
use crypto_market_replay::{http_market_data, msg_sub_handle};
use tower_http::trace::{DefaultMakeSpan, TraceLayer};

mod handle_socket;
use handle_socket::handle_socket;

#[tokio::main]
async fn main() {
    let matches: clap::ArgMatches = clap_app!(quic =>
        (about: "Signal calculation organization procedure")
        (@arg HOST: -h --host "bind host")
        (@arg PORT: -p --port "bind port")
        (@arg LOG: --log "log log save directory 'logs/'")
        (@arg LEVEL: --level "log show level")
        (@arg DATA: --data "market data base directory")
    )
    .get_matches();

    if std::env::var_os("RUST_LOG").is_none() {
        std::env::set_var("RUST_LOG", "example_websockets=debug,tower_http=debug")
    }
    tracing_subscriber::fmt::init();

    let (tx, rx) = tokio::sync::mpsc::channel(1024);

    let application_config = ApplicationConfig::new(&matches);

    let addr = application_config.server.get_addr();

    tokio::spawn(msg_sub_handle(rx));

    let app = Router::new()
        // http
        .route("/", get(|| async { "Hello, World!" }))
        .route("/file", post(http_market_data))
        // websocket
        .route("/ws", get(ws_market_data))
        // global data
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(DefaultMakeSpan::default().include_headers(true)),
        )
        .layer(Extension(application_config))
        .layer(Extension(tx));

    tracing::debug!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

async fn ws_market_data(
    ws: WebSocketUpgrade,
    Extension(_application_config): Extension<ApplicationConfig>,
    Extension(tx): Extension<SenderAction>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, tx.clone()))
}
