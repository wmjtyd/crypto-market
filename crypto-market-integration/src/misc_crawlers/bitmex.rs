use std::sync::mpsc::Sender;

use crypto_crawler::{MarketType, Message, MessageType};
use crypto_ws_client::{BitmexWSClient, WSClient};

use super::utils::create_conversion_thread;

pub(super) async fn crawl_other(market_type: MarketType, tx: Sender<Message>) {
    assert_eq!(market_type, MarketType::Unknown);
    let tx = create_conversion_thread("bitmex".to_string(), MessageType::Other, market_type, tx);
    let commands: Vec<String> = vec![
        "announcement",
        "connected",
        "instrument",
        "insurance",
        "liquidation",
        "publicNotifications",
        "settlement",
    ]
    .into_iter()
    .map(|x| format!(r#"{{"op":"subscribe","args":["{}"]}}"#, x))
    .collect();

    let ws_client = BitmexWSClient::new(tx, None).await;
    ws_client.send(&commands).await;
    ws_client.run().await;
    ws_client.close();
}
