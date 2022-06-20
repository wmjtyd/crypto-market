use crypto_market_recorder::create_writer_threads;

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    env_logger::init();

    // let args: Vec<String> = env::args().collect();
    // if args.len() != 4 && args.len() != 5 {
    //     println!("Usage: carbonbot <exchange> <market_type> <msg_type> <data_deal_type> [comma_seperated_symbols]");
    //     return;
    // }

    let args = vec!["/".to_string(),"carbonbot".to_string(), "binance".to_string(), "spot".to_string(),  
                                  "l2_topk".to_string(),  "2".to_string(),  "BTCUSDT".to_string()];

    let exchange: &'static str = Box::leak(args[1].clone().into_boxed_str());
    
    let venue = &args[1];
    let symbol = &args[2];
    let ipc_path = &args[3];

    let threads = create_writer_threads(
        exchange,
        venue, 
        symbol,
        ipc_path);

    futures::future::join_all(threads.into_iter()).await;
}
