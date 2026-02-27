use lib_asyncapi_macro::{AsyncApiSchema, asyncapi};
use lib_ws_macro::ws;
extern crate lib_ws;
use lib_asyncapi::{AsyncApiPayload, DocumentedCommand};
extern crate serde_json;
use serde::Deserialize;

#[derive(Deserialize, AsyncApiSchema)]
struct Inner {
    #[asyncapi(example = 1)]
    val: u32,
}

#[derive(Deserialize, AsyncApiSchema)]
struct Outer {
    inner: Inner,
    #[asyncapi(example = 5)]
    other: u32,
}

#[derive(Deserialize, AsyncApiSchema)]
struct Resp {
    #[asyncapi(example = true)]
    ok: bool,
}

#[asyncapi(summary = "complex", description = "nested structs", tags = ["c"], response = [Outer, Resp])]
#[ws("cmd.complex")]
async fn complex_cmd(_p: AsyncApiPayload<Outer>) {}

#[lib_test::tokio_test]
async fn asyncapi_macro_nested_structs() {
    let doc = complex_cmd::doc().expect("missing doc");
    assert_eq!(doc.summary, "complex");
    assert_eq!(doc.description, "nested structs");
    assert!(doc.payload.is_some());
    let schema = doc.payload.as_ref().unwrap();
    assert_eq!(schema.name, "Outer");
    let inner_example = &schema.schema["properties"]["inner"]["example"];
    assert_eq!(inner_example["val"], serde_json::json!(1));
    assert_eq!(doc.responses.len(), 2);
    assert_eq!(doc.responses[0].name, "Outer");
    assert_eq!(doc.responses[1].name, "Resp");
    assert!(doc.params.is_empty());
    assert_eq!(complex_cmd::PATH, "cmd.complex");
    let inner = Inner { val: 1 };
    let outer = Outer { inner: Inner { val: 2 }, other: 3 };
    let resp = Resp { ok: true };
    let _ = inner.val;
    let _ = outer.inner.val;
    let _ = outer.other;
    let _ = resp.ok;
}
