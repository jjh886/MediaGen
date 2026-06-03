use super::*;
use serde_json::json;

#[test]
fn parse_same_site_maps_cdp_strings_to_chrome_enum() {
    // Chrome SQLite enum: Unspecified=-1, None=0, Lax=1, Strict=2
    assert_eq!(parse_same_site(&json!({"sameSite": "Strict"})), 2);
    assert_eq!(parse_same_site(&json!({"sameSite": "Lax"})), 1);
    assert_eq!(parse_same_site(&json!({"sameSite": "None"})), 0);
    // 缺失字段必须落到 -1 (Unspecified)；旧实现硬编码 0 (None) 是 bug
    assert_eq!(parse_same_site(&json!({})), -1);
    assert_eq!(parse_same_site(&json!({"sameSite": "Other"})), -1);
}

#[test]
fn parse_source_scheme_uses_cdp_when_present() {
    assert_eq!(
        parse_source_scheme(&json!({"sourceScheme": "Secure"}), 0),
        2
    );
    assert_eq!(
        parse_source_scheme(&json!({"sourceScheme": "NonSecure"}), 1),
        1
    );
    assert_eq!(parse_source_scheme(&json!({"sourceScheme": "Unset"}), 1), 0);
}

#[test]
fn parse_source_scheme_falls_back_to_is_secure_when_missing() {
    // 兜底：CDP 不带 sourceScheme 时，按 is_secure 推断
    assert_eq!(parse_source_scheme(&json!({}), 1), 2);
    assert_eq!(parse_source_scheme(&json!({}), 0), 1);
}

#[test]
fn parse_source_port_preserves_cdp_minus_one() {
    // CDP 协议规定端口未知时返回 -1，必须如实保留：
    // 改成 443 反而和 Chrome 自己写入的 -1 行不上 unique index → 制造副本
    assert_eq!(parse_source_port(&json!({"sourcePort": -1}), 1), -1);
    assert_eq!(parse_source_port(&json!({"sourcePort": 443}), 1), 443);
    assert_eq!(parse_source_port(&json!({"sourcePort": 80}), 0), 80);
}

#[test]
fn parse_source_port_defaults_only_when_missing() {
    // 极旧 Chrome dump 完全没这个字段时才退到默认端口
    assert_eq!(parse_source_port(&json!({}), 1), 443);
    assert_eq!(parse_source_port(&json!({}), 0), 80);
}

#[test]
fn parse_partition_key_handles_object_form() {
    // 新版 CDP 返回 {topLevelSite, hasCrossSiteAncestor}
    let c = json!({
        "partitionKey": {
            "topLevelSite": "https://google.com",
            "hasCrossSiteAncestor": true
        }
    });
    assert_eq!(
        parse_partition_key(&c),
        ("https://google.com".to_string(), 1),
    );
}

#[test]
fn parse_partition_key_handles_legacy_string_form() {
    // 旧版 CDP 直接返回字符串
    let c = json!({"partitionKey": "https://google.com"});
    assert_eq!(
        parse_partition_key(&c),
        ("https://google.com".to_string(), 0),
    );
}

#[test]
fn parse_partition_key_defaults_when_missing() {
    assert_eq!(parse_partition_key(&json!({})), (String::new(), 0));
}

/// 回归：write_cookies_to_profile 写入的 cookie 必须能被 INSERT OR REPLACE 命中既有行，
/// 否则会和 Chrome 自己写入的真版 cookie 形成字段错位副本，破坏登录态。
/// 这里直接验证写出去的 SQLite 行 source_port=443、samesite=1 (Lax)，
/// 而不是旧实现硬编码的 -1 / 0。
#[test]
fn write_cookies_persists_cdp_fields_correctly() {
    let tmp = std::env::temp_dir().join(format!(
        "mediagen-cookie-fields-test-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&tmp).unwrap();

    let cookies = json!([{
        "name": "__Secure-1PSID",
        "value": "test_value",
        "domain": ".google.com",
        "path": "/",
        "secure": true,
        "httpOnly": true,
        "expires": 9_999_999_999.0,
        "sameSite": "Lax",
        "sourceScheme": "Secure",
        "sourcePort": 443,
        "partitionKey": {
            "topLevelSite": "https://google.com",
            "hasCrossSiteAncestor": true
        }
    }]);

    let written = write_cookies_to_profile(
        tmp.to_str().unwrap(),
        &serde_json::to_string(&cookies).unwrap(),
    );
    assert_eq!(written, 1);

    let conn = rusqlite::Connection::open(tmp.join("Default").join("Cookies")).unwrap();
    let row: (String, i32, i32, String, i32) = conn
        .query_row(
            "SELECT host_key, samesite, source_port, top_frame_site_key, has_cross_site_ancestor
             FROM cookies WHERE name='__Secure-1PSID'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .unwrap();
    assert_eq!(row.0, ".google.com");
    assert_eq!(
        row.1, 1,
        "samesite 必须是 Lax=1，旧实现硬编码 0 (None) 是 bug"
    );
    assert_eq!(
        row.2, 443,
        "source_port 必须是 443，旧实现硬编码 -1 会和 Chrome 真版字段错位"
    );
    assert_eq!(
        row.3, "https://google.com",
        "partitionKey.topLevelSite 必须保留"
    );
    assert_eq!(row.4, 1, "hasCrossSiteAncestor 必须保留");

    std::fs::remove_dir_all(&tmp).ok();
}

/// 回归：同一个 cookie 重复写入必须真的 REPLACE 而不是产生副本。
/// 旧实现硬编码 source_port=-1 + top_frame_site_key='' + has_cross_site_ancestor=0
/// 时这条恒成立，所以原本就只有一行；新实现保留真字段后，必须验证仍然是一行。
#[test]
fn write_cookies_replaces_existing_row_via_unique_index() {
    let tmp = std::env::temp_dir().join(format!(
        "mediagen-cookie-replace-test-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&tmp).unwrap();

    let cookie = json!([{
        "name": "sid",
        "value": "v1",
        "domain": ".doubao.com",
        "path": "/",
        "secure": true,
        "httpOnly": false,
        "expires": 9_999_999_999.0,
        "sameSite": "Lax",
        "sourceScheme": "Secure",
        "sourcePort": 443
    }]);
    let json_str = serde_json::to_string(&cookie).unwrap();

    write_cookies_to_profile(tmp.to_str().unwrap(), &json_str);
    write_cookies_to_profile(tmp.to_str().unwrap(), &json_str);

    let conn = rusqlite::Connection::open(tmp.join("Default").join("Cookies")).unwrap();
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM cookies WHERE name='sid'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        count, 1,
        "INSERT OR REPLACE 必须命中 unique index，重复写入只能有一行"
    );

    std::fs::remove_dir_all(&tmp).ok();
}
