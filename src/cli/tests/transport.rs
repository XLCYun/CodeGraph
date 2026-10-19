use super::{encode, writer_channel, Source};
use crate::cli::{protocol, send};
use serde_json::json;

/// 未调度写线程时也必须接纳 ready 与 completed，但不能无限积压后续消息。
#[test]
fn output_queue_accepts_ready_and_terminal_before_writer_runs() {
    let (sender, receiver) = writer_channel(Source::Output);
    let ready = json!({"protocol":protocol::PROTOCOL,"type":"ready","engine_version":"0.1.0","pack_id":"typescript-javascript"});
    let completed = protocol::failed(
        None,
        &protocol::Failure::new("invalid_request", "未收到 analyze 请求"),
    );
    send(&sender, &ready).unwrap();
    send(&sender, &completed).expect("ready + completed must fit before the writer is scheduled");
    assert!(send(&sender, &completed).is_err(), "output remains bounded");
    assert_eq!(receiver.try_recv().unwrap(), encode(&ready).unwrap());
    assert_eq!(receiver.try_recv().unwrap(), encode(&completed).unwrap());
    assert!(receiver.try_recv().is_err());
}
