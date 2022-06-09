pub(super) mod file_writer;

use crypto_crawler::*;
use crypto_msg_parser::{parse_l2, parse_trade, parse_bbo};
use log::*;
use redis::{self, Commands};
use std::{
    collections::HashMap,
    path::Path,
    sync::mpsc::{Receiver, Sender},
    thread::JoinHandle,
};
use nanomsg::{Protocol, Socket};
use std::io::{Read, Write};

pub trait Writer {
    fn write(&mut self, s: &str);
    fn close(&mut self);
}

pub use file_writer::FileWriter;

fn create_file_writer_thread(
    rx: Receiver<Message>,
    data_dir: String,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut writers: HashMap<String, FileWriter> = HashMap::new();
        for msg in rx {
            let file_name = format!("{}.{}.{}", msg.exchange, msg.market_type, msg.msg_type);
            if !writers.contains_key(&file_name) {
                let data_dir = Path::new(&data_dir)
                    .join(msg.msg_type.to_string())
                    .join(&msg.exchange)
                    .join(msg.market_type.to_string())
                    .into_os_string();
                std::fs::create_dir_all(data_dir.as_os_str()).unwrap();
                let file_path = Path::new(data_dir.as_os_str())
                    .join(file_name.clone())
                    .into_os_string();
                writers.insert(
                    file_name.clone(),
                    FileWriter::new(file_path.as_os_str().to_str().unwrap()),
                );
            }

            let s = serde_json::to_string(&msg).unwrap();

            if let Some(writer) = writers.get_mut(&file_name) {
                writer.write(&s);
            }
            // // copy to redis
            // if let Some(ref tx_redis) = tx_redis {
            //     tx_redis.send(msg).unwrap();
            // }
        }
        for mut writer in writers {
            writer.1.close();
        }
    })
}

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

fn create_redis_writer_thread(rx: Receiver<Message>, redis_url: String) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut redis_conn = connect_redis(&redis_url).unwrap();
        for msg in rx {
            let msg_type = msg.msg_type;
            let s = serde_json::to_string(&msg).unwrap();
            let topic = format!("carbonbot:{}", msg_type);
            if let Err(err) = redis_conn.publish::<&str, String, i64>(&topic, s) {
                error!("{}", err);
                return;
            }
        }
    })
}


fn create_nanomsg_writer_thread(
    rx: Receiver<Message>,
    tx_redis: Option<Sender<Message>>,
    exchange: &'static str,
    market_type: MarketType,
    msg_type: MessageType,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut writers: HashMap<String, Socket> = HashMap::new();
        for msg in rx {
            let file_name = format!("{}.{}.{}", msg.exchange, msg.market_type, msg.msg_type);
            if !writers.contains_key(&file_name) {

                let ipc_exchange_market_type_msg_type = format!(
                    "ipc:///tmp/{}/{}/{}.ipc",
                    msg.exchange,
                    msg.market_type,
                    msg.msg_type
                );
                let topic = String::from("");
                let mut socket = Socket::new(Protocol::Sub).unwrap();
                let setopt = socket.subscribe(topic.as_ref());
                let mut endpoint = socket.connect(ipc_exchange_market_type_msg_type.as_str()).unwrap();

                writers.insert(
                    file_name.clone(),
                    socket,
                );
            }

            let s = serde_json::to_string(&msg).unwrap();

            if let Some(nanomsg_writer) = writers.get_mut(&file_name) {
                match msg_type {
                    MessageType::BBO => {
                        let received_at = 1651122265862;
                        let bbo_msg = tokio::task::spawn_blocking(|| parse_bbo(exchange, MarketType::Spot, &(msg as Message).json, Some(received_at)).unwrap()).await.unwrap();
                        // encode
                        nanomsg_writer.write(s.as_bytes());
                    }
                    MessageType::Trade => {
                        let trade = tokio::task::spawn_blocking(|| parse_trade(exchange, MarketType::Spot, &(msg as Message).json).unwrap()).await.unwrap();
                        let trade = &trade[0];
                        // encode
                        nanomsg_writer.write(s.as_bytes());
                    }
                    MessageType::L2Event => {
                        let orderbook = tokio::task::spawn_blocking(|| parse_l2("binance", MarketType::Spot, &(msg as Message).json, None).unwrap()).await.unwrap();
                        let orderbook = &orderbook[0];
                        // encode
                        nanomsg_writer.write(s.as_bytes());
                    }
                    _ => panic!("Not implemented"),
                };
            }

            // copy to redis
            if let Some(ref tx_redis) = tx_redis {
                tx_redis.send(msg).unwrap();
            }
        }
    })
}

#[allow(clippy::unnecessary_unwrap)]
pub fn create_writer_threads(
    rx: Receiver<Message>,
    data_dir: Option<String>,
    redis_url: Option<String>,
    data_deal_type: &str,
    exchange: &'static str,
    market_type: MarketType,
    msg_type: MessageType,
) -> Vec<JoinHandle<()>> {
    let mut threads = Vec::new();
    if data_dir.is_none() && redis_url.is_none() {
        error!("Both DATA_DIR and REDIS_URL are not set");
        return threads;
    }

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

    if data_deal_type == "1" { // write file and nanomsg
        // channel for nanomsg
        let (tx_redis, rx_redis) = std::sync::mpsc::channel::<Message>();
        threads.push(create_nanomsg_writer_thread(rx, Some(tx_redis), exchange, market_type, msg_type));
        threads.push(create_file_writer_thread(rx_redis, data_dir.unwrap()));
    } else if data_deal_type == "2" { // nanomsg
        threads.push(create_nanomsg_writer_thread(rx, None, exchange, market_type, msg_type))
    } else if data_deal_type == "3" { // file
        threads.push(create_file_writer_thread(rx, data_dir.unwrap()))
    }

    threads
}