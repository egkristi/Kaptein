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

use crossterm::event::{self, Event, KeyCode, KeyModifiers};
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

    let mut kind = Kind::Pods;
    let mut namespace: Option<String> = None; // None = all namespaces
    let mut sort_key: kaptein_core::discovery::SortKey = kaptein_core::discovery::SortKey::Name;
    let mut sort_descending = false;
    let mut rows: Vec<TableRow> = fetch(
        client,
        kind,
        namespace.as_deref(),
        sort_key,
        sort_descending,
    )
    .await?;
    let mut scroll: usize = 0;
    let mut selected: usize = 0;
    let mut detail: Option<String> = None;
    // Fuzzy-jump mode: Some(query) means the user is typing a fuzzy query.
    let mut jump_query: Option<String> = None;

    loop {
        let status_line = if let Some(q) = jump_query.as_deref() {
            format!(
                " jump:/{q} — {} rows (Enter:accept  Esc:cancel) ",
                rows.len()
            )
        } else {
            format!(
                " {:<12} ns:{} sort:{} ({} rows) — Tab:kind  n:ns  s:sort  /:jump  d:describe  i:diagnose  q:quit ",
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
            match key.code {
                KeyCode::Esc if jump_query.is_some() => {
                    // Cancel jump mode: restore the unfiltered list.
                    jump_query = None;
                    rows = fetch(
                        client,
                        kind,
                        namespace.as_deref(),
                        sort_key,
                        sort_descending,
                    )
                    .await?;
                    selected = 0;
                    scroll = 0;
                }
                KeyCode::Esc => break,
                KeyCode::Char('q') => break,
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Char('j') | KeyCode::Down => {
                    selected = (selected + 1).min(rows.len().saturating_sub(1));
                    if selected >= scroll + 10 {
                        scroll += 1;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                    if selected < scroll {
                        scroll = selected;
                    }
                }
                KeyCode::Char('g') => {
                    selected = 0;
                    scroll = 0;
                }
                KeyCode::Char('G') => {
                    selected = rows.len().saturating_sub(1);
                    scroll = selected.saturating_sub(10);
                }
                KeyCode::Tab => {
                    kind = kind.next();
                    rows = fetch(
                        client,
                        kind,
                        namespace.as_deref(),
                        sort_key,
                        sort_descending,
                    )
                    .await?;
                    selected = 0;
                    scroll = 0;
                    detail = None;
                }
                KeyCode::Char('n') => {
                    namespace = cycle_namespace(client, namespace.clone()).await?;
                    rows = fetch(
                        client,
                        kind,
                        namespace.as_deref(),
                        sort_key,
                        sort_descending,
                    )
                    .await?;
                    selected = 0;
                    scroll = 0;
                    detail = None;
                }
                KeyCode::Char('s') => {
                    sort_key = next_sort_key(sort_key);
                    rows = fetch(
                        client,
                        kind,
                        namespace.as_deref(),
                        sort_key,
                        sort_descending,
                    )
                    .await?;
                    selected = 0;
                    scroll = 0;
                }
                KeyCode::Char('S') => {
                    sort_descending = !sort_descending;
                    rows = fetch(
                        client,
                        kind,
                        namespace.as_deref(),
                        sort_key,
                        sort_descending,
                    )
                    .await?;
                    selected = 0;
                    scroll = 0;
                }
                KeyCode::Char('d') => {
                    if let Some(r) = rows.get(selected) {
                        detail = describe(client, kind, r).await.ok();
                    }
                }
                KeyCode::Char('i') => {
                    if kind == Kind::Pods
                        && let Some(r) = rows.get(selected)
                    {
                        detail = diagnose(client, r).await.ok();
                    } else {
                        detail = Some("Diagnostics are available for pods only.".into());
                    }
                }
                KeyCode::Char('/') => {
                    // Enter fuzzy-jump mode (empty query = show all).
                    jump_query = Some(String::new());
                }
                KeyCode::Char(c) if jump_query.is_some() && c != '/' => {
                    // In jump mode: append typed chars to the query and re-rank rows.
                    if let Some(q) = jump_query.as_mut() {
                        q.push(c);
                    }
                    if let Some(q) = jump_query.as_deref() {
                        rows = fuzzy_rerank(rows.clone(), q);
                        selected = 0;
                        scroll = 0;
                    }
                }
                KeyCode::Backspace if jump_query.is_some() => {
                    if let Some(q) = jump_query.as_mut() {
                        q.pop();
                    }
                    if let Some(q) = jump_query.as_deref() {
                        rows = fuzzy_rerank(rows.clone(), q);
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

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

async fn fetch(
    client: &Client,
    kind: Kind,
    namespace: Option<&str>,
    sort_key: kaptein_core::discovery::SortKey,
    descending: bool,
) -> io::Result<Vec<TableRow>> {
    let gvk = kind.gvk();
    let items = kaptein_core::discovery::list_with(
        client,
        &gvk,
        namespace,
        Some(sort_key),
        descending,
        None,
    )
    .await
    .map_err(|e| io::Error::other(e.to_string()))?;
    Ok(items
        .into_iter()
        .map(|r| TableRow {
            name: r.name,
            namespace: r.namespace,
            status: status_for(kind),
            created: r.created.map(|t| t.0.to_string()).unwrap_or_default(),
        })
        .collect())
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

fn status_for(kind: Kind) -> String {
    match kind {
        Kind::Namespaces => "Active".into(),
        Kind::Nodes => "Ready".into(), // refined via node status in a later milestone
        Kind::Pods => "Running".into(), // refined via pod status in a later milestone
        Kind::Deployments => "Ready".into(),
        Kind::Services => "Active".into(),
    }
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
