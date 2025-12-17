use ulid::Ulid;

pub fn new_ulid_string() -> String {
    Ulid::new().to_string()
}

pub fn is_ulid_string(s: &str) -> bool {
    Ulid::from_string(s).is_ok()
}
