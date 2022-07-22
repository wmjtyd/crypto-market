use wmjtyd_libstock::data::bbo::decode_bbo;
use wmjtyd_libstock::file::reader::FileReader;
fn main() {
    let r = FileReader::new("binance_spot_bbo_BTCUSDT".to_string(), 0);
    for i in r.unwrap() {
        println!("{:?}", i);
        let msg = decode_bbo(&i).unwrap();
        println!("{}", msg.exchange);
        println!("{}", msg.ask_price);
        println!("{}", msg.ask_quantity_base);
        println!("{}", msg.bid_price);
        println!("{}", msg.bid_quantity_base);
    }
}
