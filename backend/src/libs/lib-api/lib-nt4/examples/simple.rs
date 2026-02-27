use lib_asyncapi_macro::asyncapi;
use lib_nt4::types::{AsyncApiData, AsyncApiPath, AsyncApiPayload};
use lib_nt4::Nt4App;
use lib_nt4_macro::nt4;
use nt_client::NewClientOptions;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

#[derive(Default)]
struct Counter(AtomicI32);

#[asyncapi(summary = "", description = "")]
#[nt4(pub, "demo.counter")]
fn pub_counter(data: AsyncApiData<Counter>) -> i32 {
    data.0 .0.load(Ordering::SeqCst)
}

#[asyncapi(summary = "", description = "")]
#[nt4(pub, "demo.double")]
fn pub_double(data: AsyncApiData<Counter>) -> i32 {
    data.0 .0.load(Ordering::SeqCst) * 2
}

#[asyncapi(summary = "", description = "")]
#[nt4(sub, "demo.set")]
fn sub_set(payload: AsyncApiPayload<i32>, data: AsyncApiData<Counter>) {
    data.0 .0.store(payload.0, Ordering::SeqCst);
    println!("counter set to {}", payload.0);
}

#[asyncapi(summary = "", description = "")]
#[nt4(sub, "demo.inc.{val}")]
fn sub_inc(path: AsyncApiPath<(String,)>, data: AsyncApiData<Counter>) {
    if let Ok(v) = path.0 .0.parse::<i32>() {
        data.0 .0.fetch_add(v, Ordering::SeqCst);
    }
}

#[tokio::main]
async fn main() {
    let mut app = Nt4App::with_options(NewClientOptions { unsecure_port: 5810, ..Default::default() });
    app.set_update_interval(std::time::Duration::from_millis(100));
    app.add_data(Arc::new(Counter::default()));
    app.add_pub(pub_counter);
    app.add_pub(pub_double);
    app.add_sub(sub_set);
    app.add_sub(sub_inc);
    let app = Arc::new(app);

    tokio::spawn(app.clone().start());
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
}
