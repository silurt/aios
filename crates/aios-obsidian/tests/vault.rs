use aios_caps::ports::Knowledge;
use aios_obsidian::Vault;
use aios_types::{Scope, WriteNote};

fn vault(name: &str) -> (Vault, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("aios-vault-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for sub in ["global", "inbox", "projects/demo"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
    (Vault::new(root.clone()), root)
}

#[test]
fn reads_frontmatter_title_and_tags() {
    let (v, root) = vault("front");
    std::fs::write(
        root.join("global/a.md"),
        "---\ntitle: \"Real Title\"\ntags: [one, two]\n---\n\nbody text\n",
    )
    .unwrap();

    let note = v.read("global/a.md").unwrap();
    assert_eq!(note.meta.title, "Real Title");
    assert_eq!(note.meta.tags, vec!["one", "two"]);
    assert_eq!(note.body.trim(), "body text");
}

#[test]
fn falls_back_from_frontmatter_to_heading_to_filename() {
    let (v, root) = vault("titles");
    std::fs::write(root.join("global/heading.md"), "# From Heading\n\nx\n").unwrap();
    std::fs::write(root.join("global/bare.md"), "just text\n").unwrap();

    assert_eq!(
        v.read("global/heading.md").unwrap().meta.title,
        "From Heading"
    );
    assert_eq!(v.read("global/bare.md").unwrap().meta.title, "bare");
}

#[test]
fn extracts_wikilinks_stripping_aliases_and_anchors() {
    let (v, root) = vault("links");
    std::fs::write(
        root.join("global/l.md"),
        "See [[plain]], [[target|an alias]], [[note#section]] and [[plain]] again.\n",
    )
    .unwrap();

    // Aliases and anchors are stripped, and repeats collapse.
    assert_eq!(
        v.read("global/l.md").unwrap().links,
        vec!["plain", "target", "note"]
    );
}

#[test]
fn refuses_paths_that_escape_the_vault() {
    let (v, _root) = vault("escape");
    // Agents supply these paths, so this is a real input rather than a
    // hypothetical.
    for evil in ["../../../etc/passwd", "/etc/passwd", "global/../../x"] {
        assert!(v.read(evil).is_err(), "{evil} should have been refused");
    }
}

#[test]
fn append_preserves_existing_frontmatter() {
    let (v, root) = vault("append");
    std::fs::write(
        root.join("global/n.md"),
        "---\ntitle: \"Keep Me\"\ntags: [orig]\n---\n\nfirst\n",
    )
    .unwrap();

    v.write(&WriteNote {
        path: "global/n.md".into(),
        body: "second".into(),
        append: true,
        ..Default::default()
    })
    .unwrap();

    let raw = std::fs::read_to_string(root.join("global/n.md")).unwrap();
    assert!(raw.contains("Keep Me"), "frontmatter was destroyed: {raw}");
    assert!(raw.contains("first") && raw.contains("second"));
}

#[test]
fn write_without_append_replaces() {
    let (v, root) = vault("replace");
    let path = root.join("global/r.md");
    std::fs::write(&path, "old content\n").unwrap();

    v.write(&WriteNote {
        path: "global/r.md".into(),
        body: "new content".into(),
        title: Some("R".into()),
        ..Default::default()
    })
    .unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("old content"));
    assert!(raw.contains("new content") && raw.contains("title: \"R\""));
}

#[test]
fn write_creates_missing_directories_and_adds_extension() {
    let (v, root) = vault("mkdir");
    let note = v
        .write(&WriteNote {
            path: "projects/demo/deep/nested".into(), // no .md
            body: "x".into(),
            ..Default::default()
        })
        .unwrap();

    assert_eq!(note.meta.path, "projects/demo/deep/nested.md");
    assert!(root.join("projects/demo/deep/nested.md").is_file());
}

#[test]
fn scope_restricts_listing() {
    let (v, root) = vault("scope");
    std::fs::write(root.join("global/g.md"), "g\n").unwrap();
    std::fs::write(root.join("projects/demo/p.md"), "p\n").unwrap();
    std::fs::write(root.join("inbox/i.md"), "i\n").unwrap();

    assert_eq!(v.list(&Scope::All).unwrap().len(), 3);
    assert_eq!(v.list(&Scope::Global).unwrap().len(), 1);
    assert_eq!(v.list(&Scope::Inbox).unwrap().len(), 1);
    let project = v
        .list(&Scope::Project {
            slug: "demo".into(),
        })
        .unwrap();
    assert_eq!(project.len(), 1);
    assert_eq!(project[0].path, "projects/demo/p.md");
}

#[test]
fn ignores_dot_directories() {
    let (v, root) = vault("dots");
    std::fs::create_dir_all(root.join(".obsidian")).unwrap();
    std::fs::write(root.join(".obsidian/workspace.md"), "config\n").unwrap();
    std::fs::write(root.join("global/real.md"), "real\n").unwrap();

    // Vault configuration is not knowledge.
    let listed = v.list(&Scope::All).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].path, "global/real.md");
}

#[test]
fn search_reports_one_hit_per_note_with_line_numbers() {
    let (v, root) = vault("search");
    std::fs::write(
        root.join("global/s.md"),
        "alpha\nbeta NEEDLE here\nNEEDLE again\n",
    )
    .unwrap();
    std::fs::write(root.join("global/t.md"), "nothing\n").unwrap();

    let hits = v.search(&Scope::All, "needle", 10).unwrap();
    assert_eq!(hits.len(), 1, "should not report the same note twice");
    assert_eq!(hits[0].line, 2);
    assert_eq!(hits[0].excerpt, "beta NEEDLE here");
}
