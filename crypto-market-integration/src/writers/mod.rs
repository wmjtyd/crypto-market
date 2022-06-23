pub(super) mod file_save;
pub(super) mod file_writer;

use crypto_crawler::*;
use crypto_msg_parser::{parse_bbo, parse_l2, parse_l2_topk, parse_trade};
use futures::{future::BoxFuture, FutureExt};
use log::*;
use nanomsg::{Protocol, Socket};
use redis::{self, Commands};

use std::io::Write;
use std::{
    collections::HashMap,
    sync::{
        mpsc::{Receiver, Sender},
        Arc,
    },
    thread::JoinHandle,
};

pub trait Writer {
    fn write(&mut self, s: &str);
    fn close(&mut self);
}

pub use file_writer::FileWriter;

use crate::data::encode_orderbook;

fn connect_redis(redis_url: &str) -> Result<redis::Connection, redis::RedisError> {
    assert!(!redis_url.is_empty(), "redis_url is empty");

    let mut redis_error: Option<redis::RedisError> = None;
    let mut conn: Option<redis::Connection> = None;
    for _ in 0..3 {
        match redis::Client::open(redis_url) {
            Ok(client) => match client.get_connection() {
                Ok(connection) => {
                    conn = Some(connection);
                    break;
                }
                Err(err) => redis_error = Some(err),
            },
            Err(err) => redis_error = Some(err),
        }
    }

    if let Some(connection) = conn {
        Ok(connection)
    } else {
        Err(redis_error.unwrap())
    }
}

fn create_redis_writer_thread(rx: Receiver<Arc<Message>>, redis_url: String) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut redis_conn = connect_redis(&redis_url).unwrap();
        for msg in rx {
            let msg_type = msg.msg_type;
            let s = serde_json::to_string(msg.as_ref()).unwrap();
            let topic = format!("carbonbot:{}", msg_type);
            if let Err(err) = redis_conn.publish::<&str, String, i64>(&topic, s) {
                error!("{}", err);
                return;
            }
        }
    })
}

fn writers_key(
    writers: &mut HashMap<String, Socket>,
    name: &String
) -> String {
    let file_name = name.replacen("\\", "-", 1);

    if !writers.contains_key(&file_name) {
        let ipc_exchange_market_type_msg_type = file_name.replacen(".", "_", 3);
        let ipc_exchange_market_type_msg_type = format!("ipc:///tmp/{}.ipc", ipc_exchange_market_type_msg_type);
        let topic = String::from("");
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
    _market_type: MarketType,
    msg_type: MessageType,
) {
    tokio::task::spawn(async move {
        let mut writers: HashMap<String, Socket> = HashMap::new();
        for msg in rx {
            println!("msg ->> yes");
            let msg = Arc::new(msg);

            let s = serde_json::to_string(msg.as_ref()).unwrap();

            let msg_r = msg.clone();
            match msg_type {
                MessageType::BBO => {
                    let received_at = 1651122265862;
                    let bbo_msg = tokio::task::spawn_blocking(move || {
                        parse_bbo(exchange, MarketType::Spot, &msg_r.json, Some(received_at)).unwrap()
                    })
                    .await
                    .unwrap();

                    let key = format!("{}.{}.{}.{}", 
                        msg.exchange, msg.market_type, msg.msg_type, bbo_msg.symbol);
                    let key = writers_key(&mut writers, &key);
                    let nanomsg_writer = if let Some(v) = writers.get_mut(&key) {
                        v
                    } else {
                        continue;
                    };

                    // encode
                    nanomsg_writer.write(s.as_bytes()).unwrap();
                }
                MessageType::Trade => {
                    let trade = tokio::task::spawn_blocking(move || {
                        parse_trade(exchange, MarketType::Spot, &msg_r.json).unwrap()
                    })
                    .await
                    .unwrap();

                    let key = format!("{}.{}.{}.{}", 
                        msg.exchange, msg.market_type, msg.msg_type, trade[0].symbol);
                    let key = writers_key(&mut writers, &key);
                    let nanomsg_writer = if let Some(v) = writers.get_mut(&key) {
                        v
                    } else {
                        continue;
                    };

                    let _trade = &trade[0];
                    // encode
                    nanomsg_writer.write(s.as_bytes()).unwrap();
                }
                MessageType::L2Event => {
                    let orderbook = tokio::task::spawn_blocking(move || {
                        parse_l2(exchange, MarketType::Spot, &msg_r.json, None).unwrap()
                    })
                    .await
                    .unwrap();

                    let key = format!("{}.{}.{}.{}", 
                        msg.exchange, msg.market_type, msg.msg_type, orderbook[0].symbol);
                    let key = writers_key(&mut writers, &key);
                    let nanomsg_writer = if let Some(v) = writers.get_mut(&key) {
                        v
                    } else {
                        continue;
                    };


                    let _orderbook = &orderbook[0];
                    // encode
                    nanomsg_writer.write(s.as_bytes()).unwrap();
                }
                MessageType::L2TopK => {
                    let received_at = msg.received_at as i64;
                    let orderbook = tokio::task::spawn_blocking(move || {
                        parse_l2_topk(exchange, MarketType::Spot, &msg_r.json, Some(received_at))
                            .unwrap()
                    })
                    .await
                    .unwrap();
                    
                    let key = format!("{}.{}.{}.{}", 
                        msg.exchange, msg.market_type, msg.msg_type, orderbook[0].symbol);
                    let key = writers_key(&mut writers, &key);
                    let nanomsg_writer = if let Some(v) = writers.get_mut(&key) {
                        v
                    } else {
                        continue;
                    };


                    let orderbook = &orderbook[0];
                    // encode
                    let order_book_bytes = encode_orderbook(orderbook);
                    // send
                    let order_book_bytes_u8 =
                        unsafe { std::mem::transmute::<&[i8], &[u8]>(&order_book_bytes) };
                    // println!("{:?}", String::from_utf8_lossy(order_book_bytes_u8));
                    nanomsg_writer.write(order_book_bytes_u8).unwrap();
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
    data_dir: Option<String>,
    redis_url: Option<String>,
    data_deal_type: &str,
    exchange: &'static str,
    market_type: MarketType,
    msg_type: MessageType,
) -> Vec<BoxFuture<'static, ()>> {
    let mut threads = Vec::new();

    // if data_dir.is_none() && redis_url.is_none() {
    //     error!("Both DATA_DIR and REDIS_URL are not set");
    //     return threads;
    // }

    // if data_dir.is_some() && redis_url.is_some() {
    //     // channel for Redis
    //     let (tx_redis, rx_redis) = std::sync::mpsc::channel::<Message>();
    //     threads.push(create_file_writer_thread(
    //         rx,
    //         data_dir.unwrap(),
    //         Some(tx_redis),
    //     ));
    //     threads.push(create_redis_writer_thread(rx_redis, redis_url.unwrap()));
    // } else if data_dir.is_some() {
    //     threads.push(create_file_writer_thread(rx, data_dir.unwrap(), None))
    // } else {
    //     threads.push(create_redis_writer_thread(rx, redis_url.unwrap()));
    // }
    threads.push(create_nanomsg_writer_thread(rx, None, exchange, market_type, msg_type).boxed());

    threads
}
