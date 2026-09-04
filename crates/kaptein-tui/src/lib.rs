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
//!   d  describe                y  YAML
//!   l  logs (pods)             i  diagnose
//!   Shift-B  blast radius      Shift-W  what changed
//!   h  health checks           ?  help overlay
//!   :q / Ctrl-C  quit          Esc  back/dismiss (never quits)

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
#[derive(Clone, Debug, PartialEq)]
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

    // One informer lifecycle manager per **session** (finding M): every plane the TUI
    // builds shares this manager, so `max_watches` is enforced over the set of all views
    // the operator opens, not per plane. The policy comes from `[informer]` config.
    let informers = std::sync::Arc::new(kaptein_core::informer::InformerManager::new(
        kaptein_core::config::load().informer.to_policy(),
    ));

    // An informer-backed live data plane (ADR-0006): seeded once, kept fresh by a
    // background watch task. Sorting/filtering and the table itself read the in-memory
    // plane — the TUI does *not* re-list the cluster per keystroke.
    let mut plane = new_plane(client, &kind, namespace.clone(), &informers);
    plane
        .seed()
        .await
        .map_err(|e| io::Error::other(e.to_string()))?;
    let mut watch = Some(tokio::spawn({
        let plane = plane.clone_plane();
        async move { plane.watch_loop().await }
    }));

    let mut scroll: usize = 0;
    let mut selected: usize = 0;
    let mut detail: Option<String> = None;
    // Help overlay: `?` toggles a full-screen keymap reference (the discoverability
    // backstop from M1.9). When `true`, the table is dimmed under the overlay and any
    // key dismisses it.
    let mut help: bool = false;
    // Number of table rows visible in the current terminal (set each frame; drives the
    // scroll window instead of a hardcoded constant).
    let mut page_height: usize = 10;
    let (mut rows, mut total) =
        query_plane(&plane, &kind, sort_key, sort_descending, 0, page_height).await?;
    // The selected resource's RBAC-preflighted action graph (M2.2 "per-action RBAC
    // grey-out"): computed once per (kind, namespace), so an action the operator is not
    // permitted to take is greyed out *before* they try it, not after a 403.
    let mut actions = preflight_actions_for(client, &kind, namespace.as_deref()).await;
    // Fuzzy-jump mode: Some(query) means the user is typing a fuzzy query. The full
    // snapshot is preserved in `jump_master` (a `Vec<TableRow>`), and `jump_order` is
    // the ranked list of *indices* into it (best-first) — so a per-keystroke re-rank
    // reorders indices rather than deep-cloning the whole master (finding AA).
    let mut jump_query: Option<String> = None;
    let mut jump_master: Vec<TableRow> = Vec::new();
    let mut jump_order: Vec<usize> = Vec::new();
    // Command-palette mode: Some(query) means the palette is open (vim-style ':').
    let mut palette_query: Option<String> = None;
    // The last-observed MemPlane revision: the table is only re-queried (a full
    // clone+sort of the row set) when a watch delta actually landed, not every ~10 Hz
    // frame — the fix for issue #28's "query_plane still asks for 50k rows at 10 Hz".
    let mut last_revision = plane.mem().revision();

    loop {
        // Refresh from the live plane only when its revision advanced (a watch delta
        // landed), and only the visible window is re-queried. No API call per keystroke
        // and no per-frame materialization of the whole set. Skipped while fuzzy-jump
        // mode is active (its filtered `rows` is authoritative).
        if jump_query.is_none() && palette_query.is_none() {
            let rev = plane.mem().revision();
            if rev != last_revision {
                let (new_rows, new_total, sel, scr) = requery_window(
                    &plane,
                    &kind,
                    sort_key,
                    sort_descending,
                    selected,
                    scroll,
                    page_height,
                )
                .await?;
                rows = new_rows;
                total = new_total;
                selected = sel;
                scroll = scr;
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
                " {:<12} ns:{} sort:{} ({} rows) — {}  /:jump  ::palette  ?:help  :q:quit ",
                kind.label,
                namespace.as_deref().unwrap_or("all"),
                sort_label(&kind.headers, sort_key, sort_descending),
                total,
                action_hint_line(&actions)
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

            // In **normal** mode `rows` is the visible window (`query_plane` materializes
            // only `[scroll, scroll+page_height)`, M1.8) and the selected row sits at
            // `selected - scroll`. In **jump** mode the ranked result is `jump_master`
            // indexed by `jump_order` (indices, best-first — no per-keystroke clone), and
            // `selected` indexes `jump_order` directly (`scroll == 0`).
            let in_jump = jump_query.is_some();
            let view_rows: Vec<&TableRow> = if in_jump {
                // Borrow only the visible window of the ranked jump list (indices into
                // `jump_master`, best-first) — no per-keystroke clone of the full set.
                let start = scroll.min(jump_order.len());
                let end = (scroll + page_height).min(jump_order.len()).max(start);
                jump_order[start..end]
                    .iter()
                    .map(|&i| &jump_master[i])
                    .collect()
            } else {
                // `rows` is already the visible window (`query_plane` materializes only
                // `[scroll, scroll+page_height)`); borrow it directly.
                rows.iter().collect()
            };
            let view_selected = selected.saturating_sub(scroll);
            let table_rows: Vec<Row> = view_rows
                .iter()
                .enumerate()
                .map(|(offset, r)| {
                    let base = if offset == view_selected {
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
                let actions_line = action_hint_line(&actions);
                format!(
                    "Actions: {actions_line}  (\u{d7}=forbidden, !=gated)\nPress d to describe, y for YAML, l for logs, i to diagnose, h for health checks."
                )
            });
            let detail_para = Paragraph::new(detail_text)
                .block(Block::default().title(" Detail ").borders(Borders::ALL));
            frame.render_widget(detail_para, chunks[2]);

            // Help overlay (M1.9): a full-screen keymap reference, rendered *over* the
            // table so the operator can look up any binding without leaving the view.
            // Any key dismisses it; `?` toggles; `Esc` also dismisses (and, at the root,
            // never quits).
            if help {
                let help_text = help_text();
                let overlay = ratatui::widgets::Paragraph::new(help_text)
                    .block(Block::default().title(" Help — press any key to close ").borders(Borders::ALL))
                    .style(Style::default().fg(Color::White));
                let overlay_area = ratatui::layout::Rect {
                    x: area.x + 2,
                    y: area.y + 1,
                    width: area.width.saturating_sub(4),
                    height: area.height.saturating_sub(2),
                };
                frame.render_widget(ratatui::widgets::Clear, overlay_area);
                frame.render_widget(overlay, overlay_area);
            }
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
                    // Cancel jump mode: restore the unfiltered list (re-query the visible
                    // window, not the full set).
                    jump_query = None;
                    let (new_rows, new_total) = query_plane(
                        &plane,
                        &kind,
                        sort_key,
                        sort_descending,
                        scroll,
                        scroll + page_height,
                    )
                    .await?;
                    rows = new_rows;
                    total = new_total;
                    selected = selected.min(total.saturating_sub(1));
                }
                KeyCode::Esc if help => {
                    // Esc dismisses the help overlay — and, at the root of the navigation
                    // ladder (M1.9), it is a *no-op*, never a quit. In k9s — and lazygit,
                    // and every TUI with a view stack — Esc means "back"; a k9s user's
                    // first reflex must not close Kaptein. Quit is now explicit only:
                    // `:q`/`:q!`/`:x`/`:wq`, or `Ctrl-C`.
                    help = false;
                }
                KeyCode::Char('?') if palette_query.is_none() && jump_query.is_none() => {
                    help = !help;
                }
                // While the help overlay is up, any other key dismisses it (the overlay is
                // a reference, not a modal prompt). Ctrl-C still quits.
                KeyCode::Char('c')
                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && palette_query.is_none()
                        && jump_query.is_none() =>
                {
                    break;
                }
                _ if help => {
                    help = false;
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
                            &informers,
                            &mut rows,
                            &mut selected,
                            &mut scroll,
                            page_height,
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
                    // In jump mode the list is `jump_order` (ranked indices); otherwise
                    // `total` rows of the windowed store.
                    let list_len = if jump_query.is_some() {
                        jump_order.len()
                    } else {
                        total
                    };
                    selected = (selected + 1).min(list_len.saturating_sub(1));
                    if selected >= scroll + page_height {
                        scroll = selected.saturating_add(1).saturating_sub(page_height);
                    }
                    if jump_query.is_none() {
                        requery_and_assign(
                            &plane,
                            &kind,
                            sort_key,
                            sort_descending,
                            &mut rows,
                            &mut total,
                            &mut selected,
                            &mut scroll,
                            page_height,
                        )
                        .await?;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up if palette_query.is_none() => {
                    selected = selected.saturating_sub(1);
                    if selected < scroll {
                        scroll = selected;
                    }
                    if jump_query.is_none() {
                        requery_and_assign(
                            &plane,
                            &kind,
                            sort_key,
                            sort_descending,
                            &mut rows,
                            &mut total,
                            &mut selected,
                            &mut scroll,
                            page_height,
                        )
                        .await?;
                    }
                }
                KeyCode::Char('g') if palette_query.is_none() => {
                    selected = 0;
                    scroll = 0;
                    if jump_query.is_none() {
                        requery_and_assign(
                            &plane,
                            &kind,
                            sort_key,
                            sort_descending,
                            &mut rows,
                            &mut total,
                            &mut selected,
                            &mut scroll,
                            page_height,
                        )
                        .await?;
                    }
                }
                KeyCode::Char('G') if palette_query.is_none() => {
                    let list_len = if jump_query.is_some() {
                        jump_order.len()
                    } else {
                        total
                    };
                    selected = list_len.saturating_sub(1);
                    scroll = selected.saturating_sub(page_height);
                    if jump_query.is_none() {
                        requery_and_assign(
                            &plane,
                            &kind,
                            sort_key,
                            sort_descending,
                            &mut rows,
                            &mut total,
                            &mut selected,
                            &mut scroll,
                            page_height,
                        )
                        .await?;
                    }
                }
                KeyCode::Tab if palette_query.is_none() => {
                    kind = next_kind(&kinds, &kind);
                    // Cluster-scoped kinds have no namespace: clear it to avoid a 404 on
                    // the namespaced list/watch.
                    if kind.cluster_scoped {
                        namespace = None;
                    }
                    rebuild_plane(
                        client,
                        &mut plane,
                        &mut watch,
                        &kind,
                        namespace.clone(),
                        &informers,
                    )
                    .await?;
                    selected = 0;
                    scroll = 0;
                    let (new_rows, new_total) =
                        query_plane(&plane, &kind, sort_key, sort_descending, 0, page_height)
                            .await?;
                    rows = new_rows;
                    total = new_total;
                    last_revision = plane.mem().revision();
                    detail = None;
                    actions = preflight_actions_for(client, &kind, namespace.as_deref()).await;
                }
                KeyCode::Char('n') if palette_query.is_none() => {
                    namespace = cycle_namespace(client, namespace.clone()).await?;
                    rebuild_plane(
                        client,
                        &mut plane,
                        &mut watch,
                        &kind,
                        namespace.clone(),
                        &informers,
                    )
                    .await?;
                    selected = 0;
                    scroll = 0;
                    let (new_rows, new_total) =
                        query_plane(&plane, &kind, sort_key, sort_descending, 0, page_height)
                            .await?;
                    rows = new_rows;
                    total = new_total;
                    last_revision = plane.mem().revision();
                    detail = None;
                    actions = preflight_actions_for(client, &kind, namespace.as_deref()).await;
                }
                KeyCode::Char('s') if palette_query.is_none() => {
                    sort_key = next_sort_key(sort_key, kind.headers.len());
                    selected = 0;
                    scroll = 0;
                    if jump_query.is_none() {
                        requery_and_assign(
                            &plane,
                            &kind,
                            sort_key,
                            sort_descending,
                            &mut rows,
                            &mut total,
                            &mut selected,
                            &mut scroll,
                            page_height,
                        )
                        .await?;
                    }
                }
                KeyCode::Char('S') if palette_query.is_none() => {
                    sort_descending = !sort_descending;
                    selected = 0;
                    scroll = 0;
                    if jump_query.is_none() {
                        requery_and_assign(
                            &plane,
                            &kind,
                            sort_key,
                            sort_descending,
                            &mut rows,
                            &mut total,
                            &mut selected,
                            &mut scroll,
                            page_height,
                        )
                        .await?;
                    }
                }
                KeyCode::Char('d') if palette_query.is_none() => {
                    if action_is_forbidden(&actions, "describe") {
                        detail = Some("describe is forbidden by RBAC preflight.".into());
                    } else if jump_query.is_some() {
                        if let Some(r) = jump_selected_row(&jump_master, &jump_order, selected) {
                            detail = describe(client, &kind, r).await.ok();
                        }
                    } else if let Some(r) = selected_row(&rows, selected, scroll, false) {
                        detail = describe(client, &kind, r).await.ok();
                    }
                }
                KeyCode::Char('i') if palette_query.is_none() => {
                    if action_is_forbidden(&actions, "diagnose") {
                        detail = Some("diagnose is forbidden by RBAC preflight.".into());
                    } else if kind.is_pods() {
                        let selected: Option<&TableRow> = if jump_query.is_some() {
                            jump_selected_row(&jump_master, &jump_order, selected)
                        } else {
                            selected_row(&rows, selected, scroll, false)
                        };
                        if let Some(r) = selected {
                            detail = diagnose(client, r).await.ok();
                        }
                    } else {
                        detail = Some("Diagnostics are available for pods only.".into());
                    }
                }
                KeyCode::Char('h') if palette_query.is_none() => {
                    // Per-lens health (M2.2): evaluate the selected lens-driven resource's
                    // declared health checks and show one finding per failure (or
                    // "healthy"). Built-in kinds have no lens, so nothing to evaluate.
                    if kind.lens.is_none() {
                        detail =
                            Some("Health checks are available for lens-driven kinds only.".into());
                    } else {
                        let selected: Option<&TableRow> = if jump_query.is_some() {
                            jump_selected_row(&jump_master, &jump_order, selected)
                        } else {
                            selected_row(&rows, selected, scroll, false)
                        };
                        if let Some(r) = selected {
                            detail = lens_health(client, &kind, r).await.ok();
                        }
                    }
                }
                KeyCode::Char('y') if palette_query.is_none() => {
                    // YAML view (M1.9, k9s `y`): the redacted raw manifest. `d` is the
                    // human describe; `y` is the verbatim YAML — both go through the M1.7
                    // redaction choke point, so a Secret never renders plaintext.
                    let selected: Option<&TableRow> = if jump_query.is_some() {
                        jump_selected_row(&jump_master, &jump_order, selected)
                    } else {
                        selected_row(&rows, selected, scroll, false)
                    };
                    if let Some(r) = selected {
                        detail = describe(client, &kind, r).await.ok();
                    }
                }
                KeyCode::Char('l') if palette_query.is_none() => {
                    // Logs (M1.9, k9s `l`): tail the selected pod's logs (redacted per
                    // M1.7). Pods only — other kinds report so explicitly.
                    if !kind.is_pods() {
                        detail = Some("Logs are available for pods only.".into());
                    } else {
                        let selected: Option<&TableRow> = if jump_query.is_some() {
                            jump_selected_row(&jump_master, &jump_order, selected)
                        } else {
                            selected_row(&rows, selected, scroll, false)
                        };
                        if let Some(r) = selected {
                            detail = logs(client, r).await.ok();
                        }
                    }
                }
                KeyCode::Char('B') if palette_query.is_none() => {
                    // Blast radius (M1.9 Kaptein-unique `Shift-B`): the selected
                    // resource's owners + dependents (cascade-delete chain), read-only.
                    let selected: Option<&TableRow> = if jump_query.is_some() {
                        jump_selected_row(&jump_master, &jump_order, selected)
                    } else {
                        selected_row(&rows, selected, scroll, false)
                    };
                    if let Some(r) = selected {
                        detail = blast_radius(client, &kind, r).await.ok();
                    }
                }
                KeyCode::Char('W') if palette_query.is_none() => {
                    // What changed (M1.9 Kaptein-unique `Shift-W`): recent events in the
                    // selected resource's namespace over the last 15 minutes, read-only.
                    let selected: Option<&TableRow> = if jump_query.is_some() {
                        jump_selected_row(&jump_master, &jump_order, selected)
                    } else {
                        selected_row(&rows, selected, scroll, false)
                    };
                    if let Some(r) = selected {
                        detail = what_changed(client, r).await.ok();
                    }
                }
                KeyCode::Char('/') if palette_query.is_none() => {
                    // Enter fuzzy-jump mode (empty query = show all). Snapshot the full
                    // set (one query) so backspace can re-rank against it — the fuzzy
                    // list is a search over the whole store, not just the visible window.
                    let (master, master_total) =
                        query_plane(&plane, &kind, sort_key, sort_descending, 0, 50_000).await?;
                    jump_master = master;
                    // Empty query ranks everything in input order (indices 0..n).
                    jump_order = (0..jump_master.len()).collect();
                    total = master_total;
                    selected = 0;
                    scroll = 0;
                    jump_query = Some(String::new());
                }
                KeyCode::Char(c) if jump_query.is_some() && c != '/' => {
                    // In jump mode: append typed chars to the query and re-rank *indices*
                    // into the master list (not a deep-clone of it), so backspace works.
                    if let Some(q) = jump_query.as_mut() {
                        q.push(c);
                    }
                    if let Some(q) = jump_query.as_deref() {
                        jump_order = fuzzy_rerank(&jump_master, q);
                        selected = 0;
                        scroll = 0;
                    }
                }
                KeyCode::Backspace if jump_query.is_some() => {
                    if let Some(q) = jump_query.as_mut() {
                        q.pop();
                    }
                    if let Some(q) = jump_query.as_deref() {
                        jump_order = fuzzy_rerank(&jump_master, q);
                        selected = 0;
                        scroll = 0;
                    }
                }
                KeyCode::Enter if jump_query.is_some() => {
                    // Exit jump mode, keeping the current (fuzzy-ranked) selection. Find
                    // the selected row's *absolute* index in the sorted store (one full
                    // query), then re-window to it.
                    let chosen_name = jump_selected_row(&jump_master, &jump_order, selected)
                        .map(|r| r.name.clone());
                    jump_query = None;
                    if let Some(name) = chosen_name {
                        let (full, full_total) =
                            query_plane(&plane, &kind, sort_key, sort_descending, 0, 50_000)
                                .await?;
                        total = full_total;
                        if let Some(abs) = full.iter().position(|r| r.name == name) {
                            selected = abs;
                        }
                        scroll = clamp_viewport(total, selected, 0, page_height).1;
                        let (new_rows, new_total) = query_plane(
                            &plane,
                            &kind,
                            sort_key,
                            sort_descending,
                            scroll,
                            scroll + page_height,
                        )
                        .await?;
                        rows = new_rows;
                        total = new_total;
                    }
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

/// The `?` help overlay text — a single reference of the current keymap (M1.9). This is
/// intentionally *not* derived from the action graph yet (that is the dynamic hint bar the
/// milestone tracks); it is the discoverability backstop that makes every existing binding
/// visible without reading the source or the status line.
fn help_text() -> String {
    [
        "Navigation",
        "  j / k or ↓ / ↑   move selection",
        "  g / G            jump to top / bottom",
        "  Tab              cycle resource kind",
        "  n                cycle namespace",
        "  /                fuzzy-jump filter (Enter accept, Esc cancel)",
        "  :                command palette (e.g. :q to quit)",
        "",
        "Actions (on the selected resource)",
        "  d                describe",
        "  y                YAML (redacted raw manifest)",
        "  l                logs (pods)",
        "  i                diagnose (pods)",
        "  h                lens health checks",
        "  Shift-B          blast radius (owners + dependents)",
        "  Shift-W          what changed (recent events)",
        "",
        "Sorting",
        "  s                cycle sort column",
        "  S                toggle sort direction",
        "",
        "Quit",
        "  :q / :q! / :x / :wq   quit (vim-style)",
        "  Ctrl-C            quit",
        "  Esc               back / dismiss (never quits)",
    ]
    .join("\n")
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
///
/// **Allocation-free on the hot path (finding AA):** it takes `&[TableRow]` and returns
/// `Vec<usize>` — indices into the input, ordered best-first — so a per-keystroke re-rank
/// no longer deep-clones the whole master list (`{ String, String, Vec<String> }` per
/// row). The caller holds the master list and materializes only what it renders.
fn fuzzy_rerank(rows: &[TableRow], query: &str) -> Vec<usize> {
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    kaptein_viewmodel::fuzzy_rank_indices(names, query)
        .into_iter()
        .map(|m| m.index)
        .collect()
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
    informers: &std::sync::Arc<kaptein_core::informer::InformerManager>,
    rows: &mut Vec<TableRow>,
    selected: &mut usize,
    scroll: &mut usize,
    page_height: usize,
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
        rebuild_plane(client, plane, watch, kind, namespace.clone(), informers).await?;
    }
    // Re-query the (possibly rebuilt) live plane — no new API list. These palette
    // commands reset to the top, so query the first visible window only.
    *rows = query_plane(plane, kind, *sort_key, *sort_descending, 0, page_height)
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
    informers: &std::sync::Arc<kaptein_core::informer::InformerManager>,
) -> io::Result<()> {
    // Stop the old watch task (best-effort; the stream ends on abort). The aborted
    // task's `WatchSlotGuard` releases the old view's slot back to the shared manager
    // (finding N), so a session-scoped cap does not leak a slot per view switch.
    if let Some(handle) = watch.take() {
        handle.abort();
    }
    *plane = new_plane(client, kind, namespace, informers);
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

/// Build a `LivePlane` sharing the session-scoped informer manager (finding M). The
/// policy comes from the shared manager (created once from `[informer]` config), so the
/// TUI's watch budget is operator-tunable **and** enforced across all views in the
/// session, not per plane. A lens-driven kind builds a lens plane (M2.2), so its
/// declared columns become the plane's schema.
fn new_plane(
    client: &Client,
    kind: &Kind,
    namespace: Option<String>,
    informers: &std::sync::Arc<kaptein_core::informer::InformerManager>,
) -> kaptein_integration::LivePlane {
    match &kind.lens {
        Some(vd) => kaptein_integration::LivePlane::with_shared_informers(
            client.clone(),
            kind.gvk.clone(),
            namespace,
            Some(vd.clone()),
            informers.clone(),
        ),
        None => kaptein_integration::LivePlane::with_shared_informers(
            client.clone(),
            kind.gvk.clone(),
            namespace,
            None,
            informers.clone(),
        ),
    }
}

/// Query the live plane (sort + filter in the view-model, window in the data plane) and
/// map the resulting `Page` of `Row`s into geometry-local table rows. Returns the rows
/// **and** the total matching count (`page.total`), so the TUI can show "N rows" and jump
/// to the bottom (`G`) while decoupling `total` from the materialized window.
///
/// `start`/`end` are the **visible window** (M1.8, finding Q): only the requested slice
/// is materialized into `TableRow`s — `MemPlane::query` sorts/filters an index
/// permutation and clones only `[start, end)`. A busy cluster advancing the revision per
/// watch delta now re-materializes a few dozen rows, not 50 000.
/// The row's cells are the plane's schema columns in order — the lens's columns for a
/// lens-driven kind, the built-in four for a built-in kind.
async fn query_plane(
    plane: &kaptein_integration::LivePlane,
    kind: &Kind,
    sort_key: SortColumn,
    descending: bool,
    start: usize,
    end: usize,
) -> io::Result<(Vec<TableRow>, usize)> {
    let column_ids = plane.column_ids();
    let sort_column = column_ids
        .get(sort_key.0)
        .cloned()
        .unwrap_or_else(|| "name".to_string());
    use kaptein_integration::kaptein_viewmodel::DataPlane as _;
    let page = plane
        .query(&kaptein_integration::kaptein_viewmodel::Query {
            start,
            end,
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

/// Clamp an absolute `(selected, scroll)` pair into a valid `page_height`-sized window
/// over `total` rows. Pure geometry (no terminal, no plane) so it is unit-testable.
///
/// Returns the corrected `(selected, scroll)`: `selected` is kept within `[0, total)`,
/// and `scroll` is moved so `selected` always sits inside `[scroll, scroll + page_height)`.
fn clamp_viewport(
    total: usize,
    selected: usize,
    scroll: usize,
    page_height: usize,
) -> (usize, usize) {
    if total == 0 {
        return (0, 0);
    }
    let page_height = page_height.max(1);
    let selected = selected.min(total - 1);
    let mut scroll = scroll.min(total.saturating_sub(1));
    if selected < scroll {
        scroll = selected;
    } else if selected >= scroll.saturating_add(page_height) {
        scroll = selected.saturating_add(1).saturating_sub(page_height);
    }
    (selected, scroll)
}

/// The selected `TableRow`, given the current display list and mode. In **normal** mode
/// `rows` is the visible window and `selected`/`scroll` are absolute indices, so the row
/// is at `selected - scroll`. In **jump** mode `rows` is the fuzzy-ranked list and
/// `selected` indexes it directly (`scroll` is `0`), so the row is at `selected`.
fn selected_row(
    rows: &[TableRow],
    selected: usize,
    scroll: usize,
    in_jump: bool,
) -> Option<&TableRow> {
    if in_jump {
        rows.get(selected)
    } else {
        rows.get(selected.saturating_sub(scroll))
    }
}

/// The selected row in **jump mode**: `jump_order` maps a rank position to an index into
/// `jump_master`, and `selected` is the rank position. Pure geometry (no cloning), so the
/// per-keystroke re-rank path carries no per-row allocation.
fn jump_selected_row<'a>(
    master: &'a [TableRow],
    order: &[usize],
    selected: usize,
) -> Option<&'a TableRow> {
    order.get(selected).and_then(|&i| master.get(i))
}

/// Re-query the visible window after a revision change, clamping the viewport so the
/// selection stays valid against the new `total`. Returns `(rows, total, selected, scroll)`.
async fn requery_window(
    plane: &kaptein_integration::LivePlane,
    kind: &Kind,
    sort_key: SortColumn,
    descending: bool,
    selected: usize,
    scroll: usize,
    page_height: usize,
) -> io::Result<(Vec<TableRow>, usize, usize, usize)> {
    let (rows, total) = query_plane(
        plane,
        kind,
        sort_key,
        descending,
        scroll,
        scroll + page_height,
    )
    .await?;
    let (selected, scroll) = clamp_viewport(total, selected, scroll, page_height);
    Ok((rows, total, selected, scroll))
}

/// Re-query the visible window and assign the four viewport state variables in place.
/// Used by the key handlers after a nav/sort change. The window is re-fetched at the
/// (possibly clamped) scroll so `rows` always matches `[scroll, scroll+page_height)`.
#[allow(clippy::too_many_arguments)]
async fn requery_and_assign(
    plane: &kaptein_integration::LivePlane,
    kind: &Kind,
    sort_key: SortColumn,
    descending: bool,
    rows: &mut Vec<TableRow>,
    total: &mut usize,
    selected: &mut usize,
    scroll: &mut usize,
    page_height: usize,
) -> io::Result<()> {
    let (new_rows, new_total, new_selected, new_scroll) = requery_window(
        plane,
        kind,
        sort_key,
        descending,
        *selected,
        *scroll,
        page_height,
    )
    .await?;
    *rows = new_rows;
    *total = new_total;
    *selected = new_selected;
    *scroll = new_scroll;
    Ok(())
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

/// Tail the selected pod's logs (M1.9 `l`), redacted per M1.7 by the core `pod_logs`
/// path (which routes through `redact_line`). Returns a `[container] line`-joined block,
/// or an empty string for a pod with no containers.
async fn logs(client: &Client, row: &TableRow) -> io::Result<String> {
    let lines = kaptein_core::describe::pod_logs(client, &row.namespace, &row.name, Some(200))
        .await
        .map_err(|e| io::Error::other(e.to_string()))?;
    if lines.is_empty() {
        Ok(format!("{}: no log lines", row.name))
    } else {
        Ok(lines
            .iter()
            .map(|(c, l)| format!("[{c}] {l}"))
            .collect::<Vec<_>>()
            .join("\n"))
    }
}

/// Compute the selected resource's blast radius (M1.9 `Shift-B`) — owners +
/// dependents over the ownership/cascade-delete chain, read-only.
async fn blast_radius(client: &Client, kind: &Kind, row: &TableRow) -> io::Result<String> {
    let ns = if kind.cluster_scoped || row.namespace.is_empty() {
        None
    } else {
        Some(row.namespace.as_str())
    };
    let br = kaptein_core::moat::blast_radius(client, ns, &kind.gvk, &row.name)
        .await
        .map_err(|e| io::Error::other(e.to_string()))?;
    Ok(format_blast_radius(&br))
}

/// Format a `BlastRadius` for the detail pane: owners and dependents as two
/// newline-joined lists. Pure (no I/O) so it is unit-testable.
fn format_blast_radius(br: &kaptein_core::moat::BlastRadius) -> String {
    let owners = if br.owners.is_empty() {
        "(none — top-level resource)".to_string()
    } else {
        br.owners.join(", ")
    };
    let dependents = if br.dependents.is_empty() {
        "(none — removing this affects nothing downstream)".to_string()
    } else {
        br.dependents.join(", ")
    };
    format!(
        "blast radius for {}/{}/{}\nowners: {owners}\ndependents: {dependents}",
        br.namespace, br.kind, br.name
    )
}

/// Recent events in the selected resource's namespace (M1.9 `Shift-W`), read-only.
async fn what_changed(client: &Client, row: &TableRow) -> io::Result<String> {
    let wc = kaptein_core::moat::what_changed_between(client, &row.namespace, None, None, Some(15))
        .await
        .map_err(|e| io::Error::other(e.to_string()))?;
    Ok(format_what_changed(&wc))
}

/// Format a `WhatChanged` for the detail pane. Pure (no I/O) so it is unit-testable.
fn format_what_changed(wc: &kaptein_core::moat::WhatChanged) -> String {
    let mut out = vec![format!(
        "what changed in {} (last 15 min): {} events",
        wc.namespace,
        wc.events.len()
    )];
    for e in &wc.events {
        out.push(format!(
            "  {} {} {}/{}: {}",
            e.type_, e.reason, e.kind, e.name, e.message
        ));
    }
    out.join("\n")
}

/// Evaluate a lens-driven kind's declared health checks against the selected resource
/// (M2.2 per-lens health). Fetches the **redacted** object once, runs the view-model's
/// `evaluate_health` (the same engine `viewdef-render` uses), and formats a finding per
/// failing check — or "healthy" when every check holds. This is the daily-driver surface:
/// the TUI shows what the lens's health predicates say, not just the status chip.
async fn lens_health(client: &Client, kind: &Kind, row: &TableRow) -> io::Result<String> {
    let vd = match &kind.lens {
        Some(vd) if !vd.health.is_empty() => vd,
        _ => return Ok("This lens declares no health checks.".to_string()),
    };
    let ns = if kind.cluster_scoped || row.namespace.is_empty() {
        None
    } else {
        Some(row.namespace.as_str())
    };
    let value = kaptein_core::describe::get_dynamic_redacted(client, &kind.gvk, ns, &row.name)
        .await
        .map_err(|e| io::Error::other(e.to_string()))?;
    let findings = kaptein_integration::kaptein_viewmodel::evaluate_health(
        vd,
        &kaptein_integration::kaptein_viewmodel::Redacted::from_redacted(value),
    );
    Ok(format_health_findings(&row.name, &findings))
}

/// Format a resource's health findings for the detail pane: "name: healthy" when there
/// are none, else one `id: label_key (level)` line per failing check. Pure (no I/O) so
/// it is unit-testable; the fetch + evaluation live in [`lens_health`].
fn format_health_findings(
    name: &str,
    findings: &[kaptein_integration::kaptein_viewmodel::HealthFinding],
) -> String {
    if findings.is_empty() {
        format!("{name}: healthy")
    } else {
        findings
            .iter()
            .map(|f| format!("{}: {} ({:?})", f.id, f.label_key, f.level))
            .collect::<Vec<_>>()
            .join("\n")
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

/// Render the RBAC-preflighted action graph as a compact hint (M1.9 dynamic hint bar).
/// This is the *derived* form of the old hardcoded `d:describe i:diagnose` string: it
/// lists every action the semantic layer exposes for the current kind/selection, marking
/// `Forbidden` (`×`) and `Gated` (`!`) so the guardrail is visible *before* the keystroke,
/// not as a 403 after. Pure (no I/O) so it is unit-testable — and it is one of the three
/// consumers of the same action graph (`finding AF`'s `semantic::Action`), alongside the
/// command palette and the agent tool surface.
fn action_hint_line(actions: &[Action]) -> String {
    if actions.is_empty() {
        return "describe, diagnose".to_string();
    }
    actions
        .iter()
        .map(|a| {
            let marker = match a.state {
                ActionState::Forbidden { .. } => "×",
                ActionState::Gated { .. } => "!",
                ActionState::Allowed => "",
            };
            format!("{}{}", a.id, marker)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trow(name: &str) -> TableRow {
        TableRow {
            name: name.into(),
            namespace: "ns".into(),
            cells: vec![name.into()],
        }
    }

    #[test]
    fn clamp_viewport_keeps_selection_inside_window() {
        // 100 rows, 10-row page: selection at the top, middle, and bottom all stay valid.
        assert_eq!(clamp_viewport(100, 0, 0, 10), (0, 0));
        assert_eq!(clamp_viewport(100, 5, 0, 10), (5, 0));
        assert_eq!(clamp_viewport(100, 99, 90, 10), (99, 90));
        // Selection pushed past the page end moves scroll forward.
        assert_eq!(clamp_viewport(100, 10, 0, 10), (10, 1));
        // Selection scrolled past the top pulls scroll back.
        assert_eq!(clamp_viewport(100, 3, 5, 10), (3, 3));
    }

    #[test]
    fn clamp_viewport_handles_empty_and_short_lists() {
        assert_eq!(clamp_viewport(0, 0, 0, 10), (0, 0));
        assert_eq!(clamp_viewport(3, 5, 0, 10), (2, 0)); // selected clamped to total-1
        assert_eq!(clamp_viewport(1, 0, 0, 10), (0, 0));
    }

    #[test]
    fn clamp_viewport_zero_page_height_is_safe() {
        // page_height 0 must not underflow; treated as 1. selected=99 forces scroll to
        // 99 (selected - page_height + 1) so the selection stays in the window.
        assert_eq!(clamp_viewport(100, 99, 0, 0), (99, 99));
    }

    #[test]
    fn selected_row_uses_absolute_index_in_normal_mode() {
        // Normal mode: rows is the window [scroll, scroll+len), selected is absolute.
        let rows = vec![trow("a"), trow("b"), trow("c")];
        // selected=5, scroll=5 → first row of the window.
        assert_eq!(
            selected_row(&rows, 5, 5, false).map(|r| r.name.as_str()),
            Some("a")
        );
        // selected=7, scroll=5 → third row.
        assert_eq!(
            selected_row(&rows, 7, 5, false).map(|r| r.name.as_str()),
            Some("c")
        );
        // selected below scroll clamps to the first windowed row (never underflows).
        assert_eq!(
            selected_row(&rows, 4, 5, false).map(|r| r.name.as_str()),
            Some("a")
        );
    }

    #[test]
    fn selected_row_indexes_directly_in_jump_mode() {
        let rows = vec![trow("x"), trow("y")];
        assert_eq!(
            selected_row(&rows, 1, 0, true).map(|r| r.name.as_str()),
            Some("y")
        );
        assert_eq!(selected_row(&rows, 9, 0, true), None);
    }

    #[test]
    fn jump_selected_row_resolves_order_indices_into_master() {
        let master = vec![trow("a"), trow("b"), trow("c")];
        // order [2, 0, 1] → rank 0 is "c", rank 1 is "a", rank 2 is "b".
        let order = vec![2usize, 0, 1];
        assert_eq!(
            jump_selected_row(&master, &order, 0).map(|r| r.name.as_str()),
            Some("c")
        );
        assert_eq!(
            jump_selected_row(&master, &order, 1).map(|r| r.name.as_str()),
            Some("a")
        );
        assert_eq!(jump_selected_row(&master, &order, 9), None);
    }

    #[test]
    fn fuzzy_rerank_returns_indices_best_first() {
        let master = vec![trow("nginx-ingress"), trow("nagios"), trow("zzz")];
        let order = fuzzy_rerank(&master, "nginx");
        assert_eq!(order, vec![0usize]); // only "nginx-ingress" matches
        // An empty query matches everything in input order.
        let all = fuzzy_rerank(&master, "");
        assert_eq!(all, vec![0usize, 1, 2]);
    }

    #[test]
    fn format_health_findings_reports_healthy_or_one_line_per_failure() {
        use kaptein_integration::kaptein_viewmodel::{HealthFinding, StatusLevel};
        assert_eq!(format_health_findings("pg", &[]), "pg: healthy");
        let findings = vec![
            HealthFinding {
                id: "ready-instances".into(),
                label_key: "health.ready-instances".into(),
                level: StatusLevel::Error,
            },
            HealthFinding {
                id: "replication-lag".into(),
                label_key: "health.replication-lag".into(),
                level: StatusLevel::Warning,
            },
        ];
        let out = format_health_findings("pg", &findings);
        assert_eq!(
            out,
            "ready-instances: health.ready-instances (Error)\nreplication-lag: health.replication-lag (Warning)"
        );
    }

    #[test]
    fn format_blast_radius_lists_owners_and_dependents() {
        let br = kaptein_core::moat::BlastRadius {
            namespace: "ns".into(),
            kind: "Deployment".into(),
            name: "web".into(),
            owners: vec!["ReplicaSet/abc".into()],
            dependents: vec!["Pod/x".into(), "Pod/y".into()],
        };
        let out = format_blast_radius(&br);
        assert!(out.contains("blast radius for ns/Deployment/web"));
        assert!(out.contains("owners: ReplicaSet/abc"));
        assert!(out.contains("dependents: Pod/x, Pod/y"));
    }

    #[test]
    fn format_what_changed_lists_events() {
        let wc = kaptein_core::moat::WhatChanged {
            namespace: "ns".into(),
            from_ms: 0,
            to_ms: 1,
            events: vec![kaptein_core::events::EventSummary {
                namespace: "ns".into(),
                kind: "Pod".into(),
                name: "p".into(),
                type_: "Warning".into(),
                reason: "BackOff".into(),
                message: "restarting".into(),
                count: 3,
                last_timestamp_ms: 1,
            }],
        };
        let out = format_what_changed(&wc);
        assert!(out.contains("what changed in ns (last 15 min): 1 events"));
        assert!(out.contains("Warning BackOff Pod/p: restarting"));
    }

    #[test]
    fn action_hint_line_marks_forbidden_and_gated() {
        use kaptein_integration::kaptein_viewmodel::ActionState;
        let actions = vec![
            Action {
                id: "describe".into(),
                label_key: "action.describe".into(),
                state: ActionState::Allowed,
            },
            Action {
                id: "restart".into(),
                label_key: "action.restart".into(),
                state: ActionState::Forbidden {
                    verb: "update".into(),
                    resource: "deployments".into(),
                    namespace: None,
                },
            },
            Action {
                id: "delete".into(),
                label_key: "action.delete".into(),
                state: ActionState::Gated {
                    reason_key: "guardrail.break-glass".into(),
                },
            },
        ];
        assert_eq!(action_hint_line(&actions), "describe restart\u{d7} delete!");
        assert_eq!(action_hint_line(&[]), "describe, diagnose");
    }

    #[test]
    fn help_text_documents_the_keymap_and_quit_is_explicit() {
        let h = help_text();
        // The discoverability backstop: every core binding is present.
        for needle in [
            "j / k",
            "Tab",
            "fuzzy-jump",
            "command palette",
            "describe",
            "YAML",
            "logs",
            "diagnose",
            "health",
            "blast radius",
            "what changed",
            "sort",
        ] {
            assert!(h.contains(needle), "help text must mention {needle:?}");
        }
        // Esc is documented as *never* quitting (M1.9 — a k9s user's first reflex).
        assert!(
            h.contains("never quits"),
            "help text must state Esc never quits"
        );
        // Quit is explicit: :q variants and Ctrl-C.
        assert!(h.contains(":q"), "help text must list :q quit");
        assert!(h.contains("Ctrl-C"), "help text must list Ctrl-C quit");
        assert!(
            !h.contains("Esc               quit"),
            "Esc must not be documented as quit"
        );
    }
}
