//! test/issue-366-empty-auto-select.test.js

use crate::common::*;
use sublink::builders::BuildOptions;

#[tokio::test]
async fn does_not_emit_empty_urltest_outbound_when_no_proxies_are_parsed() {
    let o = BuildOptions {
        user_agent: "SFI/1.14.0 (20; sing-box 1.13.0; language zh_CN)".into(),
        ..opts("not-a-valid-subscription", v(r#"["Apple", "Microsoft", "Streaming"]"#))
    };
    let config = singbox(&o).await;
    assert!(find(config.get("outbounds"), "tag", "⚡ 自动选择").is_none());
    let node_select = outbound(&config, "🚀 节点选择");
    assert!(!has(node_select.get("outbounds"), "⚡ 自动选择"));
    assert_eq!(exact_strs(node_select.get("outbounds")), ["DIRECT"]);
}
