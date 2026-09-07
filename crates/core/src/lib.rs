pub mod baseline;
pub mod config;
pub mod finding;
pub mod index;
pub mod lang;
pub mod parse;
pub mod symbols;
pub mod walk;

pub const ENGINE_NAME: &str = "locrin";

#[cfg(test)]
mod tests {
    #[test]
    fn engine_has_a_name() {
        assert_eq!(super::ENGINE_NAME, "locrin");
    }
}
