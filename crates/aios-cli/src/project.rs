//! `aios project …`

use crate::render::{bold, dim, green, tilde};
use anyhow::{Context, Result};
use clap::Subcommand;

#[derive(Subcommand)]
pub enum ProjectCommand {
    /// Register a project directory
    Add {
        /// Path to the project. Defaults to the current directory.
        #[arg(default_value = ".")]
        path: String,
        /// Override the slug derived from the directory name
        #[arg(long)]
        slug: Option<String>,
        /// Override the display name
        #[arg(long)]
        name: Option<String>,
        /// Tag the project. Repeatable.
        #[arg(long = "tag", value_name = "TAG")]
        tags: Vec<String>,
        /// Show what would be registered without writing to the registry
        #[arg(long)]
        dry_run: bool,
    },
    /// List registered projects
    #[command(visible_alias = "ls")]
    List {
        /// Only projects carrying this tag
        #[arg(long)]
        tag: Option<String>,
    },
    /// Show one project in full
    Show {
        /// Slug, id, or path. Defaults to the current directory.
        #[arg(default_value = ".")]
        project: String,
    },
    /// Re-run detection over a registered project
    Refresh {
        #[arg(default_value = ".")]
        project: String,
    },
    /// Remove a project from the registry (does not touch the directory)
    #[command(visible_alias = "rm")]
    Remove { project: String },

    /// Open a project's stored document in $EDITOR
    ///
    /// These files are meant to be edited by hand. This just saves you finding
    /// the path, and checks the result afterwards.
    Edit {
        #[arg(default_value = ".")]
        project: String,
    },
}

pub fn run(cmd: ProjectCommand, json: bool) -> Result<()> {
    match cmd {
        ProjectCommand::Add {
            path,
            slug,
            name,
            tags,
            dry_run,
        } => add(path, slug, name, tags, dry_run, json),
        ProjectCommand::List { tag } => list(tag.as_deref(), json),
        ProjectCommand::Show { project } => show(&project, json),
        ProjectCommand::Refresh { project } => refresh(&project, json),
        ProjectCommand::Remove { project } => remove(&project, json),
        ProjectCommand::Edit { project } => edit(&project),
    }
}

fn add(
    path: String,
    slug: Option<String>,
    name: Option<String>,
    tags: Vec<String>,
    dry_run: bool,
    json: bool,
) -> Result<()> {
    if dry_run {
        let canonical =
            std::fs::canonicalize(&path).with_context(|| format!("{path} is not a directory"))?;
        let detection = aios_core::detect::detect(&canonical);
        if json {
            println!("{}", serde_json::to_string_pretty(&detection)?);
        } else {
            println!(
                "{} {}",
                bold("would register"),
                tilde(&canonical.display().to_string())
            );
            print_detection(&detection);
        }
        return Ok(());
    }

    let registry = aios_core::Registry::open()?;
    let project = registry.add(aios_types::NewProject {
        path,
        slug,
        name,
        tags,
    })?;
    if json {
        println!("{}", serde_json::to_string_pretty(&project)?);
    } else {
        println!(
            "{} {} {}",
            green("registered"),
            bold(&project.slug),
            dim(&tilde(&project.path))
        );
    }
    Ok(())
}

fn list(tag: Option<&str>, json: bool) -> Result<()> {
    let client = crate::client::Client::connect()?;
    let projects = client.call_capability("projects.list", serde_json::json!({ "tag": tag }))?;
    let projects = projects.as_array().cloned().unwrap_or_default();
    if json {
        println!("{}", serde_json::to_string_pretty(&projects)?);
        return Ok(());
    }
    if projects.is_empty() {
        println!(
            "{}",
            dim("no projects registered — try `aios project add <path>`")
        );
        return Ok(());
    }
    let width = projects
        .iter()
        .filter_map(|p| p["slug"].as_str().map(str::len))
        .max()
        .unwrap_or(0);
    for p in &projects {
        let languages: Vec<&str> = p["languages"]
            .as_array()
            .map(|a| a.iter().filter_map(|l| l.as_str()).collect())
            .unwrap_or_default();
        let meta = if languages.is_empty() {
            String::new()
        } else {
            format!(" [{}]", languages.join(", "))
        };
        println!(
            "{}  {}{}",
            bold(&format!(
                "{:<width$}",
                p["slug"].as_str().unwrap_or(""),
                width = width
            )),
            dim(&tilde(p["path"].as_str().unwrap_or(""))),
            dim(&meta),
        );
    }
    Ok(())
}

fn show(needle: &str, json: bool) -> Result<()> {
    let client = crate::client::Client::connect()?;
    let p = client.call_capability("projects.get", serde_json::json!({ "project": needle }))?;
    if json {
        println!("{}", serde_json::to_string_pretty(&p)?);
        return Ok(());
    }
    // Pad before colouring: ANSI escapes have no width on screen but do in a
    // format specifier, so `{:<16}` over a coloured string mis-aligns.
    let field = |k: &str, v: &str| println!("{} {v}", dim(&format!("{k:<13}")));
    let s = |k: &str| p[k].as_str().unwrap_or("—").to_string();
    let list = |k: &str| {
        let items: Vec<&str> = p[k]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();
        if items.is_empty() {
            "—".to_string()
        } else {
            items.join(", ")
        }
    };
    println!("{}", bold(&s("slug")));
    field("name", &s("name"));
    field("id", &s("id"));
    field("path", &tilde(&s("path")));
    field("remote", &s("gitRemote"));
    field("branch", &s("defaultBranch"));
    field("languages", &list("languages"));
    field("package mgr", &s("packageManager"));
    field("issue prefix", &s("issuePrefix"));
    field("tags", &list("tags"));
    Ok(())
}

fn refresh(needle: &str, json: bool) -> Result<()> {
    let registry = aios_core::Registry::open()?;
    let p = registry.refresh(needle)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&p)?);
    } else {
        println!("{} {}", green("refreshed"), bold(&p.slug));
    }
    Ok(())
}

fn remove(needle: &str, json: bool) -> Result<()> {
    let registry = aios_core::Registry::open()?;
    let p = registry.remove(needle)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&p)?);
    } else {
        println!(
            "{} {} {}",
            green("removed"),
            bold(&p.slug),
            dim("(directory untouched)")
        );
    }
    Ok(())
}

fn print_detection(d: &aios_types::ProjectDetection) {
    let field = |k: &str, v: &str| println!("{} {v}", dim(&format!("{k:<13}")));
    field("remote", d.git_remote.as_deref().unwrap_or("—"));
    field("branch", d.default_branch.as_deref().unwrap_or("—"));
    field("languages", &join_or_dash(&d.languages));
    field("package mgr", d.package_manager.as_deref().unwrap_or("—"));
    field("issue prefix", d.issue_prefix.as_deref().unwrap_or("—"));
}

/// Render a list as comma-separated, or an em dash when empty, so every field in
/// `show` occupies a line whether or not it has a value.
fn join_or_dash(items: &[String]) -> String {
    if items.is_empty() {
        "—".to_string()
    } else {
        items.join(", ")
    }
}

fn edit(needle: &str) -> Result<()> {
    let registry = aios_core::Registry::open()?;
    // By storage id, not by `project.slug`: hand-editing can make them
    // disagree, and this must open the file that actually exists.
    let (id, _) = registry.locate(needle)?;
    let path = registry.document_path(needle)?;

    // VISUAL before EDITOR is the long-standing convention: EDITOR may be a
    // line editor meant for dumb terminals.
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());

    // Through a shell, because EDITOR is routinely set to something with
    // arguments ("code --wait", "emacsclient -nw").
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("{editor} \"$1\"",))
        .arg("sh")
        .arg(&path)
        .status()
        .with_context(|| format!("could not launch {editor}"))?;

    if !status.success() {
        anyhow::bail!("{editor} exited with {status}");
    }

    // Check immediately rather than letting a typo surface three commands
    // later as a confusing failure somewhere else.
    let problems = aios_core::Registry::open()?.validate();
    let mine: Vec<_> = problems
        .iter()
        .filter(|p| p.file.ends_with(&format!("{id}.json")))
        .collect();

    if mine.is_empty() {
        println!(
            "{} {}",
            green("ok"),
            dim(&tilde(&path.display().to_string()))
        );
    } else {
        for p in mine {
            println!("{} {}", crate::render::yellow("!"), p.detail);
            println!("  {} {}", dim("fix:"), p.fix);
        }
    }
    Ok(())
}
