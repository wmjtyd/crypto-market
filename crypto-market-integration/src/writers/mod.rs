use crypto_crawler::*;
use crypto_msg_parser::{parse_bbo, parse_candlestick, parse_l2, parse_l2_topk, parse_trade, parse_funding_rate};
use futures::{future::BoxFuture, FutureExt};
use log::*;
use tokio::io::AsyncWriteExt;
use wmjtyd_libstock::message::zeromq::Pub;
use wmjtyd_libstock::message::zeromq::Zeromq;

use std::{
    collections::HashMap,
    sync::{
        mpsc::{Receiver, Sender},
        Arc,
    }
};


use wmjtyd_libstock::data::{
    bbo::encode_bbo,
    kline::encode_kline,
    orderbook::encode_orderbook,
    trade::encode_trade,
    funding_rate::encode_funding_rate,
};

pub trait Writer {
    fn write(&mut self, s: &str);
    fn close(&mut self);
}


// Quickly create a message queue
async fn create(name: &str) -> impl tokio::io::AsyncWriteExt  {
    let file_name = name.replacen('/', "-", 3);

    let ipc_exchange_market_type_msg_type =
        format!("ipc:///tmp/{}.ipc", file_name);
    
    let socket = match Zeromq::<Pub>::new(&ipc_exchange_market_type_msg_type).await {
        Ok(v) =>  v,
        Err(msg) => panic!("init publish error: {}; msg: {:?}", ipc_exchange_market_type_msg_type, msg)
    };
    socket
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
        let mut writers= HashMap::new();

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
                        parse_bbo(
                            exchange,
                            MarketType::Spot,
                            &msg_r.json,
                            Some(received_at as i64),
                        )
                        .unwrap()
                    })
                    .await
                    .unwrap();


                    let symbol = bbo_msg.symbol.to_owned();
                    let byte_data = encode_bbo(&bbo_msg).unwrap();
                    data_vec.push((symbol, byte_data));
                }
                MessageType::Trade => {
                    let trade_msg = tokio::task::spawn_blocking(move || {
                        parse_trade(exchange, MarketType::Spot, &msg_r.json).unwrap()
                    })
                    .await
                    .unwrap();


                    for trdate in trade_msg {
                        let symbol = trdate.symbol.to_owned();
                        let byte_data = encode_trade(&trdate).unwrap();
                        data_vec.push((symbol, byte_data));
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
                        let byte_data = encode_orderbook(&orderbook).unwrap();
                        data_vec.push((symbol, byte_data));
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
                        let byte_data = encode_orderbook(&orderbook).unwrap();
                        data_vec.push((symbol, byte_data));
                    }
                }
                MessageType::Candlestick => {
                    let kline_msg = tokio::task::spawn_blocking(move || {
                        parse_candlestick(exchange, market_type, &msg_r.json, msg_type).unwrap()
                    })
                    .await
                    .unwrap();

                    let symbol = kline_msg.symbol.to_owned();
                    let byte_data = encode_kline(&kline_msg).unwrap();
                    data_vec.push((symbol, byte_data));
                }
                MessageType::FundingRate => {
                    let funding_rate_msg = tokio::task::spawn_blocking(move || {
                        println!("{}", msg_r.json);
                        parse_funding_rate(exchange, market_type, &msg_r.json, Some(1232))
                    }).await.unwrap().unwrap();

                    for mut funding_rate in funding_rate_msg {

                        if  None == funding_rate.estimated_rate  {
                            funding_rate.estimated_rate = Some(0.0);
                        }

                        let symbol = funding_rate.symbol.to_owned();
                        let byte_data = encode_funding_rate(&funding_rate).unwrap();
                        data_vec.push((symbol, byte_data));
                    }

                }
                _ => panic!("Not implemented"),
            };

            // Send a message to the corresponding message queue
            for (symbol, data_byte) in data_vec {
                let key = if period.len() != 0 {
                    format!(
                        "{}_{}_{}_{}_{}",
                        msg.exchange, msg.market_type, msg.msg_type, symbol, period
                    )
                } else {
                    format!(
                        "{}_{}_{}_{}",
                        msg.exchange, msg.market_type, msg.msg_type, symbol
                    )
                };
                debug!("{}", key);
                let writer_mq = if writers.contains_key(&key) {
                    writers.get_mut(&key).unwrap()
                } else {
                    println!("{key}");
                    let socket = create(&key).await;
                    writers.insert(key.to_owned(), socket);
                    writers.get_mut(&key).unwrap()
                };
                writer_mq.write_all(&data_byte).await.unwrap();
            }

            // copy to redis
            if let Some(ref tx_redis) = tx_redis {
                tx_redis.send(msg).unwrap();
            }
        }
    })
    .await
    .expect("create_nanomsg_writer_thread failed");
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
    vec![
        create_writer_thread(rx, None, exchange, market_type, msg_type, period).boxed()
    ]
}
