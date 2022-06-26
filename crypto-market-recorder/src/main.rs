use clap::clap_app;
use crypto_market_recorder::create_write_file_thread;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    env_logger::init();

    let matches: clap::ArgMatches = clap_app!(quic =>
            (about: "use save file")
            (@arg EXCHANGE:     +required "exchange")
            (@arg MARKET_TYPE:  +required "market_type")
            (@arg MSG_TYPE:     +required "msg_type")
            (@arg SYMBOL: +required "symbol")
            (@arg PERIOD: "period")
    )
    .get_matches();


    let exchange = matches.value_of("EXCHANGE").unwrap().to_string(); 
    let market_type = matches.value_of("MARKET_TYPE").unwrap();
    let msg_type = matches.value_of("MSG_TYPE").unwrap();
    let symbol = matches.value_of("SYMBOL").unwrap();

    let ipc = if let Some(period) = matches.value_of("PERIOD") {
        format!("{}_{}_{}_{}_{}", exchange, market_type, msg_type, symbol, period)
    } else {
        format!("{}_{}_{}_{}", exchange, market_type, msg_type, symbol)
    };

    create_write_file_thread(
        exchange.to_string(),
        market_type.to_string(),
        msg_type.to_string(),
        ipc.to_string()
    ).await;

}
