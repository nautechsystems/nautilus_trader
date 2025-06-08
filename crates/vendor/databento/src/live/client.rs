use std::{fmt, net::SocketAddr};

use dbn::{
    decode::dbn::{async_decode_metadata_with_fsm, async_decode_record_ref_with_fsm, fsm::DbnFsm},
    Metadata, RecordRef, VersionUpgradePolicy,
};
use time::Duration;
use tokio::{
    io::{BufReader, ReadHalf, WriteHalf},
    net::{TcpStream, ToSocketAddrs},
};
use tracing::{info, info_span, instrument, warn, Span};

use super::{
    protocol::{self, Protocol},
    ClientBuilder, Subscription, Unset,
};
use crate::ApiKey;

/// The Live client. Used for subscribing to real-time and intraday historical market data.
///
/// Use [`LiveClient::builder()`](Client::builder) to get a type-safe builder for
/// initializing the required parameters for the client.
pub struct Client {
    key: ApiKey,
    dataset: String,
    send_ts_out: bool,
    upgrade_policy: VersionUpgradePolicy,
    heartbeat_interval: Option<Duration>,
    protocol: Protocol<WriteHalf<TcpStream>>,
    peer_addr: SocketAddr,
    sub_counter: u32,
    subscriptions: Vec<Subscription>,
    reader: ReadHalf<TcpStream>,
    fsm: DbnFsm,
    session_id: String,
    span: Span,
}

impl Client {
    /// Creates a new client connected to a Live gateway.
    ///
    /// # Errors
    /// This function returns an error when `key` is invalid or it's unable to connect
    /// and authenticate with the Live gateway.
    /// This function returns an error when `key` or `heartbeat_interval` are invalid,
    /// or it's unable to connect and authenticate with the Live gateway.
    pub async fn connect(
        key: String,
        dataset: String,
        send_ts_out: bool,
        upgrade_policy: VersionUpgradePolicy,
        heartbeat_interval: Option<Duration>,
    ) -> crate::Result<Self> {
        Self::connect_with_addr(
            protocol::determine_gateway(&dataset),
            key,
            dataset,
            send_ts_out,
            upgrade_policy,
            heartbeat_interval,
        )
        .await
    }

    /// Creates a new client connected to the Live gateway at `addr`. This is an advanced method and generally
    /// [`builder()`](Self::builder) or [`connect()`](Self::connect) should be used instead.
    ///
    /// # Errors
    /// This function returns an error when `key` or `heartbeat_interval` are invalid,
    /// or it's unable to connect and authenticate with the Live gateway.
    pub async fn connect_with_addr(
        addr: impl ToSocketAddrs,
        key: String,
        dataset: String,
        send_ts_out: bool,
        upgrade_policy: VersionUpgradePolicy,
        heartbeat_interval: Option<Duration>,
    ) -> crate::Result<Self> {
        let key = ApiKey::new(key)?;
        let stream = TcpStream::connect(&addr).await?;
        let peer_addr = stream.peer_addr()?;
        let (recver, sender) = tokio::io::split(stream);
        let mut recver = BufReader::new(recver);
        let mut protocol = Protocol::new(sender);
        let session_id = protocol
            .authenticate(
                &mut recver,
                &key,
                &dataset,
                send_ts_out,
                heartbeat_interval.map(|i| i.whole_seconds()),
            )
            .await?;
        let span = info_span!("LiveClient", %dataset, session_id);
        Ok(Self {
            key,
            dataset,
            send_ts_out,
            upgrade_policy,
            heartbeat_interval,
            protocol,
            peer_addr,
            reader: recver.into_inner(),
            fsm: DbnFsm::builder()
                .upgrade_policy(upgrade_policy)
                .build()
                // Not setting input version so it's infallible
                .unwrap(),
            session_id,
            span,
            sub_counter: 0,
            subscriptions: Vec::new(),
        })
    }

    /// Returns a type-safe builder for setting the required parameters
    /// for initializing a [`LiveClient`](Client).
    pub fn builder() -> ClientBuilder<Unset, Unset> {
        ClientBuilder::default()
    }

    /// Returns the API key used by the instance of the client.
    pub fn key(&self) -> &str {
        &self.key.0
    }

    /// Returns the dataset the client is configured for.
    pub fn dataset(&self) -> &str {
        &self.dataset
    }

    /// Returns an identifier for the current Live session.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Returns whether client is configured to request that the gateway send `ts_out`
    /// with each message.
    pub fn send_ts_out(&self) -> bool {
        self.send_ts_out
    }

    /// Returns the upgrade policy for decoding DBN from previous versions.
    pub fn upgrade_policy(&self) -> VersionUpgradePolicy {
        self.upgrade_policy
    }

    /// Returns the heartbeat interval override if there is one, otherwise `None`.
    pub fn heartbeat_interval(&self) -> Option<Duration> {
        self.heartbeat_interval
    }

    /// Returns an immutable reference to all subscriptions made with this instance.
    pub fn subscriptions(&self) -> &Vec<Subscription> {
        &self.subscriptions
    }

    /// Returns a mutable reference to all subscriptions made with this instance.
    pub fn subscriptions_mut(&mut self) -> &mut Vec<Subscription> {
        &mut self.subscriptions
    }

    /// Closes the connection with the gateway, ending the session and all subscriptions.
    ///
    /// # Errors
    /// This function returns an error if the shutdown of the TCP stream is unsuccessful, this usually
    /// means the stream is no longer usable.
    pub async fn close(&mut self) -> crate::Result<()> {
        self.protocol.shutdown().await
    }

    /// Attempts to add a new subscription to the session. Note that
    /// an `Ok(())` result from this function does not necessarily indicate that
    /// the subscription succeeded, only that it was sent to the gateway.
    ///
    /// # Errors
    /// This function returns an error if it's unable to communicate with the gateway.
    ///
    /// # Cancel safety
    /// This method is not cancellation safe. If this method is used in a
    /// [`tokio::select!`] statement and another branch completes first, the subscription
    /// may have been partially sent, resulting in the gateway rejecting the
    /// subscription, sending an error, and closing the connection.
    #[instrument(parent = &self.span, skip_all)]
    pub async fn subscribe(&mut self, mut sub: Subscription) -> crate::Result<()> {
        if sub.id.is_none() {
            if self.sub_counter == u32::MAX {
                warn!("Exhausted all subscription IDs");
            } else {
                self.sub_counter += 1;
            }
            sub.id = Some(self.sub_counter);
        }
        self.protocol.subscribe(&sub).await?;
        self.subscriptions.push(sub);
        Ok(())
    }

    /// Instructs the gateway to start sending data, starting the session. Except
    /// in cases of a reconnect, this method should only be called once on a given
    /// instance.
    ///
    /// Returns the DBN metadata associated with this session. This is primarily useful
    /// when saving the data to a file to replay it later.
    ///
    /// # Errors
    /// This function returns an error if it's unable to communicate with the gateway or
    /// there was an error decoding the DBN metadata. It will also return an error if
    /// the session has already been started.
    ///
    /// # Cancel safety
    /// This method is not cancellation safe. If this method is used in a
    /// [`tokio::select!`] statement and another branch completes first, the live
    /// gateway may only receive a partial message, resulting in it sending an error and
    /// closing the connection.
    #[instrument(parent = &self.span, skip_all)]
    pub async fn start(&mut self) -> crate::Result<Metadata> {
        if self.fsm.has_decoded_metadata() {
            return Err(crate::Error::BadArgument {
                param_name: "self".to_owned(),
                desc: "ignored request to start session that has already been started".to_owned(),
            });
        };
        info!("Starting session");
        self.protocol.start_session().await?;
        Ok(async_decode_metadata_with_fsm(&mut self.reader, &mut self.fsm).await?)
    }

    /// Fetches the next record. This method should only be called after the session has
    /// been [started](Self::start).
    ///
    /// Returns `Ok(None)` if the gateway closed the connection and no more records
    /// can be read.
    ///
    /// # Errors
    /// This function returns an error when it's unable to decode the next record
    /// or it's unable to read from the TCP stream. It will also return an error if the
    /// session hasn't been started.
    ///
    /// # Cancel safety
    /// This method is cancel safe. It can be used within a [`tokio::select!`] statement
    /// without the potential for corrupting the input stream.
    #[instrument(parent = &self.span, level = "debug", skip_all)]
    pub async fn next_record(&mut self) -> crate::Result<Option<RecordRef>> {
        if !self.fsm.has_decoded_metadata() {
            return Err(crate::Error::BadArgument {
                param_name: "self".to_owned(),
                desc: "Can't call LiveClient::next_record before starting session".to_owned(),
            });
        };
        Ok(async_decode_record_ref_with_fsm(&mut self.reader, &mut self.fsm).await?)
    }

    /// Closes the current connection, then reopens the connection and authenticates
    /// with the live gateway.
    ///
    /// # Errors
    /// This function returns an error if it's unable to connect to the gateway or
    /// authentication fails.
    ///
    /// # Cancel safety
    /// This method is not cancellation safe. If this method is used in a
    /// [`tokio::select!`] statement and another branch completes first, the reconnect
    /// may be in an invalid intermediate state and the reconnect should be reattempted.
    pub async fn reconnect(&mut self) -> crate::Result<()> {
        info!("Reconnecting");
        if let Err(err) = self.close().await {
            warn!(
                ?err,
                "Failed to close connection before reconnect. Proceeding"
            );
        }
        let stream = TcpStream::connect(self.peer_addr).await?;
        let (recver, sender) = tokio::io::split(stream);
        let mut recver = BufReader::new(recver);
        self.protocol = Protocol::new(sender);
        self.sub_counter = 0;
        self.session_id = self
            .protocol
            .authenticate(
                &mut recver,
                &self.key,
                &self.dataset,
                self.send_ts_out,
                self.heartbeat_interval.map(|i| i.whole_seconds()),
            )
            .await?;
        self.reader = recver.into_inner();
        self.fsm.reset();
        self.span = info_span!("LiveClient", dataset = %self.dataset, session_id = self.session_id);
        Ok(())
    }

    /// Resubscribes to all subscriptions, removing the original `start` time, if any.
    /// Usually performed after a [`reconnect()`](Self::reconnect).
    ///
    /// # Errors
    /// This function returns an error if it fails to send any of the subscriptions to
    /// the gateway.
    pub async fn resubscribe(&mut self) -> crate::Result<()> {
        for sub in self.subscriptions.iter_mut() {
            sub.start = None;
            self.sub_counter = self.sub_counter.max(sub.id.unwrap_or(0));
            self.protocol.subscribe(sub).await?;
        }
        Ok(())
    }
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LiveClient")
            .field("key", &self.key)
            .field("dataset", &self.dataset)
            .field("send_ts_out", &self.send_ts_out)
            .field("upgrade_policy", &self.upgrade_policy)
            .field("heartbeat_interval", &self.heartbeat_interval)
            .field("peer_addr", &self.peer_addr)
            .field("sub_counter", &self.sub_counter)
            .field("subscriptions", &self.subscriptions)
            .field("session_id", &self.session_id)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use std::{ffi::c_char, fmt};

    use dbn::{
        encode::AsyncDbnMetadataEncoder,
        enums::rtype,
        publishers::Dataset,
        record::{HasRType, OhlcvMsg, RecordHeader, TradeMsg, WithTsOut},
        FlagSet, Mbp10Msg, MetadataBuilder, Record, SType, Schema,
    };
    use time::{Duration, OffsetDateTime};
    use tokio::{
        io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
        join,
        net::{TcpListener, TcpStream},
        select,
        sync::mpsc::UnboundedSender,
        task::JoinHandle,
    };
    use tracing::level_filters::LevelFilter;

    use super::*;

    struct MockLsgServer {
        dataset: String,
        send_ts_out: bool,
        listener: TcpListener,
        stream: Option<BufReader<TcpStream>>,
    }

    impl MockLsgServer {
        async fn new(dataset: String, send_ts_out: bool) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            Self {
                dataset,
                send_ts_out,
                listener,
                stream: None,
            }
        }

        async fn accept(&mut self) {
            let stream = self.listener.accept().await.unwrap().0;
            stream.set_nodelay(true).unwrap();
            self.stream = Some(BufReader::new(stream));
        }

        async fn authenticate(&mut self, heartbeat_interval: Option<Duration>) {
            self.accept().await;
            self.send("lsg-test\n").await;
            self.send("cram=t7kNhwj4xqR0QYjzFKtBEG2ec2pXJ4FK\n").await;
            let auth_line = self.read_line().await;
            let auth_start = auth_line.find("auth=").unwrap() + 5;
            let auth_end = auth_line[auth_start..].find('|').unwrap();
            let auth = &auth_line[auth_start..auth_start + auth_end];
            let (auth, bucket) = auth.split_once('-').unwrap();
            assert!(
                auth.chars().all(|c| c.is_ascii_hexdigit()),
                "Expected '{auth}' to be composed of only hex characters"
            );
            assert_eq!(bucket, "iller");
            assert!(auth_line.contains(&format!("dataset={}", self.dataset)));
            assert!(auth_line.contains("encoding=dbn"));
            assert!(auth_line.contains(&format!("ts_out={}", if self.send_ts_out { 1 } else { 0 })));
            assert!(auth_line.contains(&format!("client=Rust {}", env!("CARGO_PKG_VERSION"))));
            if let Some(heartbeat_interval) = heartbeat_interval {
                assert!(auth_line.contains(&format!(
                    "heartbeat_interval_s={}",
                    heartbeat_interval.whole_seconds()
                )));
            } else {
                assert!(!auth_line.contains("heartbeat_interval_s="));
            }
            self.send("success=1|session_id=5\n").await;
        }

        async fn subscribe(&mut self, subscription: Subscription, is_last: bool) {
            let sub_line = self.read_line().await;
            assert!(sub_line.contains(&format!("symbols={}", subscription.symbols.to_api_string())));
            assert!(sub_line.contains(&format!("schema={}", subscription.schema)));
            assert!(sub_line.contains(&format!("stype_in={}", subscription.stype_in)));
            assert!(sub_line.contains("id="));
            if let Some(start) = subscription.start {
                assert!(sub_line.contains(&format!("start={}", start.unix_timestamp_nanos())))
            }
            assert!(sub_line.contains(&format!("snapshot={}", subscription.use_snapshot as u8)));
            assert!(sub_line.contains(&format!("is_last={}", is_last as u8)));
        }

        async fn start(&mut self) {
            let start_line = self.read_line().await;
            assert_eq!(start_line, "start_session\n");
            let dataset = self.dataset.clone();
            let stream = self.stream();
            let mut encoder = AsyncDbnMetadataEncoder::new(stream);
            encoder
                .encode(
                    &MetadataBuilder::new()
                        .dataset(dataset)
                        .start(time::OffsetDateTime::now_utc().unix_timestamp_nanos() as u64)
                        .schema(None)
                        .stype_in(None)
                        .stype_out(SType::InstrumentId)
                        .build(),
                )
                .await
                .unwrap();
        }

        async fn send(&mut self, bytes: &str) {
            self.stream().write_all(bytes.as_bytes()).await.unwrap();
            info!("Sent: {}", &bytes[..bytes.len() - 1])
        }

        async fn send_record(&mut self, record: Box<dyn AsRef<[u8]> + Send>) {
            let bytes = (*record).as_ref();
            // test for partial read bugs
            let half = bytes.len() / 2;
            self.stream().write_all(&bytes[..half]).await.unwrap();
            self.stream().flush().await.unwrap();
            self.stream().write_all(&bytes[half..]).await.unwrap();
        }

        async fn read_line(&mut self) -> String {
            let mut res = String::new();
            self.stream().read_line(&mut res).await.unwrap();
            info!("Read: {}", &res[..res.len() - 1]);
            res
        }

        fn stream(&mut self) -> &mut BufReader<TcpStream> {
            self.stream.as_mut().unwrap()
        }

        async fn close(&mut self) {
            if let Some(stream) = self.stream.as_mut() {
                stream.shutdown().await.unwrap();
            }
            self.stream = None;
        }
    }

    struct Fixture {
        send: UnboundedSender<Event>,
        port: u16,
        task: JoinHandle<()>,
    }

    enum Event {
        Exit,
        Accept,
        Authenticate(Option<Duration>),
        Send(String),
        Subscribe(Subscription, bool),
        Start,
        SendRecord(Box<dyn AsRef<[u8]> + Send>),
        Disconnect,
    }

    impl fmt::Debug for Event {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                Event::Exit => write!(f, "Exit"),
                Event::Accept => write!(f, "Accept"),
                Event::Authenticate(hb_int) => write!(f, "Authenticate({hb_int:?})"),
                Event::Send(msg) => write!(f, "Send({msg:?})"),
                Event::Subscribe(sub, is_last) => write!(f, "Subscribe({sub:?}, {is_last:?})"),
                Event::Start => write!(f, "Start"),
                Event::SendRecord(_) => write!(f, "SendRecord"),
                Event::Disconnect => write!(f, "Disconnect"),
            }
        }
    }

    impl Fixture {
        pub async fn new(dataset: String, send_ts_out: bool) -> Self {
            let (send, mut recv) = tokio::sync::mpsc::unbounded_channel();
            let mut mock = MockLsgServer::new(dataset, send_ts_out).await;
            let port = mock.listener.local_addr().unwrap().port();
            let task = tokio::task::spawn(async move {
                loop {
                    match recv.recv().await {
                        Some(Event::Authenticate(hb_int)) => mock.authenticate(hb_int).await,
                        Some(Event::Accept) => mock.accept().await,
                        Some(Event::Send(msg)) => mock.send(&msg).await,
                        Some(Event::Subscribe(sub, is_last)) => mock.subscribe(sub, is_last).await,
                        Some(Event::Start) => mock.start().await,
                        Some(Event::SendRecord(rec)) => mock.send_record(rec).await,
                        Some(Event::Disconnect) => mock.close().await,
                        Some(Event::Exit) | None => break,
                    }
                }
            });
            Self { task, port, send }
        }

        /// Accept but don't authenticate
        pub fn accept(&mut self) {
            self.send.send(Event::Accept).unwrap();
        }

        /// Accept and authenticate
        pub fn authenticate(&mut self, heartbeat_interval: Option<Duration>) {
            self.send
                .send(Event::Authenticate(heartbeat_interval))
                .unwrap();
        }

        pub fn expect_subscribe(&mut self, subscription: Subscription, is_last: bool) {
            self.send
                .send(Event::Subscribe(subscription, is_last))
                .unwrap();
        }

        pub fn start(&mut self) {
            self.send.send(Event::Start).unwrap();
        }

        pub fn send(&mut self, msg: String) {
            self.send.send(Event::Send(msg)).unwrap();
        }

        pub fn send_record<R>(&mut self, record: R)
        where
            R: HasRType + AsRef<[u8]> + Clone + Send + 'static,
        {
            self.send
                .send(Event::SendRecord(Box::new(record.clone())))
                .unwrap();
        }

        pub fn disconnect(&mut self) {
            self.send.send(Event::Disconnect).unwrap()
        }

        pub async fn stop(self) {
            self.send.send(Event::Exit).unwrap();
            self.task.await.unwrap()
        }
    }

    async fn setup(
        dataset: Dataset,
        send_ts_out: bool,
        heartbeat_interval: Option<Duration>,
    ) -> (Fixture, Client) {
        let _ = tracing_subscriber::FmtSubscriber::builder()
            .with_max_level(LevelFilter::DEBUG)
            .with_test_writer()
            .try_init();
        let mut fixture = Fixture::new(dataset.to_string(), send_ts_out).await;
        fixture.authenticate(heartbeat_interval);
        let builder = Client::builder()
            .addr(format!("127.0.0.1:{}", fixture.port))
            .await
            .unwrap()
            .key("32-character-with-lots-of-filler".to_owned())
            .unwrap()
            .dataset(dataset.to_string())
            .send_ts_out(send_ts_out);
        let target = if let Some(heartbeat_interval) = heartbeat_interval {
            builder.heartbeat_interval(heartbeat_interval)
        } else {
            builder
        }
        .build()
        .await
        .unwrap();
        (fixture, target)
    }

    #[tokio::test]
    async fn test_subscribe() {
        let (mut fixture, mut client) = setup(Dataset::XnasItch, false, None).await;
        let subscription = Subscription::builder()
            .symbols(vec!["MSFT", "TSLA", "QQQ"])
            .schema(Schema::Ohlcv1M)
            .stype_in(SType::RawSymbol)
            .build();
        fixture.expect_subscribe(subscription.clone(), true);
        client.subscribe(subscription).await.unwrap();
        fixture.stop().await;
    }

    #[tokio::test]
    async fn test_subscribe_snapshot() {
        let (mut fixture, mut client) =
            setup(Dataset::XnasItch, false, Some(Duration::MINUTE)).await;
        let subscription = Subscription::builder()
            .symbols(vec!["MSFT", "TSLA", "QQQ"])
            .schema(Schema::Ohlcv1M)
            .stype_in(SType::RawSymbol)
            .use_snapshot()
            .build();
        fixture.expect_subscribe(subscription.clone(), true);
        client.subscribe(subscription).await.unwrap();
        fixture.stop().await;
    }

    #[tokio::test]
    async fn test_subscribe_snapshot_failed() {
        let (fixture, mut client) =
            setup(Dataset::XnasItch, false, Some(Duration::seconds(5))).await;

        let err = client
            .subscribe(
                Subscription::builder()
                    .symbols(vec!["MSFT", "TSLA", "QQQ"])
                    .schema(Schema::Ohlcv1M)
                    .stype_in(SType::RawSymbol)
                    .start(time::OffsetDateTime::now_utc())
                    .use_snapshot()
                    .build(),
            )
            .await
            .unwrap_err();
        assert!(err
            .to_string()
            .contains("cannot request snapshot with start time"));

        fixture.stop().await;
    }

    #[tokio::test]
    async fn test_subscription_chunking() {
        const SYMBOL: &str = "TEST";
        const SYMBOL_COUNT: usize = 1001;
        let (mut fixture, mut client) = setup(Dataset::XnasItch, false, None).await;
        let sub_base = Subscription::builder()
            .schema(Schema::Ohlcv1M)
            .stype_in(SType::RawSymbol);
        let subscription = sub_base.clone().symbols(vec![SYMBOL; SYMBOL_COUNT]).build();
        client.subscribe(subscription).await.unwrap();
        let mut i = 0;
        while i < SYMBOL_COUNT {
            let chunk_size = 500.min(SYMBOL_COUNT - i);
            fixture.expect_subscribe(
                sub_base.clone().symbols(vec![SYMBOL; chunk_size]).build(),
                i + chunk_size == SYMBOL_COUNT,
            );
            i += chunk_size;
        }
        fixture.stop().await;
    }

    #[tokio::test]
    async fn test_next_record() {
        const REC: OhlcvMsg = OhlcvMsg {
            hd: RecordHeader::new::<OhlcvMsg>(rtype::OHLCV_1M, 1, 2, 3),
            open: 1,
            high: 2,
            low: 3,
            close: 4,
            volume: 5,
        };
        let (mut fixture, mut client) =
            setup(Dataset::GlbxMdp3, false, Some(Duration::minutes(5))).await;
        fixture.start();
        let metadata = client.start().await.unwrap();
        assert_eq!(metadata.version, dbn::DBN_VERSION);
        assert!(metadata.schema.is_none());
        assert_eq!(metadata.dataset, Dataset::GlbxMdp3.as_str());
        fixture.send_record(REC);
        let rec = client.next_record().await.unwrap().unwrap();
        assert_eq!(*rec.get::<OhlcvMsg>().unwrap(), REC);
        fixture.stop().await;
    }

    #[tokio::test]
    async fn test_next_record_with_ts_out() {
        let expected = WithTsOut::new(
            TradeMsg {
                hd: RecordHeader::new::<TradeMsg>(rtype::MBP_0, 1, 2, 3),
                price: 1,
                size: 2,
                action: b'A' as c_char,
                side: b'A' as c_char,
                flags: FlagSet::default(),
                depth: 1,
                ts_recv: 0,
                ts_in_delta: 0,
                sequence: 2,
            },
            time::OffsetDateTime::now_utc().unix_timestamp_nanos() as u64,
        );
        let (mut fixture, mut client) = setup(Dataset::GlbxMdp3, true, None).await;
        fixture.start();
        let metadata = client.start().await.unwrap();
        assert_eq!(metadata.version, dbn::DBN_VERSION);
        assert!(metadata.schema.is_none());
        assert_eq!(metadata.dataset, Dataset::GlbxMdp3.as_str());
        fixture.send_record(expected.clone());
        let rec = client.next_record().await.unwrap().unwrap();
        assert_eq!(*rec.get::<WithTsOut<TradeMsg>>().unwrap(), expected);
        fixture.stop().await;
    }

    #[tokio::test]
    async fn test_close() {
        let (mut fixture, mut client) =
            setup(Dataset::GlbxMdp3, true, Some(Duration::seconds(45))).await;
        fixture.start();
        client.start().await.unwrap();
        client.close().await.unwrap();
        fixture.stop().await;
    }

    #[tokio::test]
    async fn test_error_without_success() {
        const DATASET: Dataset = Dataset::OpraPillar;
        let mut fixture = Fixture::new(DATASET.to_string(), false).await;
        let client_task = tokio::spawn(async move {
            let res = Client::builder()
                .addr(format!("127.0.0.1:{}", fixture.port))
                .await
                .unwrap()
                .key("32-character-with-lots-of-filler".to_owned())
                .unwrap()
                .dataset(DATASET.to_string())
                .build()
                .await;
            if let Err(e) = &res {
                dbg!(e);
            }
            assert!(matches!(res, Err(e) if e.to_string().contains("Unknown failure")));
        });
        let fixture_task = tokio::spawn(async move {
            fixture.accept();

            fixture.send("lsg-test\n".to_owned());
            fixture.send("cram=t7kNhwj4xqR0QYjzFKtBEG2ec2pXJ4FK\n".to_owned());
            fixture.send("Unknown failure\n".to_owned());
        });
        let (r1, r2) = join!(client_task, fixture_task);
        r1.unwrap();
        r2.unwrap();
    }

    #[tokio::test]
    async fn test_cancellation_safety() {
        let (mut fixture, mut client) = setup(Dataset::GlbxMdp3, true, None).await;
        fixture.start();
        let metadata = client.start().await.unwrap();
        assert_eq!(metadata.version, dbn::DBN_VERSION);
        assert!(metadata.schema.is_none());
        assert_eq!(metadata.dataset, Dataset::GlbxMdp3.as_str());
        fixture.send_record(Mbp10Msg::default());

        let mut int_1 = tokio::time::interval(std::time::Duration::from_millis(1));
        let mut int_2 = tokio::time::interval(std::time::Duration::from_millis(1));
        let mut int_3 = tokio::time::interval(std::time::Duration::from_millis(1));
        let mut int_4 = tokio::time::interval(std::time::Duration::from_millis(1));
        let mut int_5 = tokio::time::interval(std::time::Duration::from_millis(1));
        let mut int_6 = tokio::time::interval(std::time::Duration::from_millis(1));
        for _ in 0..5_000 {
            select! {
                _ =  int_1.tick() => {
                    fixture.send_record(Mbp10Msg::default());
                }
                _ =  int_2.tick() => {
                    fixture.send_record(Mbp10Msg::default());
                }
                _ =  int_3.tick() => {
                    fixture.send_record(Mbp10Msg::default());
                }
                _ =  int_4.tick() => {
                    fixture.send_record(Mbp10Msg::default());
                }
                _ =  int_5.tick() => {
                    fixture.send_record(Mbp10Msg::default());
                }
                _ =  int_6.tick() => {
                    fixture.send_record(Mbp10Msg::default());
                }
                res = client.next_record() => {
                    let rec = res.unwrap().unwrap();
                    dbg!(rec.header());
                    assert_eq!(*rec.get::<Mbp10Msg>().unwrap(), Mbp10Msg::default());
                }
            }
        }
        fixture.stop().await;
    }

    #[tokio::test]
    async fn test_reconnect() {
        let (mut fixture, mut client) = setup(Dataset::EqusMini, true, None).await;
        let sub = Subscription::builder()
            .symbols(["SPY", "QQQ"])
            .schema(Schema::Trades)
            .start(OffsetDateTime::UNIX_EPOCH)
            .build();
        fixture.expect_subscribe(sub.clone(), true);
        client.subscribe(sub.clone()).await.unwrap();
        fixture.start();
        let metadata = client.start().await.unwrap();
        assert_eq!(metadata.version, dbn::DBN_VERSION);
        assert!(metadata.schema.is_none());
        assert_eq!(metadata.dataset, Dataset::EqusMini.as_str());

        let trade = TradeMsg {
            hd: RecordHeader::default::<TradeMsg>(rtype::MBP_0),
            price: 1,
            size: 2,
            action: 'T' as c_char,
            side: 'B' as c_char,
            flags: FlagSet::default(),
            depth: 0,
            ts_recv: 3,
            ts_in_delta: 4,
            sequence: 5,
        };
        fixture.send_record(trade.clone());
        assert_eq!(
            *client
                .next_record()
                .await
                .unwrap()
                .unwrap()
                .get::<TradeMsg>()
                .unwrap(),
            trade
        );

        fixture.disconnect();
        // Receives None when gateway closes connection
        assert!(client.next_record().await.unwrap().is_none());
        fixture.authenticate(None);
        client.reconnect().await.unwrap();

        let mut resub = sub.clone();
        resub.start = None;
        fixture.expect_subscribe(resub, true);
        client.resubscribe().await.unwrap();
        fixture.start();
        client.start().await.unwrap();
        fixture.send_record(trade.clone());
        assert_eq!(
            *client
                .next_record()
                .await
                .unwrap()
                .unwrap()
                .get::<TradeMsg>()
                .unwrap(),
            trade
        );

        fixture.stop().await;
    }
}
