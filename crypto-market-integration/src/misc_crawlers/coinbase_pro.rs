use std::sync::mpsc::Sender;

use crypto_crawler::{Message, MarketType, MessageType};
use crypto_ws_client::{CoinbaseProWSClient, WSClient};

use super::utils::create_conversion_thread;

pub(super) async fn crawl_other(market_type: MarketType, tx: Sender<Message>) {
    let tx = create_conversion_thread(
        "coinbase_pro".to_string(),
        MessageType::Other,
        market_type,
        tx,
    );
    let commands: Vec<String> =
        vec![r#"{"type": "subscribe","channels":[{ "name": "status"}]}"#.to_string()];

    let ws_client = CoinbaseProWSClient::new(tx, None).await;
    ws_client.send(&commands).await;
    ws_client.run().await;
    ws_client.close();
}
