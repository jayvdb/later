use std::{
    sync::{Arc, Mutex},
    thread::JoinHandle,
};

use amiquip::{Channel, Connection, ConsumerOptions, Exchange, Publish, QueueDeclareOptions};
use storage::Storage;

pub type HandlerFunc<C> = Box<dyn Fn(C, String) -> anyhow::Result<()> + Sync + Send>;

pub struct BackgroundJobServer<S, C>
where
    S: Storage + Send + Sync,
    C: Send + Sync + Clone,
{
    storage: S,
    ctx: C,
    handler_fn: Arc<HandlerFunc<C>>,
    connection: Connection,
    channel: Channel,
    routing_key: String,
    workers: Vec<JoinHandle<anyhow::Result<()>>>,
    // exchange: Exchange<'exchange>,
}

impl<S, C> BackgroundJobServer<S, C>
where
    S: Storage + Sync + Send,
    C: Sync + Send + Clone,
{
    pub fn start(
        id: &str,
        amqp_address: String,
        storage: S,
        context: C,
        handler_fn: HandlerFunc<C>,
    ) -> anyhow::Result<Self> {
        let mut connection = Connection::insecure_open(&amqp_address)?;
        let channel = connection.open_channel(None)?;
        let routing_key = format!("fnf-rs-{}", id);
        //exchange.publish(Publish::new("", routing_key))?;

        let mut workers = Vec::new();
        for id in 1..5 {
            let amqp_address = amqp_address.clone();
            let routing_key = routing_key.clone();

            workers.push(std::thread::spawn(move || {
                start_worker(id, &amqp_address, routing_key)
            }));
        }

        Ok(Self {
            storage: storage,
            ctx: context,
            handler_fn: Arc::new(handler_fn),
            // exchange,
            channel,
            connection,
            routing_key,
            workers,
        })
    }

    pub fn enqueue(&mut self, job: String) -> anyhow::Result<()> {
        let id = uuid::Uuid::new_v4().to_string();
        let handler = self.handler_fn.clone();
        let ctx = self.ctx.clone();

        // self.storage.set(format!("job-{}", id), job);
        // self.storage.push_job_id(id);

        let exchange = Exchange::direct(&&self.channel);
        exchange.publish(Publish::new(job.as_bytes(), self.routing_key.clone()))?;

        // std::thread::spawn(move || {
        //     let r = (handler)(ctx, job);
        // });

        //let s = self.storage.get("".to_string());

        Ok(())
    }
}

fn start_worker(worker_id: i32, amqp_address: &str, routing_key: String) -> anyhow::Result<()> {
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
                let body = String::from_utf8_lossy(&delivery.body);
                println!("[Worker#{}] ({:>3}) Received [{}]", worker_id, i, body);
                consumer.ack(delivery)?;
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
