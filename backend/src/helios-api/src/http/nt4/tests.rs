#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;

    use rmpv::Value;

    use super::super::transport::{host_candidates, team_number_from_rio_hostname, team_number_from_rio_ip};
    use super::super::value::rmpv_to_json;

    #[test]
    fn parses_team_from_rio_ip() {
        assert_eq!(team_number_from_rio_ip(Ipv4Addr::new(10, 25, 4, 2)), Some(2504));
        assert_eq!(team_number_from_rio_ip(Ipv4Addr::new(10, 0, 0, 2)), None);
        assert_eq!(team_number_from_rio_ip(Ipv4Addr::new(172, 22, 11, 2)), None);
    }

    #[test]
    fn parses_team_from_rio_hostname() {
        assert_eq!(team_number_from_rio_hostname("roborio-254-frc.local"), Some(254));
        assert_eq!(team_number_from_rio_hostname("roborio-6328-frc"), Some(6328));
        assert_eq!(team_number_from_rio_hostname("roborio-254-frc.local."), Some(254));
        assert_eq!(team_number_from_rio_hostname("example.local"), None);
    }

    #[test]
    fn expands_rio_host_fallbacks() {
        let list = host_candidates("10.6.32.2");
        assert!(list.iter().any(|entry| entry == "10.6.32.2"));
        assert!(list.iter().any(|entry| entry == "roborio-632-frc.local"));
        assert!(list.iter().any(|entry| entry == "172.22.11.2"));
    }

    #[test]
    fn converts_binary_rmpv_payloads_to_base64_json() {
        let json = rmpv_to_json(&Value::Binary(vec![1, 2, 3, 4]));
        assert_eq!(json.get("encoding").and_then(|value| value.as_str()), Some("base64"));
        assert_eq!(json.get("data").and_then(|value| value.as_str()), Some("AQIDBA=="));
    }
}
