pub fn interval_to_cron(interval: &str) -> String {
    let num_str: String = interval.chars().filter(|c| c.is_ascii_digit()).collect();
    let unit: String = interval.chars().filter(|c| c.is_alphabetic()).collect();
    let num: u32 = num_str.parse().unwrap_or(1);

    match unit.as_str() {
        "m" => {
            if num == 1 {
                "* * * * *".to_string()
            } else {
                format!("*/{} * * * *", num)
            }
        }
        "h" => {
            if num == 1 {
                "0 * * * *".to_string()
            } else {
                format!("0 */{} * * *", num)
            }
        }
        "d" => {
            if num == 1 {
                "0 0 * * *".to_string()
            } else {
                format!("0 0 */{} * *", num)
            }
        }
        _ => "0 0 * * *".to_string(), // default to daily
    }
}
