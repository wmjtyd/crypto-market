use serde::Deserialize;
use tokio::sync::mpsc::{Receiver, Sender};

pub type ChannelAction = (usize, SocketAction);
pub type SenderAction = Sender<ChannelAction>;
pub type ReceiverAction = Receiver<ChannelAction>;

pub type TxJSON = Sender<String>;
pub type RxJSON = Receiver<String>;

pub type SignalName = String;

#[derive(Debug)]
pub enum SocketAction {
    Subscribe(SignalName, TxJSON),
    Unsubscribe(Option<SignalName>),
}

#[derive(Deserialize)]
pub struct Params {
    pub exchange: String,
    pub market_type: String,
    pub msg_type: String,
    pub symbols: String,
    pub period: Option<String>,
    pub elapse: Option<i64>,
}

#[derive(Deserialize)]
pub struct Action {
    pub action: String,
    pub args: Vec<MarketDataArg>,
}

#[derive(Debug, Deserialize, Hash, PartialEq, Eq, Clone)]
pub struct MarketDataArg {
    pub exchange: String,
    #[serde(rename = "marketType")]
    pub market_type: String,
    pub symbol: String,
    #[serde(rename = "messageType")]
    pub msg_type: String,
}

impl ToString for MarketDataArg {
    fn to_string(&self) -> String {
        let mut msg_type = self.msg_type.split("@");
        let msg_name = msg_type.next().unwrap_or(&self.msg_type);
        let msg_period = if let Some(period) = msg_type.next() {
            format!("_{period}")
        } else {
            String::new()
        };

        format!(
            "{}_{}_{}_{}{}",
            self.exchange, self.market_type, msg_name, self.symbol, msg_period
        )
    }
}
