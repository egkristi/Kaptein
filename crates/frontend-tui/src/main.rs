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
//!   q / Esc / Ctrl-C  quit

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
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};

/// A resource kind the TUI can list.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Pods,
    Deployments,
    Services,
    Nodes,
    Namespaces,
}

impl Kind {
    const ALL: [Kind; 5] = [
        Kind::Pods,
        Kind::Deployments,
        Kind::Services,
        Kind::Nodes,
        Kind::Namespaces,
    ];

    fn next(self) -> Kind {
        let i = Self::ALL.iter().position(|k| *k == self).unwrap_or(0);
        Self::ALL[(i + 1) % Self::ALL.len()]
    }

    fn gvk(self) -> GroupVersionKind {
        match self {
            Kind::Pods => GroupVersionKind::gvk("", "v1", "Pod"),
            Kind::Deployments => GroupVersionKind::gvk("apps", "v1", "Deployment"),
            Kind::Services => GroupVersionKind::gvk("", "v1", "Service"),
            Kind::Nodes => GroupVersionKind::gvk("", "v1", "Node"),
            Kind::Namespaces => GroupVersionKind::gvk("", "v1", "Namespace"),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Kind::Pods => "Pods",
            Kind::Deployments => "Deployments",
            Kind::Services => "Services",
            Kind::Nodes => "Nodes",
            Kind::Namespaces => "Namespaces",
        }
    }

    /// Whether the kind is cluster-scoped (has no namespace column).
    fn cluster_scoped(self) -> bool {
        matches!(self, Kind::Nodes | Kind::Namespaces)
    }
}

/// A tabular row (geometry-local, mirrors `kaptein_core::discovery::ResourceSummary`).
#[derive(Clone)]
struct TableRow {
    name: String,
    namespace: String,
    status: String,
    created: String,
}

#[tokio::main]
async fn main() -> io::Result<()> {
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
    let mut kind = Kind::Pods;
    let mut namespace: Option<String> = None; // None = all namespaces
    let mut sort_key: kaptein_core::discovery::SortKey = kaptein_core::discovery::SortKey::Name;
    let mut sort_descending = false;

    // An informer-backed live data plane (ADR-0006): seeded once, kept fresh by a
    // background watch task. Sorting/filtering and the table itself read the in-memory
    // plane — the TUI does *not* re-list the cluster per keystroke.
    let mut plane =
        kaptein_integration::LivePlane::new(client.clone(), kind.gvk(), namespace.clone());
    plane
        .seed()
        .await
        .map_err(|e| io::Error::other(e.to_string()))?;
    let mut watch = Some(tokio::spawn({
        let plane = plane.clone_plane();
        async move { plane.watch_loop().await }
    }));

    let mut rows: Vec<TableRow> = query_plane(&plane, sort_key, sort_descending).await?;
    let mut scroll: usize = 0;
    let mut selected: usize = 0;
    let mut detail: Option<String> = None;
    // Number of table rows visible in the current terminal (set each frame; drives the
    // scroll window instead of a hardcoded constant).
    let mut page_height: usize = 10;
    // Fuzzy-jump mode: Some(query) means the user is typing a fuzzy query. The
    // unfiltered list is preserved in `jump_master` so backspace can restore rows.
    let mut jump_query: Option<String> = None;
    let mut jump_master: Vec<TableRow> = Vec::new();
    // Command-palette mode: Some(query) means the palette is open (vim-style ':').
    let mut palette_query: Option<String> = None;

    loop {
        // Refresh from the live plane (cheap: reads the in-memory MemPlane, applying any
        // watch deltas that have landed since the last frame). No API call per keystroke.
        // Skipped while fuzzy-jump mode is active (its filtered `rows` is authoritative).
        if jump_query.is_none() && palette_query.is_none() {
            rows = query_plane(&plane, sort_key, sort_descending).await?;
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
                " {:<12} ns:{} sort:{} ({} rows) — Tab:kind  n:ns  s:sort  /:jump  ::palette  d:describe  i:diagnose  q:quit ",
                kind.label(),
                namespace.as_deref().unwrap_or("all"),
                sort_label(sort_key, sort_descending),
                rows.len()
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

            let table_rows: Vec<Row> = rows
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let base = if i == selected {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else {
                        Style::default()
                    };
                    let status_style = match r.status.as_str() {
                        "Running" | "Active" | "Ready" => Style::default().fg(Color::Green),
                        "Pending" | "ContainerCreating" => Style::default().fg(Color::Yellow),
                        _ => Style::default().fg(Color::Red),
                    };
                    Row::new(vec![
                        Cell::from(r.name.as_str()),
                        Cell::from(r.namespace.as_str()),
                        Cell::from(r.status.as_str()).style(status_style),
                        Cell::from(r.created.as_str()),
                    ])
                    .style(base)
                })
                .collect();

            let widths = [
                Constraint::Percentage(35),
                Constraint::Percentage(25),
                Constraint::Percentage(20),
                Constraint::Percentage(20),
            ];
            let table = Table::new(table_rows, widths)
                .header(Row::new(vec!["NAME", "NAMESPACE", "STATUS", "CREATED"]))
                .block(Block::default().borders(Borders::ALL));
            frame.render_stateful_widget(
                table,
                chunks[1],
                &mut ratatui::widgets::TableState::default().with_offset(scroll),
            );

            let detail_text = detail.clone().unwrap_or_else(|| {
                "Press d to describe, i to diagnose the selected resource.".into()
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
                KeyCode::Char('q') if palette_query.is_none() && jump_query.is_none() => break,
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
                    // Execute the best-matching command (or no-op if none match).
                    if let Some(q) = palette_query.as_deref()
                        && let Some(cmd) = palette_matches(q).into_iter().next()
                    {
                        execute_command(
                            cmd,
                            client,
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
                    }
                    palette_query = None;
                }
                KeyCode::Char('j') | KeyCode::Down if palette_query.is_none() => {
                    selected = (selected + 1).min(rows.len().saturating_sub(1));
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
                    selected = rows.len().saturating_sub(1);
                    scroll = selected.saturating_sub(page_height);
                }
                KeyCode::Tab if palette_query.is_none() => {
                    kind = kind.next();
                    rebuild_plane(client, &mut plane, &mut watch, kind, namespace.clone()).await?;
                    selected = 0;
                    scroll = 0;
                    detail = None;
                }
                KeyCode::Char('n') if palette_query.is_none() => {
                    namespace = cycle_namespace(client, namespace.clone()).await?;
                    rebuild_plane(client, &mut plane, &mut watch, kind, namespace.clone()).await?;
                    selected = 0;
                    scroll = 0;
                    detail = None;
                }
                KeyCode::Char('s') if palette_query.is_none() => {
                    sort_key = next_sort_key(sort_key);
                    selected = 0;
                    scroll = 0;
                }
                KeyCode::Char('S') if palette_query.is_none() => {
                    sort_descending = !sort_descending;
                    selected = 0;
                    scroll = 0;
                }
                KeyCode::Char('d') if palette_query.is_none() => {
                    if let Some(r) = rows.get(selected) {
                        detail = describe(client, kind, r).await.ok();
                    }
                }
                KeyCode::Char('i') if palette_query.is_none() => {
                    if kind == Kind::Pods
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

/// Extract a cell's display text by column index (geometry-local mapping of the
/// view-model `Row` to the TUI table; the view-model owns the *meaning* of each cell).
fn cell_text(cells: &[kaptein_integration::kaptein_viewmodel::Cell], idx: usize) -> String {
    cells
        .get(idx)
        .map(kaptein_integration::kaptein_viewmodel::cell_text)
        .unwrap_or_default()
}

/// Format a timestamp cell as a compact, local date-time string (geometry).
fn timestamp_text(cells: &[kaptein_integration::kaptein_viewmodel::Cell], idx: usize) -> String {
    match cells.get(idx) {
        Some(kaptein_integration::kaptein_viewmodel::Cell::Timestamp { millis }) => {
            format_timestamp(*millis)
        }
        _ => String::new(),
    }
}

fn format_timestamp(millis: i64) -> String {
    // `jiff::Timestamp::from_millisecond` (re-exported by `k8s-openapi`) gives a
    // readable rendering. The frontend owns *how* to display the instant; the
    // view-model owns the instant.
    k8s_openapi::jiff::Timestamp::from_millisecond(millis)
        .map(|t| t.to_string())
        .unwrap_or_default()
}

fn sort_label(key: kaptein_core::discovery::SortKey, descending: bool) -> String {
    let dir = if descending { "↓" } else { "↑" };
    let name = match key {
        kaptein_core::discovery::SortKey::Name => "name",
        kaptein_core::discovery::SortKey::Namespace => "namespace",
        kaptein_core::discovery::SortKey::Kind => "kind",
        kaptein_core::discovery::SortKey::Created => "created",
    };
    format!("{name}{dir}")
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

fn next_sort_key(key: kaptein_core::discovery::SortKey) -> kaptein_core::discovery::SortKey {
    use kaptein_core::discovery::SortKey;
    match key {
        SortKey::Name => SortKey::Namespace,
        SortKey::Namespace => SortKey::Kind,
        SortKey::Kind => SortKey::Created,
        SortKey::Created => SortKey::Name,
    }
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
    kind: &mut Kind,
    namespace: &mut Option<String>,
    sort_key: &mut kaptein_core::discovery::SortKey,
    sort_descending: &mut bool,
    plane: &mut kaptein_integration::LivePlane,
    watch: &mut Option<
        tokio::task::JoinHandle<Result<usize, kaptein_integration::IntegrationError>>,
    >,
    rows: &mut Vec<TableRow>,
    selected: &mut usize,
    scroll: &mut usize,
    detail: &mut Option<String>,
) -> io::Result<()> {
    let mut need_rebuild = false;
    match cmd {
        PaletteCommand::NextKind => {
            *kind = kind.next();
            need_rebuild = true;
        }
        PaletteCommand::NextNamespace => {
            *namespace = cycle_namespace(client, namespace.clone()).await?;
            need_rebuild = true;
        }
        PaletteCommand::CycleSort => {
            *sort_key = next_sort_key(*sort_key);
        }
        PaletteCommand::ToggleSortDirection => {
            *sort_descending = !*sort_descending;
        }
        PaletteCommand::DescribeSelected => {
            if let Some(r) = rows.get(*selected) {
                *detail = describe(client, *kind, r).await.ok();
            }
            return Ok(());
        }
        PaletteCommand::DiagnoseSelected => {
            if *kind == Kind::Pods
                && let Some(r) = rows.get(*selected)
            {
                *detail = diagnose(client, r).await.ok();
            } else {
                *detail = Some("Diagnostics are available for pods only.".into());
            }
            return Ok(());
        }
        PaletteCommand::Quit => {
            // The caller treats `Quit` by breaking out of the run loop; signal via a
            // sentinel is overkill — instead we just return and the UI stays open.
            return Ok(());
        }
    }
    if need_rebuild {
        rebuild_plane(client, plane, watch, *kind, namespace.clone()).await?;
    }
    // Re-query the (possibly rebuilt) live plane — no new API list.
    *rows = query_plane(plane, *sort_key, *sort_descending).await?;
    *selected = 0;
    *scroll = 0;
    *detail = None;
    Ok(())
}

/// Rebuild the live plane for a new (kind, namespace) and restart the watch task.
/// Sorting/filtering are re-applied by `query_plane` without a new API list.
async fn rebuild_plane(
    client: &Client,
    plane: &mut kaptein_integration::LivePlane,
    watch: &mut Option<
        tokio::task::JoinHandle<Result<usize, kaptein_integration::IntegrationError>>,
    >,
    kind: Kind,
    namespace: Option<String>,
) -> io::Result<()> {
    // Stop the old watch task (best-effort; the stream ends on abort).
    if let Some(handle) = watch.take() {
        handle.abort();
    }
    *plane = kaptein_integration::LivePlane::new(client.clone(), kind.gvk(), namespace);
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

/// Query the live plane (sort + filter in the view-model, window in the data plane) and
/// map the resulting `Page` of `Row`s into geometry-local table rows.
async fn query_plane(
    plane: &kaptein_integration::LivePlane,
    sort_key: kaptein_core::discovery::SortKey,
    descending: bool,
) -> io::Result<Vec<TableRow>> {
    let sort_column = match sort_key {
        kaptein_core::discovery::SortKey::Name => "name",
        kaptein_core::discovery::SortKey::Namespace => "namespace",
        kaptein_core::discovery::SortKey::Kind => "kind",
        kaptein_core::discovery::SortKey::Created => "created",
    };
    use kaptein_integration::kaptein_viewmodel::DataPlane as _;
    let page = plane
        .query(&kaptein_integration::kaptein_viewmodel::Query {
            start: 0,
            end: 50_000,
            sort: Some(kaptein_integration::kaptein_viewmodel::SortSpec {
                column: sort_column.to_string(),
                descending,
            }),
            filter: None,
        })
        .await
        .map_err(|e| io::Error::other(e.to_string()))?;
    Ok(page
        .rows
        .into_iter()
        .map(|r| TableRow {
            name: cell_text(&r.cells, 0),
            namespace: cell_text(&r.cells, 1),
            status: cell_text(&r.cells, 2),
            created: timestamp_text(&r.cells, 3),
        })
        .collect())
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

async fn describe(client: &Client, kind: Kind, row: &TableRow) -> io::Result<String> {
    let gvk = kind.gvk();
    let ns = if kind.cluster_scoped() || row.namespace.is_empty() {
        None
    } else {
        Some(row.namespace.as_str())
    };
    kaptein_core::describe::describe_dynamic(client, &gvk, ns, &row.name)
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
