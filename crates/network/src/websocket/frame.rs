// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Observes complete inbound frames before the backend assembles fragmented messages.

use std::{
    io::{self, Cursor, IoSlice},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll},
};

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_tungstenite::tungstenite::protocol::frame::FrameHeader;

const MAX_FRAME_HEADER_BYTES: usize = 14;
const UPGRADE_TERMINATOR: &[u8; 4] = b"\r\n\r\n";

#[derive(Debug)]
pub(super) struct FrameStream<S> {
    inner: S,
    observer: Option<FrameObserver>,
}

impl<S> FrameStream<S> {
    pub(super) fn new(inner: S, activity: Option<Arc<AtomicBool>>) -> Self {
        Self {
            inner,
            observer: activity.map(|activity| FrameObserver {
                activity,
                upgrade_matched: 0,
                header: [0; MAX_FRAME_HEADER_BYTES],
                header_len: 0,
                payload_remaining: 0,
                invalid: false,
            }),
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for FrameStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let start = buf.filled().len();
        let result = Pin::new(&mut this.inner).poll_read(cx, buf);
        if matches!(result, Poll::Ready(Ok(())))
            && let Some(observer) = &mut this.observer
        {
            observer.record(&buf.filled()[start..]);
        }

        result
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for FrameStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write_vectored(cx, bufs)
    }
}

#[derive(Debug)]
struct FrameObserver {
    activity: Arc<AtomicBool>,
    upgrade_matched: usize,
    header: [u8; MAX_FRAME_HEADER_BYTES],
    header_len: usize,
    payload_remaining: u64,
    invalid: bool,
}

impl FrameObserver {
    fn record(&mut self, mut bytes: &[u8]) {
        while self.upgrade_matched < UPGRADE_TERMINATOR.len() {
            let Some((&byte, remaining)) = bytes.split_first() else {
                return;
            };

            bytes = remaining;

            self.upgrade_matched = if byte == UPGRADE_TERMINATOR[self.upgrade_matched] {
                self.upgrade_matched + 1
            } else {
                usize::from(byte == b'\r')
            };
        }

        while !bytes.is_empty() && !self.invalid {
            if self.payload_remaining == 0 && !self.read_header(&mut bytes) {
                return;
            }

            let consumed = self.payload_remaining.min(bytes.len() as u64) as usize;
            self.payload_remaining -= consumed as u64;
            bytes = &bytes[consumed..];

            if self.payload_remaining == 0 {
                self.activity.store(true, Ordering::Relaxed);
            }
        }
    }

    fn read_header(&mut self, bytes: &mut &[u8]) -> bool {
        if self.header_len == 0 {
            let mut cursor = Cursor::new(*bytes);
            match FrameHeader::parse(&mut cursor) {
                Ok(Some((_, length))) => {
                    *bytes = &bytes[cursor.position() as usize..];
                    self.payload_remaining = length;
                    return true;
                }
                Ok(None) => {
                    self.header[..bytes.len()].copy_from_slice(bytes);
                    self.header_len = bytes.len();
                    *bytes = &[];
                    return false;
                }
                Err(_) => {
                    self.invalid = true;
                    return false;
                }
            }
        }

        while let Some((&byte, remaining)) = bytes.split_first() {
            *bytes = remaining;
            self.header[self.header_len] = byte;
            self.header_len += 1;
            let mut cursor = Cursor::new(&self.header[..self.header_len]);
            match FrameHeader::parse(&mut cursor) {
                Ok(Some((_, length))) => {
                    self.header_len = 0;
                    self.payload_remaining = length;
                    return true;
                }
                Ok(None) => {}
                Err(_) => {
                    self.invalid = true;
                    return false;
                }
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use nautilus_cryptography::providers::install_cryptographic_provider;
    use rcgen::generate_simple_self_signed;
    use rstest::rstest;
    use rustls::{ClientConfig, RootCertStore, ServerConfig, pki_types::PrivatePkcs8KeyDer};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_rustls::TlsAcceptor;
    use tokio_tungstenite::tungstenite::{
        client::IntoClientRequest,
        protocol::frame::{
            FrameHeader,
            coding::{Data, OpCode},
        },
        stream::Mode,
    };

    use super::FrameStream;

    #[rstest]
    #[case(0, 1)]
    #[case(125, 1)]
    #[case(126, 1)]
    #[case(65_536, 1)]
    #[case(126, 7)]
    #[case(65_536, 7)]
    #[tokio::test]
    async fn test_frame_activity_requires_complete_frame(
        #[case] payload_len: usize,
        #[case] chunk_len: usize,
    ) {
        let mut wire = b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\n\r\n".to_vec();

        let header = FrameHeader {
            is_final: false,
            opcode: OpCode::Data(Data::Text),
            ..FrameHeader::default()
        };

        header.format(payload_len as u64, &mut wire).unwrap();
        wire.extend(std::iter::repeat_n(b'x', payload_len));
        let activity = Arc::new(AtomicBool::new(false));
        let mut stream = FrameStream::new(wire.as_slice(), Some(Arc::clone(&activity)));
        let mut received = vec![0; wire.len()];

        for offset in (0..wire.len()).step_by(chunk_len) {
            let end = (offset + chunk_len).min(wire.len());
            stream.read_exact(&mut received[offset..end]).await.unwrap();
            assert_eq!(activity.swap(false, Ordering::Relaxed), end == wire.len());
        }

        assert_eq!(received, wire);
    }

    #[tokio::test]
    async fn test_disabled_observation_preserves_bytes() {
        let bytes = b"arbitrary stream contents";
        let mut stream = FrameStream::new(bytes.as_slice(), None);
        let mut received = Vec::new();
        stream.read_to_end(&mut received).await.unwrap();

        assert_eq!(received, bytes);
    }

    #[tokio::test]
    async fn test_frame_activity_preserves_coalesced_partial_header() {
        let upgrade = b"HTTP/1.1 101 Switching Protocols\r\n\r\n";
        let mut wire = upgrade.to_vec();
        wire.extend_from_slice(b"\x01\x01a\x00\x01b");
        let activity = Arc::new(AtomicBool::new(false));
        let mut stream = FrameStream::new(wire.as_slice(), Some(Arc::clone(&activity)));
        let mut received = vec![0; wire.len()];
        let split = upgrade.len() + 4;
        stream.read_exact(&mut received[..split]).await.unwrap();
        assert!(activity.swap(false, Ordering::Relaxed));
        stream
            .read_exact(&mut received[split..=split])
            .await
            .unwrap();
        assert!(!activity.swap(false, Ordering::Relaxed));
        stream.read_exact(&mut received[split + 1..]).await.unwrap();

        assert_eq!(received, wire);
        assert!(activity.load(Ordering::Relaxed));
    }

    #[rstest]
    #[case(1)]
    #[case(2)]
    #[tokio::test]
    async fn test_invalid_header_preserves_transport_bytes(#[case] chunk_len: usize) {
        let wire = b"HTTP/1.1 101 Switching Protocols\r\n\r\n\x83\x00";
        let activity = Arc::new(AtomicBool::new(false));
        let mut stream = FrameStream::new(wire.as_slice(), Some(Arc::clone(&activity)));
        let mut received = vec![0; wire.len()];

        for offset in (0..wire.len()).step_by(chunk_len) {
            let end = (offset + chunk_len).min(wire.len());
            stream.read_exact(&mut received[offset..end]).await.unwrap();
        }

        assert_eq!(received, wire);
        assert!(!activity.load(Ordering::Relaxed));
    }

    #[tokio::test]
    async fn test_frame_activity_observes_decrypted_tls_bytes() {
        install_cryptographic_provider();
        let generated = generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        let cert = generated.cert.der().clone();
        let key = PrivatePkcs8KeyDer::from(generated.signing_key.serialize_der());
        let config = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert.clone()], key.into())
            .unwrap();
        let mut roots = RootCertStore::empty();
        roots.add(cert).unwrap();

        let connector = Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        );
        let (client_io, server_io) = tokio::io::duplex(1024);
        let wire = b"HTTP/1.1 101 Switching Protocols\r\n\r\n\x01\x03abc";

        let server = tokio::spawn(async move {
            let mut stream = TlsAcceptor::from(Arc::new(config))
                .accept(server_io)
                .await
                .unwrap();
            stream.write_all(wire).await.unwrap();
            stream.shutdown().await.unwrap();
        });

        let request = "wss://localhost/".into_client_request().unwrap();
        let tls = crate::tls::tcp_tls(&request, Mode::Tls, client_io, Some(connector))
            .await
            .unwrap();
        let activity = Arc::new(AtomicBool::new(false));
        let mut stream = FrameStream::new(tls, Some(Arc::clone(&activity)));
        let mut received = Vec::new();
        stream.read_to_end(&mut received).await.unwrap();
        server.await.unwrap();

        assert_eq!(received, wire);
        assert!(activity.load(Ordering::Relaxed));
    }
}
