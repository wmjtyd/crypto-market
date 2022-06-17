use phf::phf_map;

pub static EXANGE: phf::Map<&'static str, u8> = phf_map! {
    "FTX" => 2,
    "BINANCE" => 3,
};

pub static SYMBLE: phf::Map<&'static str, u8> = phf_map! {
    "BTC/USDT" => 1,
    "BTC/USD" => 2,
    "USDT/USD" => 3,
};

pub static INFOTYPE: phf::Map<&'static str, u8> = phf_map! {
    "asks" => 1,
    "bids" => 2,
};
