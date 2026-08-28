//! Kaptein TUI — the daily-driver MVP surface.
//!
//! A ratatui table over cluster resources with vim navigation, resource-kind switching,
//! namespace filtering, and a detail pane (describe + diagnostics) for the selected
//! resource. It consumes `kaptein-core`, owns only *geometry*, and recomputes nothing
//! semantic (ADR-0005).
//!
//! Keys:
//!   j/k  move selection        g/G  top/bottom
//!   <Tab>  cycle resource kind  n  cycle namespace
//!   d  describe selected        i  diagnose selected
//!   :q / Esc / Ctrl-C  quit

#![forbid(unsafe_code)]

use std::io;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use kube::Client;
use kube::core::GroupVersionKind;
// The TUI reaches `kaptein-core` only through the integration layer (layer
// dependency rule: frontend → integration → core, never frontend → core).
use kaptein_integration::kaptein_core;
use kaptein_integration::kaptein_viewmodel::{Action, ActionState};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

/// A resource kind the TUI can list. A kind is either one of the built-in kinds, or a
/// **lens-driven** kind (M2.2): a discovered CRD whose lens supplies its columns, so a
/// new lens file makes its CRD navigable with no recompile.
#[derive(Clone)]
struct Kind {
    /// Human-readable kind label (shown in the status line).
    label: String,
    /// The `group/version/kind` the plane lists.
    gvk: GroupVersionKind,
    /// Whether the kind is cluster-scoped (has no namespace column).
    cluster_scoped: bool,
    /// The rendered column headers (geometry): fixed for built-ins, the lens's column
    /// ids for a lens-driven kind.
    headers: Vec<String>,
    /// Index of the name column (for describe/jump navigation).
    name_col: usize,
    /// Index of the namespace column, or `None` when cluster-scoped.
    namespace_col: Option<usize>,
    /// Index of the status column (drives cell color), if any.
    status_col: Option<usize>,
    /// The lens driving this kind (its columns become the plane's schema); `None` for
    /// built-in kinds.
    lens: Option<kaptein_viewmodel::ViewDefinition>,
}

impl Kind {
    fn builtin(label: &str, group: &str, version: &str, kind: &str, cluster_scoped: bool) -> Self {
        Self {
            label: label.to_string(),
            gvk: GroupVersionKind::gvk(group, version, kind),
            cluster_scoped,
            headers: vec![
                "NAME".into(),
                "NAMESPACE".into(),
                "STATUS".into(),
                "CREATED".into(),
            ],
            name_col: 0,
            namespace_col: if cluster_scoped { None } else { Some(1) },
            status_col: Some(2),
            lens: None,
        }
    }

    /// Build a lens-driven kind from a validated [`kaptein_viewmodel::ViewDefinition`]
    /// (M2.2). The lens's columns become the plane's schema and the table's headers.
    fn from_lens(vd: kaptein_viewmodel::ViewDefinition) -> Self {
        let headers: Vec<String> = vd
            .columns
            .iter()
            .map(|c| c.id.to_ascii_uppercase())
            .collect();
        let name_col = vd
            .columns
            .iter()
            .position(|c| c.field.as_deref() == Some("metadata.name"))
            .unwrap_or(0);
        let namespace_col = vd
            .columns
            .iter()
            .position(|c| c.field.as_deref() == Some("metadata.namespace"));
        let status_col = vd
            .columns
            .iter()
            .position(|c| c.kind == kaptein_viewmodel::ColumnKind::Status);
        let cluster_scoped = namespace_col.is_none();
        let label = vd.target.kind.clone();
        Self {
            label,
            gvk: GroupVersionKind::gvk(&vd.target.group, &vd.target.version, &vd.target.kind),
            cluster_scoped,
            headers,
            name_col,
            namespace_col,
            status_col,
            lens: Some(vd),
        }
    }

    /// Whether this kind is the built-in `Pod` kind (the only kind with diagnostics).
    fn is_pods(&self) -> bool {
        self.gvk.kind == "Pod"
    }
}

/// The built-in kinds, in Tab order.
fn builtin_kinds() -> Vec<Kind> {
    vec![
        Kind::builtin("Pods", "", "v1", "Pod", false),
        Kind::builtin("Deployments", "apps", "v1", "Deployment", false),
        Kind::builtin("Services", "", "v1", "Service", false),
        Kind::builtin("Nodes", "", "v1", "Node", true),
        Kind::builtin("Namespaces", "", "v1", "Namespace", true),
    ]
}

/// Discover lens-driven kinds (M2.2): walk the extension directory, keep enabled lens
/// extensions, and load each lens's full `ViewDefinition` so its CRD becomes navigable
/// with no recompile. A lens that fails to load is skipped (reported to stderr), never
/// silently added.
fn discover_lens_kinds() -> Vec<Kind> {
    let dir = std::env::var("KAPTEIN_EXTENSIONS_DIR").unwrap_or_else(|_| "extensions".to_string());
    let (lenses, problems) = kaptein_core::extension::discover_lenses(std::path::Path::new(&dir));
    for p in &problems {
        eprintln!("lens discovery: {p}");
    }
    let config = kaptein_core::config::load();
    let mut kinds = Vec::new();
    for lens in lenses {
        if !config.extensions.is_enabled(&lens.id) {
            continue;
        }
        match kaptein_integration::load_lens(&lens.entrypoint) {
            Ok(vd) => kinds.push(Kind::from_lens(vd)),
            Err(e) => eprintln!("skipping lens {}: {e}", lens.id),
        }
    }
    kinds
}

/// The full kind list: built-ins first, then discovered lens kinds.
fn all_kinds() -> Vec<Kind> {
    let mut kinds = builtin_kinds();
    kinds.extend(discover_lens_kinds());
    kinds
}

/// Advance to the next kind in Tab order (wrapping), matched by identity.
fn next_kind(kinds: &[Kind], current: &Kind) -> Kind {
    let i = kinds
        .iter()
        .position(|k| k.label == current.label && k.gvk == current.gvk)
        .unwrap_or(0);
    kinds[(i + 1) % kinds.len()].clone()
}

/// Which column the table sorts by. This is a frontend-local *navigation* choice — an
/// index into the current plane's schema (the view-model owns which columns exist and
/// their order; the frontend owns which index is active — issue #32). The sort key is
/// resolved to the view-model `SortSpec` column id in `query_plane`.
#[derive(Clone, Copy, PartialEq)]
struct SortColumn(usize);

impl SortColumn {
    /// Advance the sort column through the plane's schema (wrapping).
    fn next(self, column_count: usize) -> SortColumn {
        SortColumn((self.0 + 1) % column_count.max(1))
    }
}

/// A tabular row (geometry-local, mirrors the render contract's `Row`). The cells are
/// the *display* text of the row's cells, in the view-model's column order — so a
/// lens-driven kind with N columns has N cells, and the built-in four-column view has 4.
#[derive(Clone)]
struct TableRow {
    name: String,
    namespace: String,
    cells: Vec<String>,
}

/// Run the TUI against the default kubeconfig context.
///
/// This is the library entry point: `kaptein tui` calls it from the single `kaptein`
/// binary. It resolves the client, enters raw/alternate mode, runs the event loop, and
/// restores the terminal on every exit path.
pub async fn run() -> io::Result<()> {
    let client = kaptein_core::discovery::client()
        .await
        .map_err(|e| io::Error::other(e.to_string()))?;
    run_ui(&client).await
}

async fn run_ui(client: &Client) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Run the event loop; restore the terminal on *every* exit path (including `?`
    // errors), so a broken terminal is never left behind.
    let result = run_event_loop(client, &mut terminal).await;
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    result
}

async fn run_event_loop(
    client: &Client,
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> io::Result<()> {
    // Discover kinds once at startup: built-ins first, then enabled lens kinds (M2.2) —
    // a discovered CRD with a lens becomes navigable with no recompile.
    let kinds = all_kinds();
    let mut kind = kinds
        .first()
        .cloned()
        .unwrap_or_else(|| Kind::builtin("Pods", "", "v1", "Pod", false));
    let mut namespace: Option<String> = None; // None = all namespaces
    let mut sort_key = SortColumn(0);
    let mut sort_descending = false;

    // An informer-backed live data plane (ADR-0006): seeded once, kept fresh by a
    // background watch task. Sorting/filtering and the table itself read the in-memory
    // plane — the TUI does *not* re-list the cluster per keystroke.
    let mut plane = new_plane(client, &kind, namespace.clone());
    plane
        .seed()
        .await
        .map_err(|e| io::Error::other(e.to_string()))?;
    let mut watch = Some(tokio::spawn({
        let plane = plane.clone_plane();
        async move { plane.watch_loop().await }
    }));

    let (mut rows, mut total) = query_plane(&plane, &kind, sort_key, sort_descending).await?;
    let mut scroll: usize = 0;
    let mut selected: usize = 0;
    let mut detail: Option<String> = None;
    // The selected resource's RBAC-preflighted action graph (M2.2 "per-action RBAC
    // grey-out"): computed once per (kind, namespace), so an action the operator is not
    // permitted to take is greyed out *before* they try it, not after a 403.
    let mut actions = preflight_actions_for(client, &kind, namespace.as_deref()).await;
    // Number of table rows visible in the current terminal (set each frame; drives the
    // scroll window instead of a hardcoded constant).
    let mut page_height: usize = 10;
    // Fuzzy-jump mode: Some(query) means the user is typing a fuzzy query. The
    // unfiltered list is preserved in `jump_master` so backspace can restore rows.
    let mut jump_query: Option<String> = None;
    let mut jump_master: Vec<TableRow> = Vec::new();
    // Command-palette mode: Some(query) means the palette is open (vim-style ':').
    let mut palette_query: Option<String> = None;
    // The last-observed MemPlane revision: the table is only re-queried (a full
    // clone+sort of the row set) when a watch delta actually landed, not every ~10 Hz
    // frame — the fix for issue #28's "query_plane still asks for 50k rows at 10 Hz".
    let mut last_revision = plane.mem().revision();

    loop {
        // Refresh from the live plane only when its revision advanced (a watch delta
        // landed). No API call per keystroke and no redundant per-frame clone+sort.
        // Skipped while fuzzy-jump mode is active (its filtered `rows` is authoritative).
        if jump_query.is_none() && palette_query.is_none() {
            let rev = plane.mem().revision();
            if rev != last_revision {
                let (new_rows, new_total) =
                    query_plane(&plane, &kind, sort_key, sort_descending).await?;
                rows = new_rows;
                total = new_total;
                last_revision = rev;
            }
        }

        let status_line = if let Some(q) = palette_query.as_deref() {
            let matches = palette_matches(q);
            format!(
                " :{q} — {} commands (Enter:run  Esc:cancel) ",
                matches.len()
            )
        } else if let Some(q) = jump_query.as_deref() {
            format!(
                " jump:/{q} — {} rows (Enter:accept  Esc:cancel) ",
                rows.len()
            )
        } else {
            format!(
                " {:<12} ns:{} sort:{} ({} rows) — Tab:kind  n:ns  s:sort  /:jump  ::palette  d:describe  i:diagnose  :q:quit ",
                kind.label,
                namespace.as_deref().unwrap_or("all"),
                sort_label(&kind.headers, sort_key, sort_descending),
                total
            )
        };

        terminal.draw(|frame| {
            let area = frame.area();
            let chunks = Layout::vertical([
                Constraint::Length(3),
                Constraint::Percentage(65),
                Constraint::Min(0),
            ])
            .split(area);

            // The table body is `chunks[1]`; its height (minus the header row + borders)
            // is the number of rows we can show — drive scrolling from the real terminal.
            let body_height = chunks[1].height.saturating_sub(3);
            page_height = (body_height as usize).max(1);

            let header = Block::default().title(status_line).borders(Borders::ALL);
            frame.render_widget(header, chunks[0]);

            // Materialize only the visible window (virtualization): the table renders
            // `scroll..scroll+page_height`, so allocating ratatui `Row`s for all 50k
            // objects every frame is wasted work. This is the change that keeps the
            // M1.8 perf budget reachable.
            let view_start = scroll.min(rows.len());
            let view_end = (scroll + page_height).min(rows.len()).max(view_start);
            let table_rows: Vec<Row> = rows[view_start..view_end]
                .iter()
                .enumerate()
                .map(|(offset, r)| {
                    let absolute = view_start + offset;
                    let base = if absolute == selected {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else {
                        Style::default()
                    };
                    // The status column (index `kind.status_col`) drives cell color; the
                    // level→color mapping is a frontend geometry choice (the view-model
                    // owns the status *meaning*). For lens kinds, the cell text is the
                    // lens-inferred status label.
                    let cells: Vec<Cell> = r
                        .cells
                        .iter()
                        .enumerate()
                        .map(|(i, text)| {
                            let cell = Cell::from(text.as_str());
                            if Some(i) == kind.status_col {
                                cell.style(status_style(text))
                            } else {
                                cell
                            }
                        })
                        .collect();
                    Row::new(cells).style(base)
                })
                .collect();

            let widths = column_widths(&kind.headers);
            let header_cells: Vec<Cell> = kind
                .headers
                .iter()
                .map(|h| Cell::from(h.as_str()))
                .collect();
            let table = Table::new(table_rows, widths)
                .header(Row::new(header_cells))
                .block(Block::default().borders(Borders::ALL));
            // The rows are already the visible window, so no `.with_offset` is needed —
            // the selection highlight is computed against the absolute index above.
            frame.render_stateful_widget(
                table,
                chunks[1],
                &mut ratatui::widgets::TableState::default(),
            );

            let detail_text = detail.clone().unwrap_or_else(|| {
                // Surface the selected resource's RBAC-preflighted action graph (M2.2):
                // a lens-driven kind declares its actions in the lens; built-ins expose
                // describe + diagnose. An action the operator may not perform is greyed
                // out (`Forbidden`) *before* they try it — the preflight ran once per
                // (kind, namespace), not per keystroke. The label_key is the render
                // contract's i18n key; the TUI shows the action *id* plus a forbidden
                // marker for the ones it will refuse.
                let actions_line = if actions.is_empty() {
                    "describe, diagnose".to_string()
                } else {
                    actions
                        .iter()
                        .map(|a| {
                            let marker = if matches!(a.state, ActionState::Forbidden { .. }) {
                                " (forbidden)"
                            } else {
                                ""
                            };
                            format!("{}{marker}", a.id)
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                };
                format!(
                    "Actions: {actions_line}\nPress d to describe, i to diagnose the selected resource."
                )
            });
            let detail_para = Paragraph::new(detail_text)
                .block(Block::default().title(" Detail ").borders(Borders::ALL));
            frame.render_widget(detail_para, chunks[2]);
        })?;

        if event::poll(std::time::Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            // Only react to press events — on Windows, crossterm delivers both press and
            // release, and acting on both would double every keystroke.
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Esc if palette_query.is_some() => {
                    palette_query = None;
                }
                KeyCode::Esc if jump_query.is_some() => {
                    // Cancel jump mode: restore the unfiltered list.
                    jump_query = None;
                    rows = jump_master.clone();
                    selected = 0;
                    scroll = 0;
                }
                KeyCode::Esc => break,
                KeyCode::Char('c')
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && palette_query.is_none()
                        && jump_query.is_none() =>
                {
                    break;
                }
                KeyCode::Char(':') if palette_query.is_none() && jump_query.is_none() => {
                    // Open the command palette.
                    palette_query = Some(String::new());
                }
                KeyCode::Char(c) if palette_query.is_some() && c != ':' => {
                    if let Some(q) = palette_query.as_mut() {
                        q.push(c);
                    }
                }
                KeyCode::Backspace if palette_query.is_some() => {
                    if let Some(q) = palette_query.as_mut() {
                        q.pop();
                    }
                }
                KeyCode::Enter if palette_query.is_some() => {
                    // Vim quit commands are matched exactly (before the fuzzy fallback),
                    // so `:q`, `:q!`, `:x`, and `:wq` all quit deterministically — no
                    // fuzzy ambiguity. Then fall back to the fuzzy palette.
                    let q = palette_query.as_deref().unwrap_or_default();
                    let is_quit = matches!(q.trim(), "q" | "q!" | "x" | "wq");
                    if is_quit {
                        break;
                    }
                    // Execute the best-matching command (or no-op if none match).
                    if let Some(cmd) = palette_matches(q).into_iter().next() {
                        let should_quit = execute_command(
                            cmd,
                            client,
                            &kinds,
                            &mut kind,
                            &mut namespace,
                            &mut sort_key,
                            &mut sort_descending,
                            &mut plane,
                            &mut watch,
                            &mut rows,
                            &mut selected,
                            &mut scroll,
                            &mut detail,
                        )
                        .await?;
                        palette_query = None;
                        if should_quit {
                            break;
                        }
                    } else {
                        palette_query = None;
                    }
                }
                KeyCode::Char('j') | KeyCode::Down if palette_query.is_none() => {
                    selected = (selected + 1).min(total.saturating_sub(1));
                    if selected >= scroll + page_height {
                        scroll += 1;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up if palette_query.is_none() => {
                    selected = selected.saturating_sub(1);
                    if selected < scroll {
                        scroll = selected;
                    }
                }
                KeyCode::Char('g') if palette_query.is_none() => {
                    selected = 0;
                    scroll = 0;
                }
                KeyCode::Char('G') if palette_query.is_none() => {
                    selected = total.saturating_sub(1);
                    scroll = selected.saturating_sub(page_height);
                }
                KeyCode::Tab if palette_query.is_none() => {
                    kind = next_kind(&kinds, &kind);
                    // Cluster-scoped kinds have no namespace: clear it to avoid a 404 on
                    // the namespaced list/watch.
                    if kind.cluster_scoped {
                        namespace = None;
                    }
                    rebuild_plane(client, &mut plane, &mut watch, &kind, namespace.clone()).await?;
                    let (new_rows, new_total) =
                        query_plane(&plane, &kind, sort_key, sort_descending).await?;
                    rows = new_rows;
                    total = new_total;
                    last_revision = plane.mem().revision();
                    selected = 0;
                    scroll = 0;
                    detail = None;
                    actions = preflight_actions_for(client, &kind, namespace.as_deref()).await;
                }
                KeyCode::Char('n') if palette_query.is_none() => {
                    namespace = cycle_namespace(client, namespace.clone()).await?;
                    rebuild_plane(client, &mut plane, &mut watch, &kind, namespace.clone()).await?;
                    let (new_rows, new_total) =
                        query_plane(&plane, &kind, sort_key, sort_descending).await?;
                    rows = new_rows;
                    total = new_total;
                    last_revision = plane.mem().revision();
                    selected = 0;
                    scroll = 0;
                    detail = None;
                    actions = preflight_actions_for(client, &kind, namespace.as_deref()).await;
                }
                KeyCode::Char('s') if palette_query.is_none() => {
                    sort_key = next_sort_key(sort_key, kind.headers.len());
                    selected = 0;
                    scroll = 0;
                }
                KeyCode::Char('S') if palette_query.is_none() => {
                    sort_descending = !sort_descending;
                    selected = 0;
                    scroll = 0;
                }
                KeyCode::Char('d') if palette_query.is_none() => {
                    if action_is_forbidden(&actions, "describe") {
                        detail = Some("describe is forbidden by RBAC preflight.".into());
                    } else if let Some(r) = rows.get(selected) {
                        detail = describe(client, &kind, r).await.ok();
                    }
                }
                KeyCode::Char('i') if palette_query.is_none() => {
                    if action_is_forbidden(&actions, "diagnose") {
                        detail = Some("diagnose is forbidden by RBAC preflight.".into());
                    } else if kind.is_pods()
                        && let Some(r) = rows.get(selected)
                    {
                        detail = diagnose(client, r).await.ok();
                    } else {
                        detail = Some("Diagnostics are available for pods only.".into());
                    }
                }
                KeyCode::Char('/') if palette_query.is_none() => {
                    // Enter fuzzy-jump mode (empty query = show all). Snapshot the
                    // unfiltered list so backspace can restore filtered-out rows.
                    jump_master = rows.clone();
                    jump_query = Some(String::new());
                }
                KeyCode::Char(c) if jump_query.is_some() && c != '/' => {
                    // In jump mode: append typed chars to the query and re-rank rows from
                    // the *master* list (not the already-filtered one), so backspace works.
                    if let Some(q) = jump_query.as_mut() {
                        q.push(c);
                    }
                    if let Some(q) = jump_query.as_deref() {
                        rows = fuzzy_rerank(jump_master.clone(), q);
                        selected = 0;
                        scroll = 0;
                    }
                }
                KeyCode::Backspace if jump_query.is_some() => {
                    if let Some(q) = jump_query.as_mut() {
                        q.pop();
                    }
                    if let Some(q) = jump_query.as_deref() {
                        rows = fuzzy_rerank(jump_master.clone(), q);
                        selected = 0;
                        scroll = 0;
                    }
                }
                KeyCode::Enter if jump_query.is_some() => {
                    // Exit jump mode, keeping the current (fuzzy-ranked) selection.
                    jump_query = None;
                }
                _ => {}
            }
        }
    }

    Ok(())
}

/// Map a status display text to a color (frontend geometry; the view-model owns the
/// status *meaning*).
fn status_style(status: &str) -> Style {
    match status {
        "Running" | "Active" | "Ready" => Style::default().fg(Color::Green),
        "Pending" | "ContainerCreating" => Style::default().fg(Color::Yellow),
        _ => Style::default().fg(Color::Red),
    }
}

/// Equal-width column constraints (geometry) for the table, one per header.
fn column_widths(headers: &[String]) -> Vec<Constraint> {
    headers
        .iter()
        .map(|_| Constraint::Percentage(100 / headers.len().max(1) as u16))
        .collect()
}

/// Extract a cell's display text by column index (geometry-local mapping of the
/// view-model `Row` to the TUI table; the view-model owns the *meaning* of each cell).
fn cell_text(cells: &[kaptein_integration::kaptein_viewmodel::Cell], idx: usize) -> String {
    cells
        .get(idx)
        .map(kaptein_integration::kaptein_viewmodel::cell_text)
        .unwrap_or_default()
}

fn format_timestamp(millis: i64) -> String {
    // `jiff::Timestamp::from_millisecond` (re-exported by `k8s-openapi`) gives a
    // readable rendering. The frontend owns *how* to display the instant; the
    // view-model owns the instant.
    k8s_openapi::jiff::Timestamp::from_millisecond(millis)
        .map(|t| t.to_string())
        .unwrap_or_default()
}

/// The sort indicator for the status line: the active column's id (from the plane's
/// schema) plus a direction arrow.
fn sort_label(column_ids: &[String], key: SortColumn, descending: bool) -> String {
    let dir = if descending { "↓" } else { "↑" };
    let col = column_ids.get(key.0).map(|s| s.as_str()).unwrap_or("?");
    format!("{col}{dir}")
}

/// Re-rank rows by fuzzy-jump score against `query`, dropping non-matches. Uses the
/// shared view-model matcher (renderer-agnostic semantic, ADR-0005).
fn fuzzy_rerank(rows: Vec<TableRow>, query: &str) -> Vec<TableRow> {
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    let ranked = kaptein_viewmodel::fuzzy_jump(names, query);
    let order: std::collections::HashMap<&str, usize> = ranked
        .iter()
        .enumerate()
        .map(|(i, m)| (m.candidate.as_str(), i))
        .collect();
    let mut out: Vec<TableRow> = rows
        .into_iter()
        .filter(|r| order.contains_key(r.name.as_str()))
        .collect();
    out.sort_by_key(|r| order.get(r.name.as_str()).copied().unwrap_or(usize::MAX));
    out
}

/// Advance the sort column through the plane's schema (wrapping).
fn next_sort_key(key: SortColumn, column_count: usize) -> SortColumn {
    key.next(column_count)
}

/// A command-palette action. The palette lists these and fuzzy-matches the typed query;
/// selecting one performs the action (same as the single-key bindings).
#[derive(Clone, Copy, PartialEq)]
enum PaletteCommand {
    NextKind,
    NextNamespace,
    CycleSort,
    ToggleSortDirection,
    DescribeSelected,
    DiagnoseSelected,
    Quit,
}

impl PaletteCommand {
    const ALL: [PaletteCommand; 7] = [
        PaletteCommand::NextKind,
        PaletteCommand::NextNamespace,
        PaletteCommand::CycleSort,
        PaletteCommand::ToggleSortDirection,
        PaletteCommand::DescribeSelected,
        PaletteCommand::DiagnoseSelected,
        PaletteCommand::Quit,
    ];

    fn label(self) -> &'static str {
        match self {
            PaletteCommand::NextKind => "next-kind",
            PaletteCommand::NextNamespace => "next-namespace",
            PaletteCommand::CycleSort => "cycle-sort",
            PaletteCommand::ToggleSortDirection => "toggle-sort-direction",
            PaletteCommand::DescribeSelected => "describe-selected",
            PaletteCommand::DiagnoseSelected => "diagnose-selected",
            PaletteCommand::Quit => "quit",
        }
    }
}

/// Fuzzy-match the typed palette query against the command labels, best-first.
fn palette_matches(query: &str) -> Vec<PaletteCommand> {
    let labels: Vec<&str> = PaletteCommand::ALL.iter().map(|c| c.label()).collect();
    let ranked = kaptein_viewmodel::fuzzy_jump(labels, query);
    let mut out: Vec<PaletteCommand> = ranked
        .into_iter()
        .filter_map(|m| {
            PaletteCommand::ALL
                .iter()
                .copied()
                .find(|c| c.label() == m.candidate)
        })
        .collect();
    // If the query is empty, preserve the canonical order.
    if query.trim().is_empty() {
        out = PaletteCommand::ALL.to_vec();
    }
    out
}

/// Execute a palette command (the same actions as the single-key bindings).
#[allow(clippy::too_many_arguments)]
async fn execute_command(
    cmd: PaletteCommand,
    client: &Client,
    kinds: &[Kind],
    kind: &mut Kind,
    namespace: &mut Option<String>,
    sort_key: &mut SortColumn,
    sort_descending: &mut bool,
    plane: &mut kaptein_integration::LivePlane,
    watch: &mut Option<tokio::task::JoinHandle<Result<(), kaptein_integration::IntegrationError>>>,
    rows: &mut Vec<TableRow>,
    selected: &mut usize,
    scroll: &mut usize,
    detail: &mut Option<String>,
) -> io::Result<bool> {
    let mut need_rebuild = false;
    match cmd {
        PaletteCommand::NextKind => {
            *kind = next_kind(kinds, kind);
            // Cluster-scoped kinds have no namespace — clear it to avoid a 404 on the
            // namespaced list/watch.
            if kind.cluster_scoped {
                *namespace = None;
            }
            need_rebuild = true;
        }
        PaletteCommand::NextNamespace => {
            *namespace = cycle_namespace(client, namespace.clone()).await?;
            need_rebuild = true;
        }
        PaletteCommand::CycleSort => {
            *sort_key = next_sort_key(*sort_key, kind.headers.len());
        }
        PaletteCommand::ToggleSortDirection => {
            *sort_descending = !*sort_descending;
        }
        PaletteCommand::DescribeSelected => {
            if let Some(r) = rows.get(*selected) {
                *detail = describe(client, kind, r).await.ok();
            }
            return Ok(false);
        }
        PaletteCommand::DiagnoseSelected => {
            if kind.is_pods()
                && let Some(r) = rows.get(*selected)
            {
                *detail = diagnose(client, r).await.ok();
            } else {
                *detail = Some("Diagnostics are available for pods only.".into());
            }
            return Ok(false);
        }
        PaletteCommand::Quit => {
            // Signal the run loop to break out of the event loop (the palette's `quit`
            // must actually quit, not just close the palette).
            return Ok(true);
        }
    }
    if need_rebuild {
        rebuild_plane(client, plane, watch, kind, namespace.clone()).await?;
    }
    // Re-query the (possibly rebuilt) live plane — no new API list.
    *rows = query_plane(plane, kind, *sort_key, *sort_descending)
        .await?
        .0;
    *selected = 0;
    *scroll = 0;
    *detail = None;
    Ok(false)
}

/// Rebuild the live plane for a new (kind, namespace) and restart the watch task.
/// Sorting/filtering are re-applied by `query_plane` without a new API list.
async fn rebuild_plane(
    client: &Client,
    plane: &mut kaptein_integration::LivePlane,
    watch: &mut Option<tokio::task::JoinHandle<Result<(), kaptein_integration::IntegrationError>>>,
    kind: &Kind,
    namespace: Option<String>,
) -> io::Result<()> {
    // Stop the old watch task (best-effort; the stream ends on abort).
    if let Some(handle) = watch.take() {
        handle.abort();
    }
    *plane = new_plane(client, kind, namespace);
    plane
        .seed()
        .await
        .map_err(|e| io::Error::other(e.to_string()))?;
    *watch = Some(tokio::spawn({
        let p = plane.clone_plane();
        async move { p.watch_loop().await }
    }));
    Ok(())
}

/// Build a `LivePlane` honoring the `[informer]` config policy (ADR-0006's configurable
/// watch cap + idle TTL), so the TUI's watch budget is operator-tunable. A lens-driven
/// kind builds a lens plane (M2.2), so its declared columns become the plane's schema.
fn new_plane(
    client: &Client,
    kind: &Kind,
    namespace: Option<String>,
) -> kaptein_integration::LivePlane {
    let policy = kaptein_core::config::load().informer.to_policy();
    match &kind.lens {
        Some(vd) => kaptein_integration::LivePlane::new_lens_with_policy(
            client.clone(),
            kind.gvk.clone(),
            namespace,
            vd.clone(),
            policy,
        ),
        None => kaptein_integration::LivePlane::new_with_policy(
            client.clone(),
            kind.gvk.clone(),
            namespace,
            policy,
        ),
    }
}

/// Query the live plane (sort + filter in the view-model, window in the data plane) and
/// map the resulting `Page` of `Row`s into geometry-local table rows. Returns the rows
/// **and** the total matching count (`page.total`), so the TUI can show "N rows" and jump
/// to the bottom (`G`) without materializing the whole set — the M1.8 windowing fix.
/// The row's cells are the plane's schema columns in order — the lens's columns for a
/// lens-driven kind, the built-in four for a built-in kind.
async fn query_plane(
    plane: &kaptein_integration::LivePlane,
    kind: &Kind,
    sort_key: SortColumn,
    descending: bool,
) -> io::Result<(Vec<TableRow>, usize)> {
    let column_ids = plane.column_ids();
    let sort_column = column_ids
        .get(sort_key.0)
        .cloned()
        .unwrap_or_else(|| "name".to_string());
    use kaptein_integration::kaptein_viewmodel::DataPlane as _;
    let page = plane
        .query(&kaptein_integration::kaptein_viewmodel::Query {
            start: 0,
            end: 50_000,
            sort: Some(kaptein_integration::kaptein_viewmodel::SortSpec {
                column: sort_column,
                descending,
            }),
            filter: None,
        })
        .await
        .map_err(|e| io::Error::other(e.to_string()))?;
    let total = page.total;
    let rows = page
        .rows
        .into_iter()
        .map(|r| {
            let name = cell_text(&r.cells, kind.name_col);
            let namespace = kind
                .namespace_col
                .and_then(|i| r.cells.get(i))
                .map(kaptein_integration::kaptein_viewmodel::cell_text)
                .unwrap_or_default();
            // The display cells: timestamps formatted, everything else as text.
            let cells: Vec<String> = r
                .cells
                .iter()
                .enumerate()
                .map(|(i, c)| match c {
                    kaptein_integration::kaptein_viewmodel::Cell::Timestamp { millis } => {
                        format_timestamp(*millis)
                    }
                    _ => cell_text(&r.cells, i),
                })
                .collect();
            TableRow {
                name,
                namespace,
                cells,
            }
        })
        .collect();
    Ok((rows, total))
}

async fn cycle_namespace(client: &Client, current: Option<String>) -> io::Result<Option<String>> {
    let namespaces =
        kaptein_core::discovery::list(client, &GroupVersionKind::gvk("", "v1", "Namespace"), None)
            .await
            .map_err(|e| io::Error::other(e.to_string()))?;
    let mut names: Vec<String> = namespaces.into_iter().map(|n| n.name).collect();
    names.sort();
    names.insert(0, String::new()); // empty = all namespaces

    let idx = current
        .as_ref()
        .and_then(|c| names.iter().position(|n| n == c))
        .unwrap_or(0);
    let next = names[(idx + 1) % names.len()].clone();
    Ok(if next.is_empty() { None } else { Some(next) })
}

async fn describe(client: &Client, kind: &Kind, row: &TableRow) -> io::Result<String> {
    let gvk = &kind.gvk;
    let ns = if kind.cluster_scoped || row.namespace.is_empty() {
        None
    } else {
        Some(row.namespace.as_str())
    };
    kaptein_core::describe::describe_dynamic(client, gvk, ns, &row.name)
        .await
        .map_err(|e| io::Error::other(e.to_string()))
}

async fn diagnose(client: &Client, row: &TableRow) -> io::Result<String> {
    let pod = kaptein_core::pods::get_pod(client, &row.namespace, &row.name)
        .await
        .map_err(|e| io::Error::other(e.to_string()))?;
    let findings = kaptein_core::diagnostics::diagnose(&pod);
    if findings.is_empty() {
        Ok(format!("{}: ready", row.name))
    } else {
        Ok(findings
            .iter()
            .map(|f| format!("{}: {}", f.code, f.summary))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

/// Build the RBAC-preflighted action graph for a kind (M2.2 per-action RBAC grey-out).
///
/// A lens-driven kind contributes its declared actions; a built-in kind contributes the
/// fixed `describe`/`diagnose` pair. Each action's verb is preflighted against the
/// current user's permissions (via the integration layer, which resolves the GVK to its
/// plural and runs one `SelfSubjectRulesReview`), and denied actions are downgraded to
/// `Forbidden`. This is the shipped path — the action graph a frontend renders is the
/// *governed* one, not the lens's optimistic declaration.
async fn preflight_actions_for(
    client: &Client,
    kind: &Kind,
    namespace: Option<&str>,
) -> Vec<Action> {
    let mut actions = match &kind.lens {
        Some(vd) => vd.actions_as_semantic(),
        None => vec![
            Action {
                id: "describe".into(),
                label_key: "action.describe".into(),
                state: ActionState::Allowed,
            },
            Action {
                id: "diagnose".into(),
                label_key: "action.diagnose".into(),
                state: ActionState::Allowed,
            },
        ],
    };
    kaptein_integration::preflight_actions(client, &kind.gvk, namespace, &mut actions).await;
    actions
}

/// Whether an action id is `Forbidden` in the preflighted graph (absent = not declared,
/// so not forbidden — the caller decides whether an undeclared action is applicable).
fn action_is_forbidden(actions: &[Action], id: &str) -> bool {
    actions
        .iter()
        .any(|a| a.id == id && matches!(a.state, ActionState::Forbidden { .. }))
}
