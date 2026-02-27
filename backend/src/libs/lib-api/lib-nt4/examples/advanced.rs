use lib_asyncapi_macro::asyncapi;
use lib_nt4::types::{AsyncApiData, AsyncApiPath, AsyncApiPayload};
use lib_nt4::Nt4App;
use lib_nt4_macro::nt4;
use nt_client::NewClientOptions;
use std::sync::{
    atomic::{AtomicI32, Ordering},
    Arc, LazyLock,
};
use std::time::Instant;

#[derive(Default)]
struct Counter(AtomicI32);

static START: LazyLock<Instant> = LazyLock::new(Instant::now);

// --- standalone handlers ---

#[asyncapi(summary = "", description = "")]
#[nt4(pub, "demo.uptime")]
fn uptime_pub() -> u64 {
    START.elapsed().as_secs()
}

#[asyncapi(summary = "", description = "")]
#[nt4(sub, "demo.log")]
fn log_sub(payload: AsyncApiPayload<String>) {
    println!("log: {}", payload.0);
}

#[asyncapi(summary = "", description = "")]
#[nt4(pubsub, "demo.echo")]
fn echo_pubsub(payload: AsyncApiPayload<String>) -> String {
    payload.0
}

// --- interconnected handlers using shared Counter ---

#[asyncapi(summary = "", description = "")]
#[nt4(pub, "counter.value")]
fn counter_pub(data: AsyncApiData<Counter>) -> i32 {
    data.0 .0.load(Ordering::SeqCst)
}

#[asyncapi(summary = "", description = "")]
#[nt4(sub, "counter.set")]
fn counter_set(payload: AsyncApiPayload<i32>, data: AsyncApiData<Counter>) {
    data.0 .0.store(payload.0, Ordering::SeqCst);
}

#[asyncapi(summary = "", description = "")]
#[nt4(pubsub, "counter.add.{val}")]
fn counter_add(path: AsyncApiPath<(String,)>, data: AsyncApiData<Counter>) -> i32 {
    if let Ok(v) = path.0 .0.parse::<i32>() {
        data.0 .0.fetch_add(v, Ordering::SeqCst) + v
    } else {
        0
    }
}

#[tokio::main]
async fn main() {
    let mut app = Nt4App::with_options(NewClientOptions { unsecure_port: 5810, ..Default::default() });
    app.set_update_interval(std::time::Duration::from_millis(100));
    app.add_data(Arc::new(Counter::default()));

    // standalone handlers
    app.add_pub(uptime_pub);
    app.add_sub(log_sub);
    app.add_pubsub(echo_pubsub);

    // interconnected handlers
    app.add_pub(counter_pub);
    app.add_sub(counter_set);
    app.add_pubsub(counter_add);

    let app = Arc::new(app);

    tokio::spawn(app.clone().start());
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}
