// pub(super) mod file_writer;

use concat_string::concat_string;
use nanomsg::{Protocol, Socket};
use std::io::Read;
use tracing::error;
use wmjtyd_libstock::file::writer::{DataEntry, WriteError};

// pub use file_writer::FileWriter;
pub use wmjtyd_libstock::file::writer::DataWriter;

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
    
        let mut socket =
            Socket::new(Protocol::Sub).map_err(RecorderWriterError::SocketCreationFailed)?;
        socket
            .subscribe(b"")
            .map_err(RecorderWriterError::SocketCreationFailed)?;
        let mut endpoint = socket
            .connect(&ipc_exchange_market_type_msg_type)
            .map_err(RecorderWriterError::SocketConnectionFailed)?;

        tokio::task::spawn_blocking(move || loop {
            // 数据 payload
            let mut payload = Vec::<u8>::new();
            let read_result = socket.read_to_end(&mut payload);
            match read_result {
                Ok(_) => {
                    let result = data_writer.add(DataEntry {
                        filename: ipc.to_string(),
                        data: payload,
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
        }).await?;

        endpoint
            .shutdown()
            .map_err(RecorderWriterError::SocketShutdownFailed)?;

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
