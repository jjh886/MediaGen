use rand::Rng;

pub(super) fn new_code() -> String {
    format!("{:06}", rand::thread_rng().gen_range(0..1_000_000))
}

pub(super) fn parse_code(text: &str) -> Option<&str> {
    let code = text.trim();
    (code.len() == 6 && code.bytes().all(|byte| byte.is_ascii_digit())).then_some(code)
}

#[cfg(test)]
mod tests {
    #[test]
    fn accepts_only_six_plain_digits() {
        assert_eq!(super::parse_code("269967"), Some("269967"));
        assert_eq!(super::parse_code("绑定269967"), None);
        assert_eq!(super::parse_code("26996"), None);
    }
}
