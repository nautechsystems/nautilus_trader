use pyo3::prelude::*;
use pyo3::{PyObject, Python};
use std::io;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::task;

#[pyclass]
pub struct SocketClient {
    read_task: Option<task::JoinHandle<io::Result<()>>>,
    write_mutex: Arc<Mutex<OwnedWriteHalf>>,
}

impl SocketClient {
    pub async fn connect(url: &str, handler: PyObject) -> io::Result<Self> {
        let stream = TcpStream::connect(url).await?;
        let (read_half, write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let write_mutex = Arc::new(Mutex::new(write_half));

        // keep receiving messages from socket
        // pass them as arguments to handler
        let read_task = Some(task::spawn(async move {
            let mut buf = Vec::new();
            loop {
                // TODO: use "\r\n" delimiter but `read_until`
                // only takes one byte delimiter
                let n = reader.read_until(b'\r', &mut buf).await?;
                if n == 0 {
                    break;
                }
                Python::with_gil(|py| handler.call1(py, (buf.drain(0..n).as_slice(),))).unwrap();
            }
            Ok(())
        }));

        Ok(Self {
            read_task,
            write_mutex,
        })
    }
}

#[pymethods]
impl SocketClient {
    #[staticmethod]
    fn connect_url(url: String, handler: PyObject, py: Python<'_>) -> PyResult<&PyAny> {
        pyo3_asyncio::tokio::future_into_py(py, async move {
            Ok(SocketClient::connect(&url, handler).await.unwrap())
        })
    }

    fn send<'py>(slf: PyRef<'_, Self>, data: Vec<u8>, py: Python<'py>) -> PyResult<&'py PyAny> {
        let write_half = slf.write_mutex.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut write_half = write_half.lock().await;
            write_half.write_all(&data).await?;
            write_half.flush().await?;
            Ok(())
        })
    }

    /// Closing the client aborts the reading task and shuts down the writer half
    ///
    /// # Safety
    /// - The client should not send after being closed
    /// - The client should be dropped after being closed
    fn close<'py>(slf: PyRef<'_, Self>, py: Python<'py>) -> PyResult<&'py PyAny> {
        // cancel reading task
        if let Some(ref handle) = slf.read_task {
            handle.abort();
        }

        // shut down writer
        let write_half = slf.write_mutex.clone();
        pyo3_asyncio::tokio::future_into_py(py, async move {
            let mut write_half = write_half.lock().await;
            write_half.shutdown().await.unwrap();
            Ok(())
        })
    }
}

impl Drop for SocketClient {
    fn drop(&mut self) {
        // cancel reading task
        if let Some(ref handle) = self.read_task {
            handle.abort();
        }

        // writer is automatically dropped along with the struct
    }
}
