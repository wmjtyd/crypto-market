use axum::body::StreamBody;
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use headers::HeaderValue;
use tokio_util::io::ReaderStream;

use crate::config::appliaction::ApplicationConfig;
use crate::parameter_type::Params;

pub async fn http_market_data(
    Json(params): Json<Params>,
    Extension(application_config): Extension<ApplicationConfig>,
) -> impl IntoResponse {
    let base_path = application_config.market_data_dir;

    let path = market_data_path(&base_path, &params);

    let file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(err) => return Err((StatusCode::NOT_FOUND, format!("File not found: {}", err))),
    };

    let stream = ReaderStream::new(file);
    let body = StreamBody::new(stream);

    let mut headers: Response<()> = Response::default();
    headers.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/octet-stream"),
    );
    headers.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_static(r#"attachment; filename="market-data""#),
    );
    Ok((headers, body))
}

pub fn market_data_path(base_path: &str, params: &Params) -> String {
    let time = if let Some(elapse) = params.elapse {
        chrono::Local::now() - chrono::Duration::days(elapse)
    } else {
        chrono::Local::now()
    }
    .format("%Y%m%d")
    .to_string();

    let exchange = &params.exchange;
    let market_type = &params.market_type;
    let msg_type = &params.msg_type;
    let symbol = &params.symbols;

    let mut period = String::new();

    if let Some(p) = &params.period {
        if p.is_empty() {
            period = format!("_{}", period);
        }
    }
    let symbol = format!(
        "{}_{}_{}_{}{}",
        exchange, market_type, msg_type, symbol, period
    );

    format!("{base_path}/{time}/{symbol}.csv")
}
