use errspec::mutation::sharding::Shard;
use std::collections::BTreeSet;

fn keys() -> Vec<String> {
    (0..512u64)
        .map(|index| format!("{index:016x}{:048x}", 0))
        .collect()
}
#[test]
fn partitions_are_disjoint_complete_and_independent_of_inventory_order() {
    let keys = keys();
    let mut combined = BTreeSet::new();
    for index in 1..=7 {
        let shard = Shard { index, total: 7 };
        let (part, info) = shard.select(keys.iter().map(String::as_str)).unwrap();
        let (reversed, _) = shard.select(keys.iter().rev().map(String::as_str)).unwrap();
        assert_eq!(part, reversed);
        assert_eq!(info.eligible, 512);
        assert_eq!(info.selected + info.omitted, 512);
        assert!(part.iter().all(|key| combined.insert(key.clone())));
    }
    assert_eq!(combined, keys.into_iter().collect());
}
#[test]
fn invalid_partitions_and_corrupt_identities_are_rejected() {
    for input in [
        "0/2",
        "1/0",
        "3/2",
        "-1/2",
        "1",
        "1/2/3",
        "1.5/2",
        "1/4294967296",
    ] {
        assert!(input.parse::<Shard>().is_err(), "{input}");
    }
    let shard: Shard = "1/1".parse().unwrap();
    let keys = keys();
    assert_eq!(
        shard
            .select(keys.iter().map(String::as_str))
            .unwrap()
            .0
            .len(),
        512
    );
    for input in [
        vec!["bad".into()],
        vec!["z".repeat(64)],
        vec!["a".repeat(64), "a".repeat(64)],
    ] {
        assert!(shard.select(input.iter().map(String::as_str)).is_err());
    }
    assert!(serde_json::from_str::<Shard>(r#"{"index":1,"total":2,"extra":true}"#).is_err());
}
