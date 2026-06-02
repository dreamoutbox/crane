pub fn interval_to_cron(interval: &str) -> String {
    let num_str: String = interval.chars().filter(|c| c.is_ascii_digit()).collect();
    let unit: String = interval
        .chars()
        .filter(|c| c.to_ascii_lowercase().is_alphabetic())
        .collect();
    let num: u32 = num_str.parse().unwrap_or(1);

    match unit.to_lowercase().as_str() {
        "s" | "sec" | "second" | "seconds" => {
            let minutes = num / 60;
            if minutes <= 1 {
                "* * * * *".to_string()
            } else {
                format!("*/{} * * * *", minutes)
            }
        }
        "m" | "min" | "minute" | "minutes" => {
            if num == 1 {
                "* * * * *".to_string()
            } else {
                format!("*/{} * * * *", num)
            }
        }
        "h" | "hr" | "hour" | "hours" => {
            if num == 1 {
                "0 * * * *".to_string()
            } else {
                format!("0 */{} * * *", num)
            }
        }
        "d" | "day" | "days" => {
            if num == 1 {
                "0 0 * * *".to_string()
            } else {
                format!("0 0 */{} * *", num)
            }
        }
        "w" | "wk" | "week" | "weeks" => {
            if num == 1 {
                "0 0 * * 0".to_string()
            } else {
                format!("0 0 */{} * *", num * 7)
            }
        }
        "mo" | "mth" | "month" | "months" => {
            if num == 1 {
                "0 0 1 * *".to_string()
            } else {
                format!("0 0 1 */{} *", num)
            }
        }
        _ => "0 0 * * *".to_string(), // default to daily
    }
}
