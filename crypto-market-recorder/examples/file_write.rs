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
    if let Some(read_data) = ReadData::new("name".to_string(), "ydf".to_string(), 0) {
        for data in read_data {
            println!("{:?}", data);
        }
    }

}