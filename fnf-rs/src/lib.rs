use std::{
    marker::PhantomData,
    sync::{Arc},
    thread::JoinHandle,
};

use crate::models::EnqueuedJob;
use amiquip::{Channel, Connection, ConsumerOptions, Exchange, Publish, QueueDeclareOptions};
use anyhow::Context;

pub use anyhow;
pub use fnf_core::*;
pub use serde;
pub use serde_json;

mod models;

pub struct BackgroundJobServer<C, H>
where
    H: BgJobHandler<C> + Sync + Send,
{
    ctx: PhantomData<C>,
    handler: PhantomData<H>,
    channel: Channel,
    _connection: Connection,
    routing_key: String,
    _workers: Vec<JoinHandle<anyhow::Result<()>>>,
}

impl<C, H> BackgroundJobServer<C, H>
where
    C: Sync + Send + Clone + 'static,
    H: BgJobHandler<C> + Sync + Send + 'static,
{
    pub fn start(
        id: &str,
        amqp_address: String,
        handler: H,
    ) -> anyhow::Result<Self> {
        let mut connection = Connection::insecure_open(&amqp_address)?;
        let channel = connection.open_channel(None)?;
        let routing_key = format!("fnf-rs-{}", id);

        let mut workers = Vec::new();
        let handler = Arc::new(handler);
        for id in 1..5 {
            let amqp_address = amqp_address.clone();
            let routing_key = routing_key.clone();
            let context = handler.get_ctx().clone();
            let handler = handler.clone();

            workers.push(std::thread::spawn(move || {
                start_worker(context, handler, id, &amqp_address, &routing_key)
            }));
        }

        Ok(Self {
            ctx: PhantomData,
            handler: PhantomData,
            channel,
            _connection: connection,
            routing_key,
            _workers: workers,
        })
    }

    pub fn enqueue(&self, message: impl JobParameter) -> anyhow::Result<()> {
        let id = uuid::Uuid::new_v4().to_string();

        // self.storage.set(format!("job-{}", id), job);
        // self.storage.push_job_id(id);

        let message = models::EnqueuedJob {
            id,
            ptype: message.get_ptype(),
            payload: message
                .to_bytes()
                .context("Unable to serialize the message to bytes")?,
        };
        let message_bytes = serde_json::to_vec(&message)?;
        let exchange = Exchange::direct(&self.channel);

        exchange.publish(Publish::new(&message_bytes, self.routing_key.clone()))?;

        Ok(())
    }
}

fn start_worker<C, H>(
    ctx: C,
    handler: Arc<H>,
    worker_id: i32,
    amqp_address: &str,
    routing_key: &str,
) -> anyhow::Result<()>
where
    C: Sync + Send + Clone,
    H: BgJobHandler<C> + Sync + Send,
{
    println!("[Worker#{}] Starting", worker_id);
    let mut connection = Connection::insecure_open(&amqp_address)?;
    let channel = connection.open_channel(None)?;
    let queue = channel.queue_declare(
        routing_key,
        QueueDeclareOptions {
            durable: true,
            auto_delete: false,
            ..Default::default()
        },
    )?;
    let consumer = queue.consume(ConsumerOptions {
        no_ack: false,
        ..Default::default()
    })?;

    for (i, message) in consumer.receiver().iter().enumerate() {
        match message {
            amiquip::ConsumerMessage::Delivery(delivery) => {
                match serde_json::from_slice::<EnqueuedJob>(&delivery.body) {
                    Ok(message) => {
                        // send to app and ack if successful
                        let id = message.id.clone();
                        let ptype = message.ptype;
                        let payload = message.payload;

                        println!(
                            "[Worker#{}] ({:>3}) Message received [Id: {}]",
                            worker_id, i, id
                        );

                        if let Ok(_) = handler.dispatch(ctx.clone(), ptype, &payload) {
                            consumer.ack(delivery)?;
                        }
                    }
                    Err(_err) => {
                        println!(
                            "[Worker#{}] ({:>3}) Unknown message received [{} bytes]",
                            worker_id,
                            i,
                            delivery.body.len()
                        );
                        consumer.nack(delivery, false)?;
                    }
                }
            }
            other => {
                println!("[Worker#{}] Consumer ended: {:?}", worker_id, other);
                break;
            }
        }
    }

    println!("[Worker#{}] Ended", worker_id);
    Ok(())
}
pub mod storage {
    use std::collections::{HashMap, HashSet};

    pub trait Storage {
        fn get(&'_ self, key: &str) -> Option<&'_ str>;
        fn push_job_id(&mut self, id: String);
        fn set(&mut self, key: String, value: String);
    }

    pub struct MemoryStorage {
        storage: HashMap<String, String>,
        jobs: HashSet<String>,
    }

    impl Storage for MemoryStorage {
        fn get(&'_ self, key: &str) -> Option<&'_ str> {
            let r = self.storage.get(key).map(|x| x.as_str());

            r
        }

        fn set(&mut self, key: String, value: String) {
            self.storage.insert(key, value);
        }

        fn push_job_id(&mut self, id: String) {
            self.jobs.insert(id);
        }
    }

    impl MemoryStorage {
        pub fn new() -> Self {
            Self {
                storage: Default::default(),
                jobs: Default::default(),
            }
        }
    }
}
