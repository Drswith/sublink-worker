//! Ports of the original vitest suites, one module per test file.

#[path = "../common/mod.rs"]
mod common;

mod anytls_protocol;
mod clash_builder;
mod country_group;
mod issue_256_custom_rule_global;
mod issue_297_vmess_network;
mod issue_306_mrs_format;
mod issue_334_rule_provider_collision;
mod issue_337_ss_cipher_decode;
mod issue_362_subscription_userinfo;
mod issue_366_empty_auto_select;
mod issue_370_empty_clash_output;
mod issue_371_custom_rule_options;
mod issue_388_trojan_tls;
mod issue_401_rule_set_download;
mod issue_403_provider_country_group;
mod issue_428_hy2_port_hopping;
mod kv_store;
mod node_runtime;
mod proxy_groups_override;
mod proxy_helpers;
mod proxy_providers;
mod selected_rules_compatibility;
mod singbox_input_parsing;
mod singbox_legacy_inbound;
mod singbox_route_order;
mod src_ip_cidr;
mod ss_plugin;
mod subconverter_endpoint;
mod surge_config_parser;
mod surge_input_parsing;
mod surge_unsupported_proxy;
mod udp_handling;
mod worker;
mod yaml_parsing;
