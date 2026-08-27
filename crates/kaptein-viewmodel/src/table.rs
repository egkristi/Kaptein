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
///
/// Same-variant comparisons are **allocation-free** (`&str` comparison, not a `String`
/// clone): this is the M1.8 hot spot — sorting a 50 000-row `Text` (name) or `Status`
/// column was cloning two `String`s per comparison (~1.7M allocations/query). The
/// heterogeneous fallback still goes through `cell_text` (numbers/booleans render as
/// text), which is rare in practice and cheap relative to the sort.
pub fn cmp_cells(a: &Cell, b: &Cell) -> Ordering {
    match (a, b) {
        (Cell::Number { value: x }, Cell::Number { value: y }) => x.cmp(y),
        (Cell::Timestamp { millis: x }, Cell::Timestamp { millis: y }) => x.cmp(y),
        (Cell::Text { value: x }, Cell::Text { value: y }) => x.as_str().cmp(y.as_str()),
        (Cell::Status { label_key: x, .. }, Cell::Status { label_key: y, .. }) => {
            x.as_str().cmp(y.as_str())
        }
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

/// Filter a permutation of row indices by the same `Filter` semantics as
/// [`filter_rows`], without cloning any `Row`. `indices` is the (possibly sorted)
/// permutation; it is retained in place, dropping indices whose row does not match.
pub fn filter_indices(indices: Vec<usize>, rows: &[Row], filter: Option<&Filter>) -> Vec<usize> {
    let Some(filter) = filter else {
        return indices;
    };
    let needle = filter.expression.trim().to_ascii_lowercase();
    if needle.is_empty() {
        return indices;
    }
    indices
        .into_iter()
        .filter(|&i| {
            rows[i]
                .cells
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

/// Sort a permutation of row indices by the same `SortSpec` semantics as [`sort_rows`],
/// but without cloning any `Row`. This is the allocation-conscious form used by the
/// informer-backed `MemPlane`: the caller holds `&[Row]` and sorts `indices` into it, so
/// a 50k-row query sorts 50k `usize`s instead of deep-cloning 50k `Row`s (M1.8).
///
/// `indices` must be `0..rows.len()` (any order); after the call it is the stable sort
/// order of those indices. Same-variant comparison is allocation-free via [`cmp_cells`].
pub fn sort_indices(
    indices: &mut [usize],
    rows: &[Row],
    column_ids: &[String],
    sort: Option<&SortSpec>,
) {
    let Some(sort) = sort else {
        return;
    };
    let Some(idx) = column_ids.iter().position(|id| id == &sort.column) else {
        return;
    };
    indices.sort_by(|&a, &b| {
        let ord = match (rows[a].cells.get(idx), rows[b].cells.get(idx)) {
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

    #[test]
    fn cmp_cells_orders_by_text_and_status_without_rendering_numbers() {
        // M1.8: the common sort keys (name = Text, status = Status) compare lexically.
        // Same-variant comparisons are allocation-free (`&str` comparison, not a `String`
        // clone) — the ordering is identical either way, so this asserts the *semantics*
        // the allocation-free path must preserve.
        let a = text("apple");
        let b = text("banana");
        assert_eq!(cmp_cells(&a, &b), Ordering::Less);
        assert_eq!(cmp_cells(&b, &a), Ordering::Greater);
        assert_eq!(cmp_cells(&a, &a), Ordering::Equal);

        let sa = Cell::Status {
            level: StatusLevel::Ok,
            label_key: "status.ok".into(),
        };
        let sb = Cell::Status {
            level: StatusLevel::Warning,
            label_key: "status.warning".into(),
        };
        assert_eq!(cmp_cells(&sa, &sb), Ordering::Less);
        assert_eq!(cmp_cells(&sb, &sa), Ordering::Greater);

        // Numbers still compare numerically, not lexically.
        assert_eq!(cmp_cells(&num(9), &num(10)), Ordering::Less);
        // The heterogeneous fallback (Text vs Number) renders both as text.
        assert_eq!(cmp_cells(&text("9"), &num(10)), Ordering::Greater); // "9" > "10" lexically
    }
}
