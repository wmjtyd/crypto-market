use crypto_msg_parser::{
    BboMsg,
    TradeMsg,
    OrderBookMsg,
    KlineMsg,
};
use wmjtyd_libstock::data::{
    bbo::decode_bbo,
    trade::decode_trade,
    orderbook::decode_orderbook,
    kline::decode_kline
};

pub struct Msg<T>(pub Option<T>);

pub trait Code<T> {
    fn decode(&self, c: &[u8]) -> T;
    fn clone(&self) -> Msg<T> {
        Msg(None)
    }
}


// 更加直观的更换类型
#[macro_export]
macro_rules! msg {
    ($name:ident) => {
        &Msg::<$name>(None)
    };
}

// 实现 Code 拥有 解码的能力
macro_rules! add_msg_type {
    ($type:ident, $decode:path) => {
        impl Code<$type> for Msg<$type> {
            fn decode(&self, c: &[u8]) -> $type {
                $decode(c).unwrap()
            }
        }
    };
}

add_msg_type!(BboMsg, decode_bbo);
add_msg_type!(TradeMsg, decode_trade);
add_msg_type!(OrderBookMsg, decode_orderbook);
add_msg_type!(KlineMsg, decode_kline);