use crate::context::Context;
use crate::registry::{Capability, Effect};
use aios_core::{Error, Result};
use aios_types::{
    CaptureNoteInput, Note, NoteHit, NoteRef, ReadNoteInput, Scope, ScopedInput, SearchNotesInput,
    WriteNote,
};

/// Turn the `scope` / `project` pair into one [`Scope`].
///
/// `project` is a convenience for agents, which find a slug easier to supply
/// than a tagged union. Supplying both is a caller bug and is rejected rather
/// than silently resolved in some arbitrary precedence order.
fn resolve_scope(ctx: &Context, scope: Option<Scope>, project: Option<String>) -> Result<Scope> {
    match (scope, project) {
        (Some(_), Some(_)) => Err(Error::Invalid(
            "pass either `scope` or `project`, not both".into(),
        )),
        (Some(s), None) => Ok(s),
        (None, Some(needle)) => Ok(Scope::Project {
            slug: ctx.registry.resolve(&needle)?.slug,
        }),
        (None, None) => Ok(Scope::All),
    }
}

pub fn register(items: &mut Vec<Capability>) {
    items.push(Capability::new(
        "kb.list",
        "List notes in the knowledge base",
        Effect::Read,
        |ctx: &Context, input: ScopedInput| -> Result<Vec<NoteRef>> {
            let scope = resolve_scope(ctx, input.scope, input.project)?;
            ctx.ports.knowledge.list(&scope)
        },
    ));

    items.push(Capability::new(
        "kb.search",
        "Full-text search across the knowledge base",
        Effect::Read,
        |ctx: &Context, input: SearchNotesInput| -> Result<Vec<NoteHit>> {
            let scope = resolve_scope(ctx, input.scope, input.project)?;
            ctx.ports
                .knowledge
                .search(&scope, &input.query, input.limit.unwrap_or(25))
        },
    ));

    items.push(Capability::new(
        "kb.read",
        "Read one note, with its outgoing wikilinks resolved",
        Effect::Read,
        |ctx: &Context, input: ReadNoteInput| -> Result<Note> {
            ctx.ports.knowledge.read(&input.path)
        },
    ));

    items.push(Capability::new(
        "kb.write",
        "Create or update a note. Prefer append for agent-authored content",
        Effect::Write,
        |ctx: &Context, input: WriteNote| -> Result<Note> { ctx.ports.knowledge.write(&input) },
    ));

    items.push(Capability::new(
        "kb.capture",
        "Append a quick note to the inbox without choosing a location",
        Effect::Write,
        |ctx: &Context, input: CaptureNoteInput| -> Result<Note> {
            // Capture must never require a decision: agents and phones both use
            // it mid-thought. One dated note per day, appended to.
            let today = aios_core::today();
            ctx.ports.knowledge.write(&WriteNote {
                path: format!("inbox/{today}.md"),
                body: input.body,
                title: input.title.or(Some(today)),
                tags: input.tags,
                append: true,
            })
        },
    ));
}
