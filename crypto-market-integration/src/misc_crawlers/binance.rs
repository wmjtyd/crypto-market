use std::sync::mpsc::Sender;

use crypto_crawler::{MarketType, Message, MessageType};
use crypto_ws_client::{BinanceInverseWSClient, BinanceLinearWSClient, WSClient};

use super::utils::create_conversion_thread;

pub(super) async fn crawl_other(market_type: MarketType, tx: Sender<Message>) {
    let tx = create_conversion_thread("binance".to_string(), MessageType::Other, market_type, tx);
    let commands =
        vec![r#"{"id":9527,"method":"SUBSCRIBE","params":["!forceOrder@arr"]}"#.to_string()];

    match market_type {
        MarketType::InverseSwap | MarketType::InverseFuture => {
            let ws_client = BinanceInverseWSClient::new(tx, None).await;
            ws_client.send(&commands).await;
            ws_client.run().await;
            ws_client.close();
        }
        MarketType::LinearSwap | MarketType::LinearFuture => {
            let ws_client = BinanceLinearWSClient::new(tx, None).await;
            ws_client.send(&commands).await;
            ws_client.run().await;
            ws_client.close();
        }
        _ => panic!("Unknown market_type {}", market_type),
    }
}
