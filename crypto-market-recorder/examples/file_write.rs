use  crypto_market_recorder::ReadData;

// 案例！
fn main() {

    // 添加数据
    // let mut a = WriteData::new();
    // a.add_order_book("name".to_string(), "ydf".to_string(), vec![104, 101, 108, 108, 111]);
    // a.add_order_book("name".to_string(), "ydf".to_string(), vec![32]);
    // a.add_order_book("name".to_string(), "ydf".to_string(), vec![119, 111, 114, 108, 100]);
    // let c = a.start();
    // c.join().unwrap();


    // 读取文件
    // if let Some(read_data) = ReadData::new("binance_spot_candlestick_BTCUSDT_60".to_string(), 0) {
    //     for data in read_data {
            
    //         let kline = decode_kline(data);
    //         println!("{}", kline.json);
    //         println!("{}", kline.symbol);
    //         println!("{}", kline.close);
    //         println!("{}", kline.high);
    //         println!("{}", kline.open);
    //         println!("{}", kline.msg_type);
    //         println!("{}", kline.timestamp);

    //     }
    // }
    // if let Some(read_data) = ReadData::new("binance_spot_bbo_BTCUSDT".to_string(), 0) {
    //     for data in read_data {
            
    //         let bbo = decode_bbo(data);
    //         println!("json {}", bbo.json);
    //         println!("symol {}", bbo.symbol);
    //         println!("msg_type {}", bbo.msg_type);
    //         println!("timestamp {}", bbo.timestamp);

    //     }
    // }

}