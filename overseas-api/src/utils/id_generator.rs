use chrono::Utc;
use rand::Rng;

fn generate_compact_id(prefix: &str) -> String {
    let timestamp = Utc::now().timestamp_millis().rem_euclid(1_000_000_000);
    let random: u32 = rand::thread_rng().gen_range(0..=0xFF_FFFF);
    format!("{}{:09}{:06X}", prefix, timestamp, random)
}

pub fn generate_uid() -> String {
    generate_compact_id("USR")
}

pub fn generate_order_no() -> String {
    let time = Utc::now().format("%Y%m%d%H%M%S").to_string();
    let mut rng = rand::thread_rng();
    let random: u32 = rng.gen();
    format!("ORD{}{:06X}", time, random)
}

pub fn generate_key_id() -> String {
    generate_compact_id("KEY")
}

pub fn generate_package_id() -> String {
    generate_compact_id("PKG")
}

pub fn generate_request_id() -> String {
    let time = Utc::now().format("%Y%m%d%H%M%S").to_string();
    let mut rng = rand::thread_rng();
    let random: u32 = rng.gen();
    format!("REQ{}{:06X}", time, random)
}

pub fn generate_workspace_id() -> String {
    generate_compact_id("WS")
}

pub fn generate_project_id() -> String {
    generate_compact_id("PRJ")
}

#[cfg(test)]
mod tests {
    use super::{
        generate_key_id, generate_package_id, generate_project_id, generate_uid,
        generate_workspace_id,
    };

    #[test]
    fn compact_ids_fit_database_columns() {
        for id in [
            generate_uid(),
            generate_key_id(),
            generate_package_id(),
            generate_workspace_id(),
            generate_project_id(),
        ] {
            assert!(id.len() <= 20);
        }
    }
}
