use wmjtyd_libstock::file::reader::FileReader;
use wmjtyd_libstock::data::orderbook::OrderbookStructure;
use wmjtyd_libstock::data::serializer::StructDeserializer;

fn main() {
    let r = FileReader::new("binance_spot_l2_topk_BTCUSDT".to_string(), 0);
    for i in r.unwrap() {
        let a = OrderbookStructure::deserialize(&mut i.as_slice()).unwrap();
        println!("{:?}", a);
    }
}