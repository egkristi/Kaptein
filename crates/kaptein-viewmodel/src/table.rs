//! Table-query semantics — the renderer-agnostic sort/filter that every `DataPlane`
//! implementation shares (ADR-0005).
//!
//! This is where "sorting and filtering" live, not in a frontend and not duplicated in
//! `kaptein-core`. A `DataPlane` maps its rows to `Vec<Row>` + a column-id schema, then
//! hands the `Query` (sort + filter + window) to these functions. The result is a bounded
//! `Page`, never a full materialization.

use std::cmp::Ordering;

use crate::render::{Cell, Filter, Row, SortSpec};

/// The display text of a cell, used for substring filtering and as the fallback sort key.
pub fn cell_text(cell: &Cell) -> String {
    match cell {
        Cell::Text { value } => value.clone(),
        Cell::Number { value } => value.to_string(),
        Cell::Timestamp { millis } => millis.to_string(),
        Cell::Status { label_key, .. } => label_key.clone(),
        Cell::Redacted => String::new(),
    }
}

/// Total order across heterogeneous cells. Numbers compare numerically, timestamps
/// chronologically, everything else lexically by display text.
pub fn cmp_cells(a: &Cell, b: &Cell) -> Ordering {
    match (a, b) {
        (Cell::Number { value: x }, Cell::Number { value: y }) => x.cmp(y),
        (Cell::Timestamp { millis: x }, Cell::Timestamp { millis: y }) => x.cmp(y),
        _ => cell_text(a).cmp(&cell_text(b)),
    }
}

/// Filter rows by the `Filter` expression, as a case-insensitive substring match over
/// **every** cell's text. A `None`/empty expression keeps all rows. This is the cheap,
/// predictable form of the `Filter` contract; the full expression language lands with
/// the lens engine (Phase 2) — but the *shape* is already a serializable string.
pub fn filter_rows(rows: Vec<Row>, filter: Option<&Filter>) -> Vec<Row> {
    let Some(filter) = filter else {
        return rows;
    };
    let needle = filter.expression.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return rows;
    }
    rows.into_iter()
        .filter(|row| {
            row.cells
                .iter()
                .any(|c| cell_text(c).to_ascii_lowercase().contains(&needle))
        })
        .collect()
}

/// Sort rows by the given `SortSpec`, resolving `column` against the `column_ids`
/// schema (column id → cell index). An unknown column leaves order unchanged (stable).
/// The sort is stable, so equal keys keep their identity order — deterministic across
/// frontends and in headless/CI.
pub fn sort_rows(rows: &mut [Row], column_ids: &[String], sort: Option<&SortSpec>) {
    let Some(sort) = sort else {
        return;
    };
    let Some(idx) = column_ids.iter().position(|id| id == &sort.column) else {
        return;
    };
    rows.sort_by(|a, b| {
        let ord = match (a.cells.get(idx), b.cells.get(idx)) {
            (Some(x), Some(y)) => cmp_cells(x, y),
            (Some(_), None) => Ordering::Greater,
            (None, Some(_)) => Ordering::Less,
            (None, None) => Ordering::Equal,
        };
        if sort.descending { ord.reverse() } else { ord }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{RowId, StatusLevel};

    fn text(v: &str) -> Cell {
        Cell::Text { value: v.into() }
    }
    fn num(v: i64) -> Cell {
        Cell::Number { value: v }
    }
    fn row(id: &str, cells: Vec<Cell>) -> Row {
        Row {
            id: RowId(id.into()),
            cells,
        }
    }

    #[test]
    fn filter_matches_substring_across_cells() {
        let rows = vec![
            row("a", vec![text("zebra")]),
            row("b", vec![text("apple"), text("ns-prod")]),
            row("c", vec![text("banana")]),
        ];
        let f = Filter {
            expression: "prod".into(),
        };
        let out = filter_rows(rows, Some(&f));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, RowId("b".into()));
    }

    #[test]
    fn filter_none_keeps_all() {
        let rows = vec![row("a", vec![text("x")]), row("b", vec![text("y")])];
        assert_eq!(filter_rows(rows.clone(), None), rows);
    }

    #[test]
    fn sort_by_numeric_column() {
        let ids = vec!["name".into(), "count".into()];
        let mut rows = vec![
            row("a", vec![text("a"), num(10)]),
            row("b", vec![text("b"), num(2)]),
            row("c", vec![text("c"), num(9)]),
        ];
        sort_rows(
            &mut rows,
            &ids,
            Some(&SortSpec {
                column: "count".into(),
                descending: false,
            }),
        );
        assert_eq!(rows[0].id, RowId("b".into()));
        assert_eq!(rows[1].id, RowId("c".into()));
        assert_eq!(rows[2].id, RowId("a".into()));
    }

    #[test]
    fn sort_descending_reverses() {
        let ids = vec!["name".into()];
        let mut rows = vec![row("a", vec![text("a")]), row("b", vec![text("b")])];
        sort_rows(
            &mut rows,
            &ids,
            Some(&SortSpec {
                column: "name".into(),
                descending: true,
            }),
        );
        assert_eq!(rows[0].id, RowId("b".into()));
    }

    #[test]
    fn sort_unknown_column_is_stable_noop() {
        let ids = vec!["name".into()];
        let mut rows = vec![row("a", vec![text("a")]), row("b", vec![text("b")])];
        sort_rows(
            &mut rows,
            &ids,
            Some(&SortSpec {
                column: "nope".into(),
                descending: false,
            }),
        );
        assert_eq!(rows[0].id, RowId("a".into()));
    }

    #[test]
    fn status_cell_has_label_text() {
        let cell = Cell::Status {
            level: StatusLevel::Warning,
            label_key: "status.running".into(),
        };
        assert_eq!(cell_text(&cell), "status.running");
    }
}
