//! A tiny crate used to prove Cargo validation runs.

/// Adds two integers.
pub fn add(left: i32, right: i32) -> i32 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::add;

    #[test]
    fn adds() {
        assert_eq!(add(1, 1), 2);
    }
}
