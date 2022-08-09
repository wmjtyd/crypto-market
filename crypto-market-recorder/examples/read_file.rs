use wmjtyd_libstock::file::reader::FileReader;
use wmjtyd_libstock::data::bbo::BboStructure;
use wmjtyd_libstock::data::serializer::StructDeserializer;

fn main() {
    let r = FileReader::new("binance_spot_bbo_BTCUSDT".to_string(), 0);
    for i in r.unwrap() {
        println!("{:?}", i);
        let  msg = BboStructure::deserialize(&mut i.as_slice()).unwrap();
        println!("{}", msg.exchange);
        println!("{}", msg.ask_price);
        println!("{}", msg.ask_quantity_base);
        println!("{}", msg.bid_price);
        println!("{}", msg.bid_quantity_base);

    }
}