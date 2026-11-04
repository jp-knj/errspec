use errspec::mutation::ownership::{Decision, Record, Response};
fn record() -> Record {
    Record {
        version: 1,
        run_id: "errspecmutationtest".into(),
        token: "a".repeat(64),
        owner: 123,
        supervisor: 456,
        socket: "/tmp/owned.sock".into(),
    }
}
fn response() -> Response {
    Response {
        version: 1,
        run_id: "errspecmutationtest".into(),
        owner: 123,
        supervisor: 456,
        group_owned: true,
        orphaned: false,
        terminating: false,
    }
}
#[test]
fn only_an_authenticated_current_group_leader_can_report_recovery() {
    let record = record();
    let mut reply = response();
    assert_eq!(record.judge(&reply), Decision::Active);
    reply.orphaned = true;
    assert_eq!(record.judge(&reply), Decision::Unverified);
    reply.terminating = true;
    assert_eq!(record.judge(&reply), Decision::Recovered);
    reply.orphaned = false;
    assert_eq!(record.judge(&reply), Decision::Unverified);
}
#[test]
fn recycled_or_malformed_identities_cannot_authorize_recovery() {
    let initial = record();
    for change in 0..5 {
        let mut reply = response();
        reply.orphaned = true;
        reply.terminating = true;
        match change {
            0 => reply.supervisor += 1,
            1 => reply.owner += 1,
            2 => reply.run_id.push('x'),
            3 => reply.version = 2,
            _ => reply.group_owned = false,
        }
        assert_eq!(initial.judge(&reply), Decision::Unverified);
    }
    for change in 0..5 {
        let mut record = record();
        match change {
            0 => record.owner = 0,
            1 => record.supervisor = record.owner,
            2 => record.run_id = "../stale".into(),
            3 => record.token.clear(),
            _ => record.version = 2,
        }
        assert!(!record.valid());
        assert_eq!(record.judge(&response()), Decision::Unverified);
    }
}
