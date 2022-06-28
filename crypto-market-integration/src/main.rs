use crypto_market_integration::{crawl_other, create_writer_threads};
use clap::clap_app;
use crypto_crawler::*;
use crypto_market_type::MarketType;
use crypto_msg_type::MessageType;
use log::*;
use redis::IntoConnectionInfo;
use std::{env, str::FromStr, sync::Arc};

pub async fn crawl(
    exchange: &'static str,
    market_type: MarketType,
    msg_type: MessageType,
    data_dir: Option<String>,
    redis_url: Option<String>,
    symbols: Option<&[String]>,
    period: String,
) {
    // if data_dir.is_none() && redis_url.is_none() {
    //     error!("Both DATA_DIR and REDIS_URL are not set");
    //     return;
    // }
    let (tx, rx) = std::sync::mpsc::channel::<Message>();

    let period = Arc::new(period);

    let period_arc = period.clone();
    tokio::task::spawn(async move {
        let writer_threads = create_writer_threads(
            rx,
            data_dir,
            redis_url,
            exchange,
            market_type,
            msg_type,
            period_arc,
        );
        futures::future::join_all(writer_threads.into_iter()).await;
    });

    if msg_type == MessageType::Candlestick {
        let mut symbol_interval_list = Vec::new();
        if let Some(arry) = symbols {
            let period = format!("{}", period);
            let period = u64::from_str(&period).unwrap();
            for symol in arry {
                symbol_interval_list.push((symol.to_string(), period as usize));
            }
        }
        let symbol_interval_list: &[(String, usize)] = &symbol_interval_list;
        let symbol_interval_list = Some(symbol_interval_list);
        crawl_candlestick(exchange, market_type, symbol_interval_list, tx).await;
    } else if msg_type == MessageType::OpenInterest {
        tokio::task::spawn_blocking(move || crawl_open_interest(exchange, market_type, tx))
            .await
            .unwrap();
    } else if msg_type == MessageType::Other {
        crawl_other(exchange, market_type, tx).await;
    } else {
        match msg_type {
            MessageType::BBO => {
                crawl_bbo(exchange, market_type, symbols, tx).await;
            }
            MessageType::Trade => {
                crawl_trade(exchange, market_type, symbols, tx).await;
            }
            MessageType::L2Event => {
                crawl_l2_event(exchange, market_type, symbols, tx).await;
            }
            MessageType::L3Event => {
                crawl_l3_event(exchange, market_type, symbols, tx).await;
            }
            MessageType::L2Snapshot => {
                let symbols = if let Some(symbols) = symbols {
                    symbols.to_vec()
                } else {
                    vec![]
                };
                tokio::task::spawn_blocking(move || {
                    let symbols_local = symbols;
                    crawl_l2_snapshot(exchange, market_type, Some(&symbols_local), tx)
                });
            }
            MessageType::L2TopK => {
                crawl_l2_topk(exchange, market_type, symbols, tx).await;
            }
            MessageType::L3Snapshot => {
                let symbols = if let Some(symbols) = symbols {
                    symbols.to_vec()
                } else {
                    vec![]
                };
                tokio::task::spawn_blocking(move || {
                    let symbols_local = symbols;
                    crawl_l3_snapshot(exchange, market_type, Some(&symbols_local), tx)
                });
            }
            MessageType::Ticker => {
                crawl_ticker(exchange, market_type, symbols, tx).await;
            }
            MessageType::FundingRate => {
                crawl_funding_rate(exchange, market_type, symbols, tx).await
            }
            _ => panic!("Not implemented"),
        };
    }
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    env_logger::init();

    let matches: clap::ArgMatches = clap_app!(quic =>
            (about: "use save file")
            (@arg EXCHANGE:     +required "exchange")
            (@arg MARKET_TYPE:  +required "market_type")
            (@arg MSG_TYPE:     +required "msg_type")
            (@arg PERIOD: "period")
            (@arg COMMA_SEPERATED_SYMBOLS: -c --comma_seperated_symbols +use_delimiter "comma_seperated_symbols")
    )
    .get_matches();

    // let args = vec!["carbonbot".to_string(), "binance".to_string(), "spot".to_string(),
    //                               "l2_topk".to_string(),  "2".to_string(),  "BTCUSDT".to_string()];

    let exchange = matches.value_of("EXCHANGE").unwrap().to_string();
    let exchange: &'static str = Box::leak(exchange.clone().into_boxed_str());

    let market_type_str = matches.value_of("MARKET_TYPE").unwrap();
    let market_type = MarketType::from_str(market_type_str);
    if market_type.is_err() {
        println!("Unknown market type: {}", market_type_str);
        return;
    }
    let market_type = market_type.unwrap();

    let msg_type_str = matches.value_of("MSG_TYPE").unwrap();
    let msg_type = MessageType::from_str(msg_type_str);
    if msg_type.is_err() {
        println!("Unknown msg type: {}", msg_type_str);
        return;
    }
    let msg_type = msg_type.unwrap();


    let data_dir = if std::env::var("DATA_DIR").is_err() {
        info!("The DATA_DIR environment variable does not exist");
        None
    } else {
        let url = std::env::var("DATA_DIR").unwrap();
        Some(url)
    };

    let redis_url = if std::env::var("REDIS_URL").is_err() {
        info!("The REDIS_URL environment variable does not exist");
        None
    } else {
        let url = std::env::var("REDIS_URL").unwrap();
        Some(url)
    };

    let period = matches.value_of("PERIOD").unwrap_or("");

    let specified_symbols = if let Some(v) = matches.values_of("COMMA_SEPERATED_SYMBOLS") {
        v.collect::<Vec<&str>>()
            .iter()
            .map(|v| v.to_string())
            .collect()
    } else {
        fetch_symbols_retry(exchange, market_type)
    };

    // let mut specified_symbols = Vec::new();
    // specified_symbols.push(args[5].as_str().to_string());

    // if data_dir.is_none() && redis_url.is_none() {
    //     panic!("The environment variable DATA_DIR and REDIS_URL are not set, at least one of them should be set");
    // }

    let pid = std::process::id();
    // write pid to file
    {
        let mut dir = std::env::temp_dir()
            .join("carbonbot-pids")
            .join(msg_type_str);
        let _ = std::fs::create_dir_all(&dir);
        dir.push(format!("{}.{}.{}", exchange, market_type_str, msg_type_str));
        std::fs::write(dir.as_path(), pid.to_string()).expect("Unable to write pid to file");
    }
    crawl(
        exchange,
        market_type,
        msg_type,
        data_dir,
        redis_url,
        Some(&specified_symbols),
        period.to_string(),
    )
    .await;
}
