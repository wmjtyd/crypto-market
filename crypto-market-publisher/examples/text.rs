use std::sync::mpsc::{channel, Sender};

struct BboMsg {
    pub name: String,
}

struct OrderBookMsg {
    pub name: String,
}

// 上面是已经存在在项目里面的结构体。

/// 解码参数和返回值的声明
trait Code<T> {
    fn decode(c: &[u8]) -> T;
}

/// 实现 Code 拥有 解码的能力
macro_rules! add_msg_type {
    ($type:ident, $decode:path) => {
        impl Code<$type> for $type {
            fn decode(c: &[u8]) -> $type {
                $decode(c)
            }
        }
    };
}

/// 解码操作1
fn decode_order_book(c: &[u8]) -> OrderBookMsg {
    OrderBookMsg {
        name: format!("decoder_order_book {}", String::from_utf8_lossy(c)),
    }
}

/// 解码操作2
fn decode_bbo(c: &[u8]) -> BboMsg {
    BboMsg {
        name: format!("decode_bbo {}", String::from_utf8_lossy(c)),
    }
}

add_msg_type!(OrderBookMsg, decode_order_book);
add_msg_type!(BboMsg, decode_bbo);

// 测试结果
#[tokio::main]
async fn main() {
    let (tx, rx) = channel();

    client::<BboMsg, _>(tx);
    // client(msg!(OrderBookMsg), tx);

    let msg = rx.recv().unwrap();
    println!("{}", msg.name);
}

// 模拟需要解码的函数
fn client<C: Code<T>, T>(tx: Sender<T>) {
    tx.send(C::decode(b"data")).unwrap();
}
