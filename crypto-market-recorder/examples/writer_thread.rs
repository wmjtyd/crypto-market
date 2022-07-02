use crypto_market_recorder::create_write_file_thread;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    create_write_file_thread("", "", "", "".to_string())
        .await
        .expect("create_write_file_thread failed");
}
