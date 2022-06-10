pub(super) mod file_writer;

use crypto_crawler::*;
use crypto_msg_parser::{parse_l2, parse_trade, parse_bbo, OrderBookMsg, parse_l2_topk};
use log::*;
use redis::{self, Commands};
use std::{
    collections::HashMap,
    path::Path,
    sync::mpsc::{Receiver, Sender},
    thread::JoinHandle, time::SystemTime,
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
                        let orderbook = tokio::task::spawn_blocking(|| parse_l2(exchange, MarketType::Spot, &(msg as Message).json, None).unwrap()).await.unwrap();
                        let orderbook = &orderbook[0];
                        // encode
                        nanomsg_writer.write(s.as_bytes());
                    }
                    MessageType::L2TopK => {
                        let received_at = msg.received_at as i64;
                        let orderbook = tokio::task::spawn_blocking(move || parse_l2_topk(exchange, MarketType::Spot, &(msg as Message).json, Some(received_at)).unwrap()).await.unwrap();
                        let orderbook = &orderbook[0];
                        // encode
                        let order_book_bytes = encode_orderbook(*orderbook); 
                        // send
                        nanomsg_writer.write(&order_book_bytes);
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


pub fn long_to_hex(num:i64) -> String {
    let num_hex = format!("{:x}", num); // to hex
    let mut num_hex_len = num_hex.len() / 2;
    if (num_hex_len * 2 < num_hex.len()) {
        num_hex_len = (num_hex_len + 1);
    }
    let pad_len = num_hex_len * 2;
    let long_hex = format!("{0:0>pad_len$}", num_hex, pad_len=pad_len);
    long_hex
 }


 fn hex_to_byte(mut hex: String) -> Vec<u8>{
    hex = str::replace(&hex, " ", "");
    let mut bytes: Vec<u8> = Vec::new();

    if hex.len() % 2 == 1 {
        return bytes;
    }

    let mut hex_split: Vec<String> = Vec::new();
    for i in 0..(hex.len()/2) {
        let str=  &hex[i*2..i*2+2];
        hex_split.push(str.to_string());
    }

    for i in hex_split.iter() {
        let num = u8::from_str_radix(i, 16);
        match num {
            Ok(t) => bytes.push(t),
            Err(_err) => break
        }
    }

    bytes
}

fn encode_num_to_bytes(mut value: String) -> [u8;5] {
    let mut result: [u8;5] = [0;5];
    let mut e = 0;

    if value.find("E-") != Some(0) {
        let split: Vec<&str> = value.split("E-").collect();
        let a = split[1];
        e = a.parse().unwrap();
        value = split[0].to_string();
    }

    result[4] = match value.find(".") {
        Some(_index) => value.len() - _index - 1 + e,
        None => 0
    } as u8;

    value = value.replace(".", "");
    let hex_byte = hex_to_byte((long_to_hex(value.parse().unwrap())));
    let length = hex_byte.len();
    if hex_byte.len() > 0 {
        result[3] = *hex_byte.get(length-1).unwrap();
        if hex_byte.len() > 1 {
            result[2] = *hex_byte.get(length-2).unwrap();
            if hex_byte.len() > 2 {
                result[1] = *hex_byte.get(length-3).unwrap();
                if hex_byte.len() > 3 {
                    result[0] = *hex_byte.get(length-4).unwrap();
                }
            }
        }
    }

    result
}

pub fn encode_orderbook(orderbook: OrderBookMsg) -> Vec<u8> {
    let mut orderbook_bytes: Vec<u8> = Vec::new();

    let exchange_timestamp = orderbook.timestamp;

    //1、交易所时间戳:6 or 8 字节时间戳 
    let exchange_timestamp_hex = long_to_hex(exchange_timestamp);
    let exchange_timestamp_hex_byte = hex_to_byte(exchange_timestamp_hex);
    orderbook_bytes.extend_from_slice(&exchange_timestamp_hex_byte);

    //2、收到时间戳:6 or 8 字节时间戳 
    let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).expect("get millis error");
    let now_ms = now.as_millis();
    let received_timestamp_hex = long_to_hex(now_ms as i64);
    let received_timestamp_hex_byte = hex_to_byte(received_timestamp_hex);
    orderbook_bytes.extend_from_slice(&received_timestamp_hex_byte);

    //3、EXANGE 1字节信息标识
    let _exchange = EXANGE.get(&orderbook.exchange.as_str()).unwrap();
    orderbook_bytes.push(*_exchange);

    //4、MARKET_TYPE 1字节信息标识
    let _market_type = match orderbook.market_type {
        Spot => 1,
        LinearFuture => 2,
        InverseFuture => 3,
        LinearSwap => 4,
        InverseSwap => 5,
        EuropeanOption => 6,
    };
    orderbook_bytes.push(_market_type);

    //5、MESSAGE_TYPE 1字节信息标识
    let _message_type = match orderbook.msg_type {
        Trade => 1,
        BBO => 2,
        L2TopK => 3,
        L2Snapshot => 4,
        L2Event => 5,
        L3Snapshot => 6,
        L3Event => 7,
        Ticker => 8,
        Candlestick => 9,
        OpenInterest => 10,
        FundingRate => 11,
        Other => 12,
    };
    orderbook_bytes.push(_message_type);

    //6、SYMBLE 2字节信息标识
    let _pair = SYMBLE.get(&orderbook.pair.as_str()).unwrap();
    let _pair_hex = long_to_hex(*_pair as i64);
    if(_pair_hex.len() < 4) {
        let _pair_hex = format!("{:0>4}", _pair_hex);
    }
    let _pair_hex_byte = hex_to_byte(_pair_hex);
    orderbook_bytes.extend_from_slice(&_pair_hex_byte);

    //7、ask、bid
    let mut markets = HashMap::new();
    markets.insert("asks", orderbook.asks);
    markets.insert("bids", orderbook.bids);
    
    for (k, order_list) in markets {

        let _type = INFOTYPE.get(k).unwrap();
        //1）字节信息标识
        orderbook_bytes.push(*_type);

        //2）字节信息体的长度
        let list_len = (order_list.len() * 10) as i64;
        let list_len_hex = long_to_hex(list_len);
        if(list_len_hex.len() < 4) {
            let list_len_hex = format!("{:0>4}", list_len_hex);
        }
        let list_len_hex_byte = hex_to_byte(list_len_hex);
        orderbook_bytes.extend_from_slice(&list_len_hex_byte);

        for order in order_list {

            //3）data(price(5)、quant(5))	10*dataLen	BYTE[10*dataLen] 信息体
            let price = order.price;
            let quantity_base = order.quantity_base;

            let price_bytes = encode_num_to_bytes(price.to_string());
            let quantity_base_bytes = encode_num_to_bytes(quantity_base.to_string());
            orderbook_bytes.extend_from_slice(&price_bytes);
            orderbook_bytes.extend_from_slice(&quantity_base_bytes);
        }
    }

    // let compressed = compress_to_vec(&bytes, 6);
    // println!("compressed from {} to {}", data.len(), compressed.len());
    orderbook_bytes

}