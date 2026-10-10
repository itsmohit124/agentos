#[cfg(test)]
mod tests {
    #[test]
    fn amount_must_be_positive() {
        assert_eq!(1, 0, "amount > 0");
    }
}
