use clap::clap_app;
use concat_string::concat_string;
use crypto_market_recorder::create_write_file_thread;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    tracing_subscriber::fmt::init();
    let matches: clap::ArgMatches = clap_app!(quic =>
            (about: "use save file")
            (@arg EXCHANGE:     +required "exchange")
            (@arg MARKET_TYPE:  +required "market_type")
            (@arg MSG_TYPE:     +required "msg_type")
            (@arg SYMBOL: +required "symbol")
            (@arg PERIOD: "period")
    )
    .get_matches();

    let exchange = matches.value_of("EXCHANGE").unwrap();
    let market_type = matches.value_of("MARKET_TYPE").unwrap();
    let msg_type = matches.value_of("MSG_TYPE").unwrap();
    let symbol = matches.value_of("SYMBOL").unwrap();

    let period = if let Some(period) = matches.value_of("PERIOD") {
        concat_string!("_", period)
    } else {
        "".to_string()
    };
    let ipc = concat_string!( exchange, "_", market_type, "_", msg_type, "_", symbol, period);

    create_write_file_thread(exchange, market_type, msg_type, ipc.to_string())
        .await
        .expect("create_write_file_thread failed");
}
