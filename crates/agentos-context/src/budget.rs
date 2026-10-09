//! Token-budget allocation.
//!
//! Lower priority numbers are kept longer. Within one priority, a higher score is kept first.
//! Ties break by item key so the same inputs always drop the same items.

use std::collections::BTreeSet;

/// One budgetable piece of a Context Pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetItem {
    /// Stable key (`node:` or `fragment:` plus an id).
    pub key: String,
    /// 1 is kept the longest. 8 is dropped first.
    pub priority: u8,
    /// Higher is kept first inside one priority.
    pub score: u32,
    /// Token estimate. Zero-cost items always fit.
    pub tokens: u32,
}

/// Keys that fit in `limit`. `None` keeps every item.
pub fn allocate(items: &[BudgetItem], limit: Option<u32>) -> BTreeSet<String> {
    let mut ordered: Vec<&BudgetItem> = items.iter().collect();
    ordered.sort_by(|left, right| {
        left.priority
            .cmp(&right.priority)
            .then(right.score.cmp(&left.score))
            .then(left.key.cmp(&right.key))
    });
    let mut kept = BTreeSet::new();
    let mut used = 0u32;
    for item in ordered {
        let next = used.saturating_add(item.tokens);
        if limit.is_some_and(|limit| next > limit) {
            continue;
        }
        used = next;
        kept.insert(item.key.clone());
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(priority: u8) -> BudgetItem {
        BudgetItem {
            key: format!("p{priority}"),
            priority,
            score: 1,
            tokens: 10,
        }
    }

    #[test]
    fn drops_from_the_bottom_of_the_documented_order() {
        let items: Vec<BudgetItem> = (1..=8).map(item).collect();
        let kept = allocate(&items, Some(35));
        assert_eq!(
            kept.into_iter().collect::<Vec<_>>(),
            vec!["p1".to_owned(), "p2".to_owned(), "p3".to_owned()]
        );

        let kept = allocate(&items, Some(70));
        assert!(!kept.contains("p8"));
        assert!(kept.contains("p7"));
        assert_eq!(kept.len(), 7);

        let kept = allocate(&items, Some(0));
        assert!(kept.is_empty());

        let kept = allocate(&items, None);
        assert_eq!(kept.len(), 8);
    }

    #[test]
    fn within_one_priority_a_lower_score_drops_first() {
        let items = vec![
            BudgetItem {
                key: "low".to_owned(),
                priority: 6,
                score: 1,
                tokens: 10,
            },
            BudgetItem {
                key: "high".to_owned(),
                priority: 6,
                score: 5,
                tokens: 10,
            },
        ];
        let kept = allocate(&items, Some(10));
        assert!(kept.contains("high"));
        assert!(!kept.contains("low"));
    }
}
