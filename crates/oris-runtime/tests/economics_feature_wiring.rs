#![cfg(feature = "economics")]

fn assert_type<T>() {}

#[test]
fn economics_standard_feature_paths_resolve() {
    let policy = oris_runtime::economics::RoiPolicy::default();
    let earned = oris_runtime::economics::compute_replay_evu(600, &policy);
    assert_eq!(earned, 3);

    let mut journal = oris_runtime::economics::LedgerJournal::new("node-a");
    journal.record_replay_success("gene-a", 600, 1_700_000_000_000, &policy);
    assert_eq!(journal.balance(), 3);
    assert_eq!(journal.replay_balance(), 3);

    assert_type::<oris_runtime::economics::EvuLedger>();
    assert_type::<oris_runtime::economics::StakePolicy>();
    assert_type::<oris_runtime::economics::EconomicsSignal>();
    assert_type::<oris_runtime::economics::ValidationSettlement>();
}
