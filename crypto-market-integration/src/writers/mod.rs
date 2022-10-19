use std::collections::HashMap;
use std::io::Write;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use crypto_crawler::*;
use crypto_msg_parser::{
    parse_bbo,
    parse_candlestick,
    parse_funding_rate,
    parse_l2,
    parse_l2_topk,
    parse_trade,
};
use futures::future::BoxFuture;
use futures::FutureExt;
use log::*;
use wmjtyd_libstock::data::bbo::BboStructure;
use wmjtyd_libstock::data::funding_rate::FundingRateStructure;
use wmjtyd_libstock::data::kline::KlineStructure;
use wmjtyd_libstock::data::orderbook::OrderbookStructure;
use wmjtyd_libstock::data::serializer::StructSerializer;
use wmjtyd_libstock::data::trade::TradeStructure;
use wmjtyd_libstock::message::traits::{Bind, SyncPublisher};
use wmjtyd_libstock::message::zeromq::ZeromqPublisher;

pub trait Writer {
    fn write(&mut self, s: &str);
    fn close(&mut self);
}

// Quickly create a message queue
async fn create(name: &str) -> impl SyncPublisher {
    let file_name = name.replacen('/', "-", 3);

    let ipc_exchange_market_type_msg_type = format!("ipc:///tmp/{}.ipc", file_name);

    if let Ok(mut publisher) = ZeromqPublisher::new() {
        if publisher.bind(&ipc_exchange_market_type_msg_type).is_err() {
            // 以后需要处理一下
            panic!("ipc bind error {}", ipc_exchange_market_type_msg_type);
        }
        publisher
    } else {
        panic!("init publish error");
    }
}

async fn create_writer_thread(
    rx: Receiver<Message>,
    tx_redis: Option<Sender<Arc<Message>>>,
    exchange: &'static str,
    market_type: MarketType,
    msg_type: MessageType,
    period: Arc<String>,
) {
    tokio::task::spawn(async move {
        let mut writers = HashMap::new();

        for msg in rx {
            debug!("msg ->> yes");
            let msg = Arc::new(msg);
            let msg_r = msg.clone();
            let mut data_vec = Vec::new();

            // Convert the message to &[u8]
            match msg_type {
                MessageType::BBO => {
                    let received_at = msg_r.received_at;
                    let bbo_msg = tokio::task::spawn_blocking(move || {
                        // FIXME: bbo returns multiple results!
                        parse_bbo(
                            exchange,
                            MarketType::Spot,
                            &msg_r.json,
                            Some(received_at as i64),
                        )
                        .unwrap()
                        .swap_remove(0)
                    })
                    .await
                    .unwrap();

                    let symbol = bbo_msg.symbol.to_owned();

                    if let Ok(bbo_structure) = BboStructure::try_from(&bbo_msg) {
                        let mut byte_data = Vec::new();
                        if bbo_structure.serialize(&mut byte_data).is_err() {
                            continue;
                        }
                        data_vec.push((symbol, byte_data));
                    };
                }
                MessageType::Trade => {
                    let trade_msg = tokio::task::spawn_blocking(move || {
                        parse_trade(exchange, MarketType::Spot, &msg_r.json).unwrap()
                    })
                    .await
                    .unwrap();

                    for trdate in trade_msg {
                        let symbol = trdate.symbol.to_owned();

                        if let Ok(trade_structure) = TradeStructure::try_from(&trdate) {
                            let mut byte_data = Vec::new();
                            if trade_structure.serialize(&mut byte_data).is_err() {
                                continue;
                            }

                            data_vec.push((symbol, byte_data));
                        };
                    }
                }
                MessageType::L2Event => {
                    let orderbook_msg = tokio::task::spawn_blocking(move || {
                        parse_l2(exchange, MarketType::Spot, &msg_r.json, None).unwrap()
                    })
                    .await
                    .unwrap();

                    for orderbook in orderbook_msg {
                        let symbol = orderbook.symbol.to_owned();

                        if let Ok(order_book_structure) = OrderbookStructure::try_from(&orderbook) {
                            let mut byte_data = Vec::new();
                            if order_book_structure.serialize(&mut byte_data).is_err() {
                                continue;
                            }
                            data_vec.push((symbol, byte_data));
                        };
                    }
                }
                MessageType::L2TopK => {
                    let received_at = msg.received_at as i64;
                    let orderbook_msg = tokio::task::spawn_blocking(move || {
                        parse_l2_topk(exchange, MarketType::Spot, &msg_r.json, Some(received_at))
                            .unwrap()
                    })
                    .await
                    .unwrap();

                    for orderbook in orderbook_msg {
                        let symbol = orderbook.symbol.to_owned();

                        if let Ok(order_book_structure) = OrderbookStructure::try_from(&orderbook) {
                            let mut byte_data = Vec::new();
                            if order_book_structure.serialize(&mut byte_data).is_err() {
                                continue;
                            }
                            data_vec.push((symbol, byte_data));
                        };
                    }
                }
                MessageType::Candlestick => {
                    let kline_msg = tokio::task::spawn_blocking(move || {
                        // FIXME: candlestick returns multiple results!
                        parse_candlestick(exchange, market_type, &msg_r.json, None).unwrap()
                    })
                    .await
                    .unwrap();

                    for kline in kline_msg {
                        let symbol = kline.symbol.to_owned();

                        if let Ok(order_book_structure) = KlineStructure::try_from(&kline) {
                        let mut byte_data = Vec::new();
                            if order_book_structure.serialize(&mut byte_data).is_err() {
                            continue;
                        }
                        data_vec.push((symbol, byte_data));
                    };
                }
                }
                MessageType::FundingRate => {
                    let funding_rate_msg = tokio::task::spawn_blocking(move || {
                        parse_funding_rate(exchange, market_type, &msg_r.json, Some(1232))
                    })
                    .await
                    .unwrap()
                    .unwrap();

                    for mut funding_rate in funding_rate_msg {
                        if None == funding_rate.estimated_rate {
                            funding_rate.estimated_rate = Some(0.0);
                        }

                        let symbol = funding_rate.symbol.to_owned();
                        if let Ok(funding_rate_structure) =
                            FundingRateStructure::try_from(&funding_rate)
                        {
                            let mut byte_data = Vec::new();
                            if funding_rate_structure.serialize(&mut byte_data).is_err() {
                                continue;
                            }
                            data_vec.push((symbol, byte_data));
                        };
                    }
                }
                _ => panic!("Not implemented"),
            };

            // Send a message to the corresponding message queue
            for (symbol, data_byte) in data_vec {
                let key = format!(
                    "{}_{}_{}_{}{}{}",
                    msg.exchange, 
                    msg.market_type, 
                    msg.msg_type, 
                    symbol, 
                    if period.len() == 0 {""} else {"_"},
                    period
                );
                let writer_mq = if writers.contains_key(&key) {
                    writers.get_mut(&key).unwrap()
                } else {
                    debug!("{key}");
                    let socket = create(&key).await;
                    writers.insert(key.to_owned(), socket);
                    writers.get_mut(&key).unwrap()
                };
                if writer_mq.write_all(&data_byte).is_err() || writer_mq.flush().is_err() {
                    continue;
                }
                // writer_mq.
                // (&data_byte).await.unwrap();
            }

            // copy to redis
            if let Some(ref tx_redis) = tx_redis {
                tx_redis.send(msg).unwrap();
            }
        }
    })
    .await
    .expect("create_writer_thread failed");
}

pub fn create_writer_threads(
    rx: Receiver<Message>,
    _data_dir: Option<String>,
    _redis_url: Option<String>,
    exchange: &'static str,
    market_type: MarketType,
    msg_type: MessageType,
    period: Arc<String>,
) -> Vec<BoxFuture<'static, ()>> {
    vec![create_writer_thread(rx, None, exchange, market_type, msg_type, period).boxed()]
}
