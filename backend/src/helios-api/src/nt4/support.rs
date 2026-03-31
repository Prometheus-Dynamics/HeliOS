use std::net::Ipv4Addr;

pub(crate) fn client_name_from_hostname(hostname: &str) -> String {
    let trimmed = hostname.trim();
    if trimmed.is_empty() {
        return "HeliOS".to_string();
    }
    trimmed.to_string()
}

pub(crate) fn team_number_to_rio_ip(team: u32) -> Option<Ipv4Addr> {
    if team == 0 || team > 25_599 {
        return None;
    }
    let a = (team / 100) as u8;
    let b = (team % 100) as u8;
    Some(Ipv4Addr::new(10, a, b, 2))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_hostname_uses_default_client_name() {
        assert_eq!(client_name_from_hostname("   "), "HeliOS");
    }

    #[test]
    fn team_number_to_rio_ip_rejects_out_of_range_values() {
        assert_eq!(team_number_to_rio_ip(0), None);
        assert_eq!(team_number_to_rio_ip(25_600), None);
    }

    #[test]
    fn team_number_to_rio_ip_formats_valid_team_numbers() {
        assert_eq!(team_number_to_rio_ip(6328), Some(Ipv4Addr::new(10, 63, 28, 2)));
    }
}
