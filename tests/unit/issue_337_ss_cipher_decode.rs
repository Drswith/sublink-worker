//! test/issue-337-ss-cipher-decode.test.js

use crate::common::*;

async fn proxy(input: &str, name: &str) -> sublink::js::Value {
    let built = clash(&opts(input, "minimal".into())).await;
    find(built.get("proxies"), "name", name).cloned().unwrap_or_else(|| panic!("missing proxy {name}"))
}

#[tokio::test]
async fn parses_base64_cipher_containing_plus_and_slash() {
    let p = proxy("ss://YWVzLTI1Ni1nY206dGVzdCtwYXNzL3dvcmQ9@example.com:8388#Test-Node", "Test-Node").await;
    assert_eq!(p.get("type").as_str(), Some("ss"));
    assert_eq!(p.get("cipher").as_str(), Some("aes-256-gcm"));
    assert_eq!(p.get("password").as_str(), Some("test+pass/word="));
}

#[tokio::test]
async fn parses_cipher_containing_plus_from_sub_store() {
    let p = proxy("ss://YWVzLTEyOC1nY206bXkrc2VjcmV0@sub-store.example.com:8080#SubStore-Node", "SubStore-Node").await;
    assert_eq!(p.get("cipher").as_str(), Some("aes-128-gcm"));
    assert_eq!(p.get("password").as_str(), Some("my+secret"));
}

#[tokio::test]
async fn handles_multiple_ss_urls_with_special_chars() {
    let input = "ss://YWVzLTI1Ni1nY206cGFzcyt3aXRoK3BsdXM=@server1.com:8388#Node1\nss://YWVzLTI1Ni1nY206cGFzcy93aXRoL3NsYXNo@server2.com:8388#Node2";
    assert_eq!(proxy(input, "Node1").await.get("password").as_str(), Some("pass+with+plus"));
    assert_eq!(proxy(input, "Node2").await.get("password").as_str(), Some("pass/with/slash"));
}

#[tokio::test]
async fn handles_url_encoded_base64_userinfo() {
    let p =
        proxy("ss://YWVzLTI1Ni1nY206dGVzdCtwYXNzL3dvcmQ%3D@sip002.example.com:8388#SIP002-Node", "SIP002-Node").await;
    assert_eq!(p.get("cipher").as_str(), Some("aes-256-gcm"));
    assert_eq!(p.get("password").as_str(), Some("test+pass/word"));
}
