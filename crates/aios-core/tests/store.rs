//! Storage primitives: JSON documents and the JSONL append log.

use aios_core::store::{AppendLog, DocStore};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Thing {
    name: String,
    count: u32,
}

fn temp(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aios-store-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn round_trips_a_document() {
    let store = DocStore::new(temp("round"));
    let thing = Thing {
        name: "one".into(),
        count: 3,
    };
    store.put("things", "one", &thing).unwrap();

    assert_eq!(store.get::<Thing>("things", "one").unwrap(), Some(thing));
    assert_eq!(store.get::<Thing>("things", "absent").unwrap(), None);
}

#[test]
fn stores_the_wire_type_verbatim_under_an_envelope() {
    // The point of the envelope is that `schemaVersion` never leaks into the
    // wire type — the API contract must not gain a storage field.
    let root = temp("envelope");
    let store = DocStore::new(&root);
    store
        .put(
            "things",
            "x",
            &Thing {
                name: "n".into(),
                count: 1,
            },
        )
        .unwrap();

    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("things/x.json")).unwrap())
            .unwrap();
    assert_eq!(raw["schemaVersion"], 1);
    assert_eq!(raw["kind"], "things");
    assert_eq!(raw["data"], serde_json::json!({ "name": "n", "count": 1 }));
}

#[test]
fn refuses_documents_from_a_newer_schema() {
    // Silently dropping fields a newer build added would lose them on the next
    // write, so reading forward is an error rather than a guess.
    let root = temp("newer");
    let store = DocStore::new(&root);
    std::fs::create_dir_all(root.join("things")).unwrap();
    std::fs::write(
        root.join("things/future.json"),
        r#"{"schemaVersion":99,"kind":"things","data":{"name":"n","count":1}}"#,
    )
    .unwrap();

    let err = store.get::<Thing>("things", "future").unwrap_err();
    assert!(err.to_string().contains("newer aios"), "got {err}");
}

#[test]
fn rejects_ids_that_would_escape_the_collection() {
    // Slugs reach here from user and agent input.
    let store = DocStore::new(temp("escape"));
    for evil in ["../outside", "..", ".", "", "a/b", "with space"] {
        assert!(
            store
                .put(
                    "things",
                    evil,
                    &Thing {
                        name: "x".into(),
                        count: 0
                    }
                )
                .is_err(),
            "{evil:?} should have been refused"
        );
    }
}

#[test]
fn leaves_no_temp_files_behind_and_list_ignores_non_json() {
    let root = temp("tidy");
    let store = DocStore::new(&root);
    for id in ["a", "b", "c"] {
        store
            .put(
                "things",
                id,
                &Thing {
                    name: id.into(),
                    count: 1,
                },
            )
            .unwrap();
    }
    std::fs::write(root.join("things/README.txt"), "not a document").unwrap();

    assert_eq!(store.list::<Thing>("things").unwrap().len(), 3);
    let strays: Vec<_> = std::fs::read_dir(root.join("things"))
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().to_string_lossy().contains(".tmp"))
        .collect();
    assert!(
        strays.is_empty(),
        "atomic write left temp files: {strays:?}"
    );
}

#[test]
fn a_corrupt_document_names_itself_rather_than_vanishing() {
    // Dropping a hand-broken project from the registry silently would be far
    // more confusing than an error naming the file.
    let root = temp("corrupt");
    let store = DocStore::new(&root);
    std::fs::create_dir_all(root.join("things")).unwrap();
    std::fs::write(root.join("things/bad.json"), "{ not json").unwrap();

    let err = store.list::<Thing>("things").unwrap_err();
    assert!(err.to_string().contains("bad.json"), "got {err}");
}

#[test]
fn delete_reports_whether_it_existed() {
    let store = DocStore::new(temp("delete"));
    store
        .put(
            "things",
            "gone",
            &Thing {
                name: "g".into(),
                count: 0,
            },
        )
        .unwrap();
    assert!(store.delete("things", "gone").unwrap());
    assert!(!store.delete("things", "gone").unwrap());
}

#[test]
fn log_assigns_gapless_monotonic_sequences() {
    let log = AppendLog::new(temp("seq").join("events.jsonl"));
    for i in 1..=5u32 {
        let seq = log
            .append(
                &Thing {
                    name: "e".into(),
                    count: i,
                },
                "2026-08-24T00:00:00Z",
            )
            .unwrap();
        assert_eq!(seq, i as u64);
    }
    assert_eq!(log.last_seq().unwrap(), 5);
}

#[test]
fn log_resumes_numbering_across_reopen() {
    // The next sequence comes from the file, not from memory, so a restarted
    // process cannot restart the numbering and silently overwrite history.
    let path = temp("resume").join("events.jsonl");
    AppendLog::new(&path)
        .append(
            &Thing {
                name: "a".into(),
                count: 1,
            },
            "t",
        )
        .unwrap();
    let seq = AppendLog::new(&path)
        .append(
            &Thing {
                name: "b".into(),
                count: 2,
            },
            "t",
        )
        .unwrap();
    assert_eq!(seq, 2);
}

#[test]
fn read_since_is_the_replay_cursor() {
    let log = AppendLog::new(temp("since").join("events.jsonl"));
    for i in 1..=10u32 {
        log.append(
            &Thing {
                name: "e".into(),
                count: i,
            },
            "t",
        )
        .unwrap();
    }

    let all = log.read_since::<Thing>(0, 100).unwrap();
    assert_eq!(all.len(), 10);

    // A client that saw up to 7 asks for the rest — this is the §13.2 case.
    let rest = log.read_since::<Thing>(7, 100).unwrap();
    assert_eq!(rest.len(), 3);
    assert_eq!(rest[0].seq, 8);
    assert_eq!(rest[0].data.count, 8);

    assert_eq!(log.read_since::<Thing>(0, 4).unwrap().len(), 4);
    assert!(log.read_since::<Thing>(10, 100).unwrap().is_empty());
}

#[test]
fn a_torn_trailing_line_does_not_destroy_the_log() {
    // This is the shape a power loss leaves. Serving nothing because the last
    // record was cut short would turn a recoverable crash into an unusable log.
    let path = temp("torn").join("events.jsonl");
    let log = AppendLog::new(&path);
    for i in 1..=3u32 {
        log.append(
            &Thing {
                name: "e".into(),
                count: i,
            },
            "t",
        )
        .unwrap();
    }
    let mut raw = std::fs::read_to_string(&path).unwrap();
    raw.push_str("{\"seq\":4,\"at\":\"t\",\"data\":{\"na");
    std::fs::write(&path, raw).unwrap();

    let records = log.read_since::<Thing>(0, 100).unwrap();
    assert_eq!(records.len(), 3, "intact records must still be readable");
    // And the next append must not reuse a sequence number.
    assert_eq!(
        log.append(
            &Thing {
                name: "e".into(),
                count: 5
            },
            "t"
        )
        .unwrap(),
        4
    );
}

#[test]
fn missing_log_reads_as_empty_rather_than_erroring() {
    let log = AppendLog::new(temp("missing").join("nope.jsonl"));
    assert_eq!(log.last_seq().unwrap(), 0);
    assert!(log.read_since::<Thing>(0, 10).unwrap().is_empty());
}

#[test]
fn accepts_a_hand_written_document_without_an_envelope() {
    // Being able to write these files by hand is the point of storing JSON.
    let root = temp("handwritten");
    let store = DocStore::new(&root);
    std::fs::create_dir_all(root.join("things")).unwrap();
    std::fs::write(
        root.join("things/mine.json"),
        r#"{"name":"by hand","count":7}"#,
    )
    .unwrap();

    assert_eq!(
        store.get::<Thing>("things", "mine").unwrap(),
        Some(Thing {
            name: "by hand".into(),
            count: 7
        })
    );
}

#[test]
fn adopts_a_hand_written_document_on_the_next_write() {
    // Liberal in what it accepts, strict in what it emits — otherwise the bare
    // form becomes a second format to support forever.
    let root = temp("adopt");
    let store = DocStore::new(&root);
    std::fs::create_dir_all(root.join("things")).unwrap();
    std::fs::write(root.join("things/m.json"), r#"{"name":"x","count":1}"#).unwrap();

    let thing: Thing = store.get("things", "m").unwrap().unwrap();
    store.put("things", "m", &thing).unwrap();

    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("things/m.json")).unwrap())
            .unwrap();
    assert_eq!(raw["schemaVersion"], 1);
}

#[test]
fn a_broken_hand_edit_reports_the_useful_error() {
    // The envelope error would say "missing field `data`", which tells someone
    // editing the file nothing. They need the error about their own JSON.
    let root = temp("brokenedit");
    let store = DocStore::new(&root);
    std::fs::create_dir_all(root.join("things")).unwrap();
    std::fs::write(
        root.join("things/b.json"),
        r#"{"name":"x","count":"not a number"}"#,
    )
    .unwrap();

    let err = store.get::<Thing>("things", "b").unwrap_err().to_string();
    assert!(err.contains("b.json"), "should name the file: {err}");
    assert!(
        !err.contains("`data`"),
        "should not leak the envelope: {err}"
    );
}

#[test]
fn list_with_ids_reports_the_filename_not_a_field() {
    let root = temp("ids");
    let store = DocStore::new(&root);
    store
        .put(
            "things",
            "filed-as",
            &Thing {
                name: "called".into(),
                count: 1,
            },
        )
        .unwrap();

    let listed = store.list_with_ids::<Thing>("things").unwrap();
    assert_eq!(listed[0].0, "filed-as");
    assert_eq!(listed[0].1.name, "called");
}
