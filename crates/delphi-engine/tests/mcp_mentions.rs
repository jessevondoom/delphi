//! Spec 077: Integration tests for host mention routing, themed alias resolution, and delphi_roster.

use delphi_engine::mcp_host::Server;
use delphi_engine::workspace::make_test_workspace;
use serde_json::json;

fn init_server(s: &mut Server) {
    let init_req = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "test-client", "version": "1.0"}
        }
    });
    s.handle(init_req);
}

fn call_tool(s: &mut Server, name: &str, args: serde_json::Value) -> serde_json::Value {
    let req = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": name,
            "arguments": args
        }
    });
    s.handle(req).expect("reply expected")
}

#[test]
fn mcp_mentions_get_agent_and_roster() {
    let tmp = tempfile::tempdir().unwrap();
    let ws = make_test_workspace(tmp.path());
    let mut s = Server::new(&ws.dir);
    init_server(&mut s);

    // 1. get_agent with themed alias ("athena") and canonical role ("orchestrator")
    let pack_alias = call_tool(&mut s, "get_agent", json!({"role": "athena"}));
    assert_ne!(pack_alias["result"]["isError"], true);
    let alias_text = pack_alias["result"]["content"][0]["text"].as_str().unwrap();
    assert!(alias_text.contains("Athena"), "expected Athena in agent pack: {alias_text}");
    assert!(alias_text.contains("⟦•─•⟧"), "expected badge in agent pack");

    let pack_canonical = call_tool(&mut s, "get_agent", json!({"role": "orchestrator"}));
    assert_ne!(pack_canonical["result"]["isError"], true);
    let canonical_text = pack_canonical["result"]["content"][0]["text"].as_str().unwrap();
    assert_eq!(alias_text, canonical_text, "themed alias and canonical role must produce identical agent packs");

    // 2. get_agent with Daedalus / architect
    let pack_daedalus = call_tool(&mut s, "get_agent", json!({"role": "Daedalus"}));
    let pack_architect = call_tool(&mut s, "get_agent", json!({"role": "architect"}));
    assert_eq!(
        pack_daedalus["result"]["content"][0]["text"].as_str().unwrap(),
        pack_architect["result"]["content"][0]["text"].as_str().unwrap()
    );

    // 3. list_roles returns aliases containing themed and role tags
    let list_res = call_tool(&mut s, "list_roles", json!({}));
    assert_ne!(list_res["result"]["isError"], true);
    let list_json: serde_json::Value = serde_json::from_str(list_res["result"]["content"][0]["text"].as_str().unwrap()).unwrap();
    let roles = list_json.as_array().expect("roles array");
    assert!(!roles.is_empty());

    let athena_entry = roles.iter().find(|r| r["role"] == "orchestrator").expect("orchestrator entry");
    assert_eq!(athena_entry["name"], "Athena");
    let aliases = athena_entry["aliases"].as_array().expect("aliases array");
    assert!(aliases.contains(&json!("@athena")));
    assert!(aliases.contains(&json!("@orchestrator")));
    assert!(aliases.contains(&json!("@delphi")));

    // 4. delphi_roster returns markdown roster table
    let roster_res = call_tool(&mut s, "delphi_roster", json!({}));
    assert_ne!(roster_res["result"]["isError"], true);
    let roster_text = roster_res["result"]["content"][0]["text"].as_str().unwrap();
    assert!(roster_text.contains("# Delphi Agent Roster"), "roster title");
    assert!(roster_text.contains("@athena"), "roster mentions @athena");
    assert!(roster_text.contains("@orchestrator"), "roster mentions @orchestrator");
    assert!(roster_text.contains("@daedalus"), "roster mentions @daedalus");
    assert!(roster_text.contains("@ares"), "roster mentions @ares");
}
