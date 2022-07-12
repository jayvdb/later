#[macro_use]
extern crate rocket;

use bg::*;
use fnf_rs::{BackgroundJobServer, JobId};
use rocket::State;

mod bg;
#[allow(dead_code)]
mod non_generated;

fn handle_sample_message(
    _ctx: &DeriveHandlerContext<JobContext>,
    payload: SampleMessage,
) -> anyhow::Result<()> {
    println!("On Handle handle_sample_message: {:?}", payload);

    Ok(())
}
fn handle_another_sample_message(
    _ctx: &DeriveHandlerContext<JobContext>,
    payload: AnotherSampleMessage,
) -> anyhow::Result<()> {
    let id = _ctx.enqueue(SampleMessage {
        txt: "test".to_string(),
    })?;

    println!("On Handle handle_another_sample_message: {:?}, enqueued: {}", payload, id);

    Ok(())
}

struct AppContext {
    jobs: BackgroundJobServer<JobContext, DeriveHandler<JobContext>>,
}

impl AppContext {
    pub fn enqueue<T: fnf_rs::JobParameter>(&self, msg: T) -> anyhow::Result<JobId> {
        self.jobs.enqueue(msg)
    }
}

#[get("/")]
fn hello(state: &State<AppContext>) -> String {
    let id = uuid::Uuid::new_v4().to_string();
    let msg = AnotherSampleMessage { txt: id };
    state.enqueue(msg).expect("Enqueue Job");
    "Hello, world!".to_string()
}

#[launch]
fn rocket() -> _ {
    let job_ctx = JobContext {};
    let bjs = DeriveHandlerBuilder::new(
        job_ctx,
        "fnf-example".into(),
        "amqp://guest:guest@localhost:5672".into(),
    )
    .with_sample_message_handler(handle_sample_message)
    .with_another_sample_message_handler(handle_another_sample_message)
    .build()
    .expect("start bg server");

    let ctx = AppContext { jobs: bjs };

    #[cfg(debug_assertions)]
    non_generated::test_non_generated();

    rocket::build().mount("/", routes![hello]).manage(ctx)
}
