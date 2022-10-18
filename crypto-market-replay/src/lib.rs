use std::collections::{HashMap, HashSet};

use futures::StreamExt;
use parameter_type::SocketAction;
use serde_json::json;
use tokio::runtime::Handle;
use tokio::sync::mpsc::Sender;
use wmjtyd_libstock::message::traits::{Connect, Subscribe};
use wmjtyd_libstock::message::zeromq::ZeromqSubscriber;

pub mod config;
use config::dynamic::DynamicConfigHandler;

pub mod parameter_type;
use parameter_type::{ReceiverAction, TxJSON};

mod market_data_file;
pub use market_data_file::http_market_data;

pub async fn msg_sub_handle(config_path: String, mut receiver_action: ReceiverAction) {
    // 当前运行时环境
    let handle = Handle::current();

    // 传输订阅数据
    let (data_tx, mut data_rx) = tokio::sync::mpsc::channel::<(String, String,Vec<u8>)>(1024);

    // 配置文件更新时
    let _watcher = DynamicConfigHandler::new(
        &config_path,
        handle,
        Box::new(move |handle, market_data_arg| {
            let data_tx = data_tx.clone();

            let symbol = market_data_arg.to_string();

            let path = format!("ipc:///tmp/{}.ipc", symbol);

            tracing::debug!("path {path}");

            handle.spawn(async move {
                let mut sub = ZeromqSubscriber::new().expect("sub init error");
                if sub.connect(&path).is_err() || sub.subscribe(b"").is_err() {
                    tracing::error!("sub connect");
                    return;
                }

                while let Some(Ok(data)) = StreamExt::next(&mut sub).await {
                    tracing::debug!("get data {path}");
                    if data_tx
                        .send((symbol.to_owned(), market_data_arg.msg_type.to_owned(), data))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            })
        }),
    )
    .watcher()
    .expect("Failed to create watcher");

    // 用以查询用户信息
    let mut user_list: HashMap<usize, (TxJSON, HashSet<String>)> = HashMap::new();
    // 用以推送数据（优化推送速度）
    let mut sub_list: HashMap<String, HashMap<usize, TxJSON>> = HashMap::new();

    loop {
        tokio::select! {
            // 接收用户动作处理
            data = receiver_action.recv() =>{
                if let Some((id, action)) = data {
                    tracing::debug!("action {:?}", action);
                    let _ = action_handle(id, action, &mut user_list, &mut sub_list).await;
                    continue;
                }
            }
            // 推送数据
            data = data_rx.recv() =>{
                if let Some((symbol, msg_type, data)) = data {
                    let _ = data_hander(symbol, msg_type, data, &mut sub_list).await;
                    continue;
                }
            }
        };
        break;
    }
    tracing::debug!("loop end")
}

async fn action_handle(
    id: usize,
    action: SocketAction,
    user_list: &mut HashMap<usize, (TxJSON, HashSet<String>)>,
    sub_list: &mut HashMap<String, HashMap<usize, TxJSON>>,
) -> Option<()> {
    tracing::debug!("sub_list {:?}", sub_list);
    let state = match action {
        // 订阅处理
        SocketAction::Subscribe(signal_name, tx_json) => {
            action_handle_subscribe(id, &signal_name, tx_json.clone(), user_list, sub_list)
                .is_some()
        }
        // 取消订阅处理
        SocketAction::Unsubscribe(signal_name) => {
            action_handle_unsubscribe(id, signal_name, user_list, sub_list).is_some()
        }
    };
    let (tx_json, _) = user_list.get_mut(&id)?;

    // 响应
    if state {
        let msg = r#"{"state": "success","code": 200,"message": ""}"#.to_string();
        let _ = tx_json.send(msg).await;
    } else {
        let msg = r#"{"state": "failure","code": 500,"message": "server error"}"#.to_string();
        let _ = tx_json.send(msg).await;
    }
    Some(())
}

fn action_handle_subscribe(
    id: usize,
    symbol: &str,
    tx_json: Sender<String>,
    user_list: &mut HashMap<usize, (TxJSON, HashSet<String>)>,
    sub_list: &mut HashMap<String, HashMap<usize, TxJSON>>,
) -> Option<()> {
    tracing::debug!("{:?}", user_list);
    if let Some((_, user_info)) = user_list.get_mut(&id) {
        if !user_info.contains(symbol) {
            return None;
        }
        user_info.insert(symbol.to_owned());
    } else {
        let market_data_list = HashSet::from([symbol.to_owned()]);
        let _ = user_list.insert(id, (tx_json.clone(), market_data_list));
    }
    let user_session = sub_list.get_mut(symbol)?;
    let _ = user_session.insert(id, tx_json.clone());
    Some(())
}
fn action_handle_unsubscribe(
    id: usize,
    signal_name: Option<String>,
    user_list: &mut HashMap<usize, (TxJSON, HashSet<String>)>,
    sub_list: &mut HashMap<String, HashMap<usize, TxJSON>>,
) -> Option<()> {
    tracing::debug!("{:?}", user_list);

    if let Some(signal_name) = signal_name {
        sub_list.remove(&signal_name)?;
        return Some(());
    }

    for symbol in user_list.get(&id)?.1.iter() {
        let _ = sub_list.remove(symbol);
    }
    user_list.remove(&id)?;

    Some(())
}

pub async fn data_hander(
    symbol: String,
    msg_type: String,
    data: Vec<u8>,
    sub_list: &mut HashMap<String, HashMap<usize, TxJSON>>,
) -> anyhow::Result<()> {
    let data_base64 = base64::encode(data);

    let json = json!({
        "messageType": msg_type,
        "payload": data_base64
    }).to_string();

    tracing::debug!("{:?}", sub_list);

    if let Some(session_list) = sub_list.get_mut(&symbol) {
        for (_, tx_json) in session_list.iter_mut() {
            let _ = tx_json.send(json.to_owned()).await;
        }
        return Ok(());
    } else {
        let _ = sub_list.insert(symbol, HashMap::new());
        return Ok(());
    }
}
