//! Registry behaviour. Every test runs against an in-memory database, so the
//! suite never touches `~/.aios`.

use aios_core::{Registry, detect};
use aios_types::NewProject;

/// A registry rooted in a scratch directory, so the suite never touches the
/// real `~/.aios`.
fn registry(name: &str) -> Registry {
    let root = std::env::temp_dir().join(format!("aios-reg-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    Registry::at(root)
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aios-test-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::canonicalize(&dir).unwrap()
}

#[test]
fn adds_and_resolves_by_slug_id_and_path() {
    let reg = registry("resolve");
    let dir = temp_dir("resolve");
    let p = reg
        .add(NewProject {
            path: dir.display().to_string(),
            ..Default::default()
        })
        .unwrap();

    assert_eq!(reg.resolve(&p.slug).unwrap().id, p.id);
    assert_eq!(reg.resolve(p.id.as_str()).unwrap().id, p.id);
    assert_eq!(reg.resolve(&dir.display().to_string()).unwrap().id, p.id);
}

#[test]
fn rejects_the_same_directory_twice() {
    let reg = registry("dupe");
    let dir = temp_dir("dupe");
    let req = || NewProject {
        path: dir.display().to_string(),
        ..Default::default()
    };
    reg.add(req()).unwrap();

    let err = reg.add(req()).unwrap_err();
    assert!(
        matches!(err, aios_core::Error::PathAlreadyRegistered { .. }),
        "got {err:?}"
    );
}

#[test]
fn canonicalizes_so_equivalent_paths_are_one_project() {
    let reg = registry("canon");
    let dir = temp_dir("canon");
    reg.add(NewProject {
        path: dir.display().to_string(),
        ..Default::default()
    })
    .unwrap();

    // The same directory reached via a `..` traversal must collide, not duplicate.
    let indirect = dir.join("..").join(dir.file_name().unwrap());
    let err = reg
        .add(NewProject {
            path: indirect.display().to_string(),
            ..Default::default()
        })
        .unwrap_err();
    assert!(matches!(
        err,
        aios_core::Error::PathAlreadyRegistered { .. }
    ));
}

#[test]
fn rejects_a_slug_that_is_not_slug_shaped() {
    let reg = registry("badslug");
    let dir = temp_dir("badslug");
    let err = reg
        .add(NewProject {
            path: dir.display().to_string(),
            slug: Some("Not A Slug".into()),
            ..Default::default()
        })
        .unwrap_err();
    assert!(matches!(err, aios_core::Error::Invalid(_)), "got {err:?}");
}

#[test]
fn lists_filtered_by_tag() {
    let reg = registry("tags");
    let a = temp_dir("tag-a");
    let b = temp_dir("tag-b");
    reg.add(NewProject {
        path: a.display().to_string(),
        tags: vec!["work".into()],
        ..Default::default()
    })
    .unwrap();
    reg.add(NewProject {
        path: b.display().to_string(),
        tags: vec!["personal".into()],
        ..Default::default()
    })
    .unwrap();

    assert_eq!(reg.list(None).unwrap().len(), 2);
    let work = reg.list(Some("work")).unwrap();
    assert_eq!(work.len(), 1);
    assert_eq!(work[0].tags, vec!["work".to_string()]);
}

#[test]
fn removing_a_project_frees_its_slug() {
    let reg = registry("cascade");
    let dir = temp_dir("cascade");
    let p = reg
        .add(NewProject {
            path: dir.display().to_string(),
            tags: vec!["x".into(), "y".into()],
            ..Default::default()
        })
        .unwrap();
    reg.remove(&p.slug).unwrap();

    assert_eq!(reg.count().unwrap(), 0);
    // The slug must be reusable afterwards, which it is not if tag rows survive
    // and the foreign key is left dangling.
    reg.add(NewProject {
        path: dir.display().to_string(),
        ..Default::default()
    })
    .unwrap();
}

#[test]
fn slugify_collapses_separators_and_trims() {
    assert_eq!(detect::slugify("rothert.cc"), "rothert-cc");
    assert_eq!(detect::slugify("My Project!!"), "my-project");
    assert_eq!(
        detect::slugify("--leading--and--trailing--"),
        "leading-and-trailing"
    );
    assert_eq!(detect::slugify("!!!"), "");
}
