use std::sync::mpsc::Sender;

use crypto_crawler::{MarketType, Message, MessageType};
use crypto_ws_client::{HuobiSpotWSClient, WSClient};

use super::utils::create_conversion_thread;

pub(super) async fn crawl_other(market_type: MarketType, tx: Sender<Message>) {
    let tx = create_conversion_thread("huobi".to_string(), MessageType::Other, market_type, tx);
    let commands = vec![r#"{"sub":"market.overview","id":"crypto-ws-client"}"#.to_string()];

    match market_type {
        MarketType::Spot => {
            let ws_client = HuobiSpotWSClient::new(tx, None).await;
            ws_client.send(&commands).await;
            ws_client.run().await;
            ws_client.close();
        }
        MarketType::InverseFuture => {
            let ws_client = HuobiSpotWSClient::new(tx, None).await;
            ws_client.send(&commands).await;
            ws_client.run().await;
            ws_client.close();
        }
        MarketType::LinearSwap => {
            let ws_client = HuobiSpotWSClient::new(tx, None).await;
            ws_client.send(&commands).await;
            ws_client.run().await;
            ws_client.close();
        }
        MarketType::InverseSwap => {
            let ws_client = HuobiSpotWSClient::new(tx, None).await;
            ws_client.send(&commands).await;
            ws_client.run().await;
            ws_client.close();
        }
        MarketType::EuropeanOption => {
            let ws_client = HuobiSpotWSClient::new(tx, None).await;
            ws_client.send(&commands).await;
            ws_client.run().await;
            ws_client.close();
        }
        _ => panic!("Unknown market_type {}", market_type),
    };
}
