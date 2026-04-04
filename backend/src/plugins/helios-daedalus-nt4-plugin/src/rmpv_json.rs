use base64::Engine;

pub fn to_json(value: &rmpv::Value) -> serde_json::Value {
    use rmpv::Value as V;
    match value {
        V::Nil => serde_json::Value::Null,
        V::Boolean(b) => serde_json::Value::Bool(*b),
        V::Integer(int) => {
            if let Some(i) = int.as_i64() {
                serde_json::Value::Number(i.into())
            } else if let Some(u) = int.as_u64() {
                serde_json::Value::Number(serde_json::Number::from(u))
            } else {
                serde_json::Value::Null
            }
        }
        V::F32(f) => serde_json::Value::Number(serde_json::Number::from_f64(*f as f64).unwrap_or_else(|| 0.into())),
        V::F64(f) => serde_json::Value::Number(serde_json::Number::from_f64(*f).unwrap_or_else(|| 0.into())),
        V::String(s) => serde_json::Value::String(s.to_string()),
        V::Binary(bin) => serde_json::Value::String(base64::engine::general_purpose::STANDARD.encode(bin)),
        V::Array(arr) => serde_json::Value::Array(arr.iter().map(to_json).collect()),
        V::Map(map) => {
            let mut out = serde_json::Map::new();
            for (k, v) in map {
                let key = match k {
                    V::String(s) => s.to_string(),
                    other => other.to_string(),
                };
                out.insert(key, to_json(v));
            }
            serde_json::Value::Object(out)
        }
        V::Ext(_, _) => serde_json::Value::Null,
    }
}
