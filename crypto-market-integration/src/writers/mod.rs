use crypto_crawler::*;
use crypto_msg_parser::{parse_bbo, parse_candlestick, parse_l2, parse_l2_topk, parse_trade};
use futures::{future::BoxFuture, FutureExt};
use log::*;
use nanomsg::{Protocol, Socket};

use std::io::Write;
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
};

pub trait Writer {
    fn write(&mut self, s: &str);
    fn close(&mut self);
}


fn writers_key(writers: &mut HashMap<String, Socket>, name: &String) -> String {
    let file_name = name.replacen("\\", "-", 1);

    if !writers.contains_key(&file_name) {
        let ipc_exchange_market_type_msg_type = file_name.replacen(".", "_", 4);
        let ipc_exchange_market_type_msg_type =
            format!("ipc:///tmp/{}.ipc", ipc_exchange_market_type_msg_type);
        debug!("{}", ipc_exchange_market_type_msg_type);
        let mut socket = Socket::new(Protocol::Pub).unwrap();
        let _endpoint = socket
            .bind(ipc_exchange_market_type_msg_type.as_str())
            .unwrap();

        writers.insert(file_name.clone(), socket);
    }
    file_name
}

async fn create_nanomsg_writer_thread(
    rx: Receiver<Message>,
    tx_redis: Option<Sender<Arc<Message>>>,
    exchange: &'static str,
    market_type: MarketType,
    msg_type: MessageType,
    period: Arc<String>,
) {
    tokio::task::spawn(async move {
        let mut writers: HashMap<String, Socket> = HashMap::new();
        for msg in rx {
            debug!("msg ->> yes");
            let msg = Arc::new(msg);

            let s = serde_json::to_string(msg.as_ref()).unwrap();

            let msg_r = msg.clone();
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

                    let key = format!(
                        "{}.{}.{}.{}",
                        msg.exchange, msg.market_type, msg.msg_type, bbo_msg.symbol
                    );
                    let key = writers_key(&mut writers, &key);
                    let nanomsg_writer = if let Some(v) = writers.get_mut(&key) {
                        v
                    } else {
                        continue;
                    };

                    // encode
                    let bbo_msg = encode_bbo(&bbo_msg).unwrap();

                    nanomsg_writer.write(&bbo_msg).unwrap();
                }
                MessageType::Trade => {
                    let trade = tokio::task::spawn_blocking(move || {
                        parse_trade(exchange, MarketType::Spot, &msg_r.json).unwrap()
                    })
                    .await
                    .unwrap();

                    let key = format!(
                        "{}.{}.{}.{}",
                        msg.exchange, msg.market_type, msg.msg_type, trade[0].symbol
                    );
                    let key = writers_key(&mut writers, &key);
                    let nanomsg_writer = if let Some(v) = writers.get_mut(&key) {
                        v
                    } else {
                        continue;
                    };

                    let _trade = &trade[0];
                    // encode
                    let trade_bytes_u8 = encode_trade(&trade[0]).unwrap();

                    nanomsg_writer.write(&trade_bytes_u8).unwrap();
                }
                MessageType::L2Event => {
                    let orderbook = tokio::task::spawn_blocking(move || {
                        parse_l2(exchange, MarketType::Spot, &msg_r.json, None).unwrap()
                    })
                    .await
                    .unwrap();

                    let key = format!(
                        "{}.{}.{}.{}",
                        msg.exchange, msg.market_type, msg.msg_type, orderbook[0].symbol
                    );
                    let key = writers_key(&mut writers, &key);
                    let nanomsg_writer = if let Some(v) = writers.get_mut(&key) {
                        v
                    } else {
                        continue;
                    };

                    let order_book_msg = &orderbook[0];
                    let order_book_msg_u8 = 
                        encode_orderbook(&order_book_msg).unwrap();

                    // encode
                    nanomsg_writer.write(&order_book_msg_u8).unwrap();
                }
                MessageType::L2TopK => {
                    let received_at = msg.received_at as i64;
                    let orderbook = tokio::task::spawn_blocking(move || {
                        parse_l2_topk(exchange, MarketType::Spot, &msg_r.json, Some(received_at))
                            .unwrap()
                    })
                    .await
                    .unwrap();

                    let key = format!(
                        "{}.{}.{}.{}",
                        msg.exchange, msg.market_type, msg.msg_type, orderbook[0].symbol
                    );
                    let key = writers_key(&mut writers, &key);
                    let nanomsg_writer = if let Some(v) = writers.get_mut(&key) {
                        v
                    } else {
                        continue;
                    };

                    let orderbook = &orderbook[0];

                    // encode
                    let order_book_bytes = encode_orderbook(orderbook).unwrap();

                    nanomsg_writer.write(&order_book_bytes).unwrap();
                }
                MessageType::Candlestick => {
                    let kline_msg = tokio::task::spawn_blocking(move || {
                        parse_candlestick(exchange, market_type, &msg_r.json, msg_type).unwrap()
                    })
                    .await
                    .unwrap();

                    let key = format!(
                        "{}.{}.{}.{}.{}",
                        msg.exchange, msg.market_type, msg.msg_type, kline_msg.symbol, period
                    );
                    let key = writers_key(&mut writers, &key);
                    let nanomsg_writer = if let Some(v) = writers.get_mut(&key) {
                        v
                    } else {
                        continue;
                    };

                    // encode
                    let kline_msg_bytes = encode_kline(&kline_msg).unwrap();

                    nanomsg_writer.write(&kline_msg_bytes).unwrap();
                }
                _ => panic!("Not implemented"),
            };
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
    let mut threads = Vec::new();

    threads.push(
        create_nanomsg_writer_thread(rx, None, exchange, market_type, msg_type, period).boxed(),
    );

    threads
}
