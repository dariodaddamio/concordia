use concordia::usage::read_codex_limits;
const BIN: &str = env!("CARGO_BIN_EXE_concordia-fake-provider");

#[tokio::test]
async fn native_limits_reader_binds_home_and_only_uses_initialize_and_quota_read() {
    for percent in [13, 72] {
        let dir = tempfile::tempdir().unwrap();
        let limits = serde_json::json!({"planType":"pro","primary":{"usedPercent":percent,"windowDurationMins":300,"resetsAt":1900000000i64}});
        let result = if percent == 72 {
            serde_json::json!({"rateLimits":null,"rateLimitsByLimitId":{"codex":limits}})
        } else {
            serde_json::json!({"rateLimits":limits})
        };
        std::fs::write(dir.path().join("rpc-fixture.json"), result.to_string()).unwrap();
        let result = read_codex_limits(BIN, dir.path()).await.unwrap();
        let value: serde_json::Value = serde_json::from_str(&result).unwrap();
        assert_eq!(value["rate_limits"]["primary"]["usedPercent"], percent);
        let calls = std::fs::read_to_string(dir.path().join("rpc-calls.jsonl")).unwrap();
        let methods: Vec<_> = calls
            .lines()
            .map(|l| {
                serde_json::from_str::<serde_json::Value>(l).unwrap()["method"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        assert_eq!(
            methods,
            ["initialize", "initialized", "account/rateLimits/read"]
        );
    }
}
#[tokio::test]
async fn quota_errors_closed_streams_and_oversized_output_fail_without_a_model_turn() {
    for mode in ["closed", "error", "flood"] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("rpc-fixture.json"),
            serde_json::json!({"mode":mode}).to_string(),
        )
        .unwrap();
        assert!(read_codex_limits(BIN, dir.path()).await.is_err());
        assert!(!std::fs::read_to_string(dir.path().join("rpc-calls.jsonl"))
            .unwrap()
            .contains("turn/start"));
    }
}
