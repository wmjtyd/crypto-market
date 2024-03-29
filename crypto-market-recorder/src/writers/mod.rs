// pub(super) mod file_writer;

use concat_string::concat_string;
use tracing::error;
// pub use file_writer::FileWriter;
pub use wmjtyd_libstock::file::writer::DataWriter;
use wmjtyd_libstock::file::writer::{DataEntry, WriteError};
use wmjtyd_libstock::message::traits::{Connect, StreamExt, Subscribe};
use wmjtyd_libstock::message::zeromq::ZeromqSubscriber;

pub trait Writer {
    fn write(&mut self, s: &str);
    fn close(&mut self);
}

pub async fn create_write_file_thread(
    _exchange: &str,
    _market_type: &str,
    _msg_type: &str,
    ipc: String,
) -> RecorderWriterResult<()> {
    // concat_string for better performance
    let ipc_exchange_market_type_msg_type = concat_string!("ipc:///tmp/", ipc, ".ipc");

    tracing::debug!("{}", ipc_exchange_market_type_msg_type);

    let task = async move {
        let mut data_writer = DataWriter::new();
        data_writer.start().await?;

        let mut subscriber = ZeromqSubscriber::new().expect("init error!");
        if subscriber
            .connect(&ipc_exchange_market_type_msg_type)
            .is_err()
            || subscriber.subscribe(b"").is_err()
        {
            // 这里有问题
            return Ok(());
        }

        tracing::info!("start read data");

        tokio::task::spawn(async move {
            loop {
                tracing::debug!("await data");
                // 数据 payload
                let message = StreamExt::next(&mut subscriber).await;


                if message.is_none() {
                    break;
                }

                match message.unwrap() {
                    Ok(message) => {
                        tracing::debug!("{:?}", message);
                        let result = data_writer.add(DataEntry {
                            filename: ipc.to_string(),
                            data: message,
                        });

                        if let Err(e) = result {
                            error!("Failed to add data: {e}");
                            break;
                        }
                    }
                    Err(err) => {
                        error!("Client failed to receive payload: {err}");
                        break;
                    }
                }
            }
            tracing::info!("stop read");
        })
        .await?;

        Ok::<(), RecorderWriterError>(())
    };

    tokio::task::spawn(async {
        let response = task.await;

        if let Err(err) = response {
            error!("failed to run create_write_file task: {err}");
        }
    })
    .await?;

    Ok(())
}

#[derive(thiserror::Error, Debug)]
pub enum RecorderWriterError {
    #[error("writer error: {0}")]
    WriterError(#[from] WriteError),

    #[error("failed to create the writer thread: {0}")]
    CreateThreadFailed(#[from] tokio::task::JoinError),
}

pub type RecorderWriterResult<T> = Result<T, RecorderWriterError>;
