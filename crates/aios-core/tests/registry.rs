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

/// Write a project document directly, the way a person editing `~/.aios` would.
fn handwrite(root: &std::path::Path, filename: &str, slug: &str, path: &str) {
    std::fs::create_dir_all(root.join("projects")).unwrap();
    std::fs::write(
        root.join("projects").join(format!("{filename}.json")),
        format!(
            r#"{{"id":"01TEST00000000000000000000","slug":"{slug}","name":"n","path":"{path}",
               "gitRemote":null,"defaultBranch":null,"languages":[],"packageManager":null,
               "issuePrefix":null,"tags":[],
               "createdAt":"2026-08-24T00:00:00Z","updatedAt":"2026-08-24T00:00:00Z"}}"#
        ),
    )
    .unwrap();
}

fn registry_root(name: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("aios-reg-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn validate_reports_a_filename_that_disagrees_with_its_slug() {
    let root = registry_root("mismatch");
    let dir = temp_dir("mismatch-target");
    handwrite(
        &root,
        "filed-here",
        "says-there",
        &dir.display().to_string(),
    );

    let problems = Registry::at(&root).validate();
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(problems[0].detail.contains("filed-here"));
    assert!(problems[0].detail.contains("says-there"));
}

#[test]
fn validate_reports_a_path_that_no_longer_exists() {
    let root = registry_root("missingpath");
    handwrite(&root, "gone", "gone", "/definitely/not/here");

    let problems = Registry::at(&root).validate();
    assert!(
        problems
            .iter()
            .any(|p| p.detail.contains("no longer exists")),
        "{problems:?}"
    );
}

#[test]
fn validate_reports_two_projects_claiming_one_directory() {
    let root = registry_root("dupepath");
    let dir = temp_dir("dupepath-target").display().to_string();
    handwrite(&root, "one", "one", &dir);
    handwrite(&root, "two", "two", &dir);

    let problems = Registry::at(&root).validate();
    assert!(
        problems
            .iter()
            .any(|p| p.detail.contains("also registered as")),
        "{problems:?}"
    );
}

#[test]
fn a_mismatched_document_is_updated_in_place_and_not_forked() {
    // Writing by `project.slug` instead of the filename would create a second
    // document and leave the original stale.
    let root = registry_root("inplace");
    let dir = temp_dir("inplace-target");
    handwrite(
        &root,
        "filename",
        "different-slug",
        &dir.display().to_string(),
    );
    let reg = Registry::at(&root);

    reg.refresh("filename").unwrap();
    let files: Vec<_> = std::fs::read_dir(root.join("projects"))
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        files,
        vec!["filename.json".to_string()],
        "refresh forked a document"
    );
}

#[test]
fn a_mismatched_document_is_actually_deleted() {
    // Deleting by `project.slug` would silently miss the real file.
    let root = registry_root("deletemismatch");
    let dir = temp_dir("deletemismatch-target");
    handwrite(
        &root,
        "filename",
        "different-slug",
        &dir.display().to_string(),
    );
    let reg = Registry::at(&root);

    reg.remove("filename").unwrap();
    assert_eq!(reg.count().unwrap(), 0);
}
