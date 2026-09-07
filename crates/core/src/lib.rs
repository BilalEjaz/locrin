pub const ENGINE_NAME: &str = "locrin";

#[cfg(test)]
mod tests {
    #[test]
    fn engine_has_a_name() {
        assert_eq!(super::ENGINE_NAME, "locrin");
    }
}
