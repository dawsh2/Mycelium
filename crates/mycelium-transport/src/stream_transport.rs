use crate::buffer_pool::BufferPool;
use crate::codec::write_message;
use crate::error::Result;
use crate::stream::handle_stream_connection;
use dashmap::DashMap;
use mycelium_protocol::{Envelope, Message};
use std::marker::PhantomData;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::{broadcast, Mutex};
use zerocopy::AsBytes;

#[derive(Clone)]
pub(crate) struct StreamTransportState {
    pub(crate) channels: Arc<DashMap<String, broadcast::Sender<Envelope>>>,
    pub(crate) type_to_topic: Arc<DashMap<u16, String>>,
    pub(crate) channel_capacity: usize,
    pub(crate) buffer_pool: Option<Arc<BufferPool>>,
}

impl StreamTransportState {
    pub(crate) fn new(channel_capacity: usize, buffer_pool: Option<BufferPool>) -> Self {
        Self {
            channels: Arc::new(DashMap::new()),
            type_to_topic: Arc::new(DashMap::new()),
            channel_capacity,
            buffer_pool: buffer_pool.map(Arc::new),
        }
    }

    pub(crate) fn register_type<M: Message>(&self) {
        self.type_to_topic.insert(M::TYPE_ID, M::TOPIC.to_string());
    }

    pub(crate) fn get_or_create_channel(&self, topic: &str) -> broadcast::Sender<Envelope> {
        self.channels
            .entry(topic.to_string())
            .or_insert_with(|| broadcast::channel(self.channel_capacity).0)
            .clone()
    }
}

pub(crate) fn spawn_client_reader<S>(stream: S, state: StreamTransportState, label: &'static str)
where
    S: AsyncReadExt + Unpin + Send + 'static,
{
    let channels = Arc::clone(&state.channels);
    let type_to_topic = Arc::clone(&state.type_to_topic);
    let buffer_pool = state.buffer_pool.clone();

    tokio::spawn(async move {
        let pool_ref = buffer_pool.as_ref().map(|p| p.as_ref());
        if let Err(e) = handle_stream_connection(stream, channels, type_to_topic, pool_ref).await {
            tracing::error!("{} client reader error: {}", label, e);
        }
    });
}

pub struct StreamPublisher<M, W> {
    write_half: Arc<Mutex<W>>,
    _phantom: PhantomData<M>,
}

impl<M, W> StreamPublisher<M, W> {
    pub(crate) fn new(write_half: Arc<Mutex<W>>) -> Self {
        Self {
            write_half,
            _phantom: PhantomData,
        }
    }
}

impl<M, W> StreamPublisher<M, W>
where
    M: Message + AsBytes,
    W: AsyncWriteExt + Unpin + Send,
{
    pub async fn publish(&self, msg: M) -> Result<()> {
        let mut stream = self.write_half.lock().await;
        write_message(&mut *stream, &msg).await?;
        stream.flush().await?;
        Ok(())
    }
}
