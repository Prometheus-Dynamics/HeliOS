use lib_nt4::Nt4App;
use lib_nt4_macro::nt4;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

static PUB: AtomicUsize = AtomicUsize::new(0);
static SUB: AtomicUsize = AtomicUsize::new(0);
static PUBSUB: AtomicUsize = AtomicUsize::new(0);

#[nt4(pub, "loop.pub")]
fn pub_loop(_app: Arc<Nt4App>) -> i32 {
    PUB.fetch_add(1, Ordering::SeqCst);
    1
}

#[nt4(sub, "loop.sub")]
fn sub_loop(_app: Arc<Nt4App>, _path: String, _val: serde_json::Value) {
    SUB.fetch_add(1, Ordering::SeqCst);
}

#[nt4(pubsub, "loop.pubsub")]
fn pubsub_loop(_app: Arc<Nt4App>, _path: String, _val: serde_json::Value) -> i32 {
    PUBSUB.fetch_add(1, Ordering::SeqCst);
    2
}

#[test]
fn handlers_run() {
    let app = Arc::new(Nt4App::new());
    futures::executor::block_on(pub_loop::execute(app.clone()));
    futures::executor::block_on(sub_loop::execute(app.clone(), String::new(), serde_json::Value::Null));
    futures::executor::block_on(pubsub_loop::execute(app, String::new(), serde_json::Value::Null));
    assert_eq!(PUB.load(Ordering::SeqCst), 1);
    assert_eq!(SUB.load(Ordering::SeqCst), 1);
    assert_eq!(PUBSUB.load(Ordering::SeqCst), 1);
}

#[test]
fn update_interval_setter() {
    let mut app = Nt4App::new();
    assert_eq!(app.update_interval(), Duration::from_secs(5));
    app.set_update_interval(Duration::from_secs(1));
    assert_eq!(app.update_interval(), Duration::from_secs(1));
}
