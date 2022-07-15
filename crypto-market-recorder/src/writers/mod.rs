// pub(super) mod file_writer;

use concat_string::concat_string;
use tracing::error;
use wmjtyd_libstock::file::writer::{DataEntry, WriteError};

// pub use file_writer::FileWriter;
pub use wmjtyd_libstock::file::writer::DataWriter;

use wmjtyd_libstock::message::zeromq::Zeromq;
use tokio::io::AsyncReadExt;
use wmjtyd_libstock::message::zeromq::Sub;

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

    let task = async move {
        let mut data_writer = DataWriter::new();
        data_writer.start().await?;
        let mut socket = Zeromq::<Sub>::new(&ipc_exchange_market_type_msg_type).await.unwrap();
        socket.subscribe("").await.unwrap();

        let mut buf = [0u8; 8192];
        tokio::task::spawn( async move { loop {
            // 数据 payload
            let size = socket.read(&mut buf).await;
            match size {
                Ok(size) => {
                    let result = data_writer.add(DataEntry {
                        filename: ipc.to_string(),
                        data: buf[..size].to_vec(),
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
        }}).await?;


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

    #[error("failed to shutdown the socket: {0}")]
    SocketShutdownFailed(nanomsg::Error),

    #[error("failed to create a socket: {0}")]
    SocketCreationFailed(nanomsg::Error),

    #[error("failed to connect to a socket: {0}")]
    SocketConnectionFailed(nanomsg::Error),

    #[error("failed to subscribe to a socket: {0}")]
    SocketSubscribeFailed(nanomsg::Error),
}

pub type RecorderWriterResult<T> = Result<T, RecorderWriterError>;
