//! Drives the real `liquidcad serve` binary over stdin/stdout, the same way
//! the Electron main process does.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn sample() -> String {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../projects/modular-dock/liquidcad.json")
        .display()
        .to_string()
}

#[test]
fn serve_speaks_the_protocol() {
    let data_dir = std::env::temp_dir().join(format!("liquidcad-serve-{}", std::process::id()));
    let mut child = Command::new(env!("CARGO_BIN_EXE_liquidcad"))
        .args(["serve", "--data-dir"])
        .arg(&data_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("engine starts");
    let mut stdin = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut next = || -> Value { serde_json::from_str(&lines.next().unwrap().unwrap()).unwrap() };

    let ready = next();
    assert_eq!(ready["event"], "ready");
    assert!(ready["kernel"].as_str().unwrap().len() > 3);

    let send = |stdin: &mut std::process::ChildStdin, id: u64, payload: Value| {
        writeln!(stdin, "{}", json!({ "id": id, "payload": payload })).unwrap();
    };
    send(
        &mut stdin,
        1,
        json!({ "method": "open", "params": { "path": sample() } }),
    );
    let opened = next();
    assert_eq!(opened["id"], 1);
    assert_eq!(opened["result"]["type"], "session");
    let meshes = opened["result"]["data"]["document"]["scene"]["meshes"]
        .as_array()
        .unwrap();
    // 84 dock parts plus the 27-part gangway.
    assert_eq!(meshes.len(), 111);

    // Three edits of the same parameter in one burst.
    let burst: String = [120, 168, 144]
        .iter()
        .enumerate()
        .map(|(i, w)| {
            format!(
                "{}\n",
                json!({ "id": 10 + i, "payload": { "method": "setParameter", "params": { "name": "width_in", "value": w } } })
            )
        })
        .collect();
    stdin.write_all(burst.as_bytes()).unwrap();
    let replies: Vec<Value> = (0..3).map(|_| next()).collect();
    assert_eq!(
        replies
            .iter()
            .map(|r| r["id"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        vec![10, 11, 12]
    );
    let last = &replies[2]["result"];
    assert_eq!(last["type"], "session");
    assert_eq!(
        last["data"]["document"]["project"]["parameters"]["width_in"]["value"],
        144.0
    );
    for r in &replies[..2] {
        let kind = r["result"]["type"].as_str().unwrap();
        assert!(kind == "superseded" || kind == "session", "{r}");
    }

    send(
        &mut stdin,
        20,
        json!({ "method": "setParameter", "params": { "name": "width_in", "value": 100 } }),
    );
    let rejected = next();
    assert_eq!(rejected["error"]["code"], "validation");
    assert!(rejected["error"]["message"]
        .as_str()
        .unwrap()
        .contains("96 or 120"));

    writeln!(
        stdin,
        "{{\"id\":21,\"payload\":{{\"method\":\"deleteEverything\"}}}}"
    )
    .unwrap();
    let bad = next();
    assert_eq!(bad["id"], 21);
    assert_eq!(bad["error"]["code"], "protocol");

    send(
        &mut stdin,
        22,
        json!({ "method": "select", "params": { "id": "frame.stringer.stbd" } }),
    );
    let selection = next();
    assert_eq!(selection["result"]["type"], "selection");
    assert_eq!(
        selection["result"]["data"]["component"]["bounds"]["origin"][0],
        142.0
    );

    send(
        &mut stdin,
        23,
        json!({ "method": "history", "params": { "limit": 3 } }),
    );
    let history = next();
    assert_eq!(history["result"]["type"], "history");
    assert!(!history["result"]["data"].as_array().unwrap().is_empty());

    send(&mut stdin, 24, json!({ "method": "drawing" }));
    let drawing = next();
    assert_eq!(drawing["result"]["type"], "drawing");
    assert_eq!(drawing["result"]["data"]["width"], 1224.0);

    drop(stdin);
    assert!(child.wait().unwrap().success());
}
