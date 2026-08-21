//! Kaptein TUI — a ratatui table view over Kubernetes resources.
//!
//! The first renderer-agnostic projection: it lists pods (default) with vim navigation
//! (`j`/`k`/`g`/`G`/`q`), demonstrating the "thin frontend" pattern — it consumes data
//! from `kaptein-core`, owns only *geometry* (scroll, selection, column layout), and
//! recomputes nothing semantic.

use std::io;

use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use kube::core::GroupVersionKind;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Row, Table};

/// A tabular row in the TUI's own geometry space (not the view-model's `Row`).
struct TableRow {
    name: String,
    namespace: String,
    kind: String,
    created: String,
}

#[tokio::main]
async fn main() -> io::Result<()> {
    // Connect to the cluster and fetch pods (default resource).
    let client = kaptein_core::discovery::client()
        .await
        .map_err(|e| io::Error::other(e.to_string()))?;
    let gvk = GroupVersionKind::gvk("", "v1", "Pod");
    let items = kaptein_core::discovery::list(&client, &gvk, None)
        .await
        .map_err(|e| io::Error::other(e.to_string()))?;

    let rows: Vec<TableRow> = items
        .into_iter()
        .map(|r| TableRow {
            name: r.name,
            namespace: r.namespace,
            kind: r.kind,
            created: r.created.map(|t| t.0.to_string()).unwrap_or_default(),
        })
        .collect();

    run_ui(rows).await
}

async fn run_ui(rows: Vec<TableRow>) -> io::Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut scroll: usize = 0;
    let mut selected: usize = 0;

    loop {
        terminal.draw(|frame| {
            let area = frame.area();
            let chunks = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);

            let header = Block::default()
                .title(" Kaptein — Pods (j/k navigate, q quit) ")
                .borders(Borders::ALL);
            frame.render_widget(header, chunks[0]);

            let rows: Vec<Row> = rows
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    let style = if i == selected {
                        Style::default().add_modifier(Modifier::REVERSED)
                    } else {
                        Style::default()
                    };
                    Row::new(vec![
                        Cell::from(r.name.as_str()),
                        Cell::from(r.namespace.as_str()),
                        Cell::from(r.kind.as_str()),
                        Cell::from(r.created.as_str()),
                    ])
                    .style(style)
                })
                .collect();

            let widths = [
                Constraint::Percentage(30),
                Constraint::Percentage(25),
                Constraint::Percentage(20),
                Constraint::Percentage(25),
            ];
            let table = Table::new(rows, widths)
                .header(Row::new(vec!["NAME", "NAMESPACE", "KIND", "CREATED"]))
                .block(Block::default().borders(Borders::ALL));
            frame.render_stateful_widget(
                table,
                chunks[1],
                &mut ratatui::widgets::TableState::default().with_offset(scroll),
            );
        })?;

        if event::poll(std::time::Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Char('j') | KeyCode::Down => {
                    selected = (selected + 1).min(rows.len().saturating_sub(1));
                    if selected >= scroll + (area_height(&terminal).unwrap_or(10)) {
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
                    scroll = selected;
                }
                _ => {}
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

fn area_height(terminal: &Terminal<CrosstermBackend<io::Stdout>>) -> Option<usize> {
    let area = terminal.size().ok()?;
    Some(area.height as usize)
}
