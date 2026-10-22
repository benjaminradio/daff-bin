//! One regression test per fixed bug in `docs/BUGS.md`, in the default
//! (fixed) behaviour. Each test would fail with reference compatibility on.
use daff::format::csv::Csv;
use daff::format::json::{parse_ndjson, parse_table, render_json};
use daff::{compare_tables3, Flags, HighlightPatch, Merger, Table, TableDiff};

fn parse(s: &str) -> Table {
    let mut t = Csv::default().parse_full(s).table;
    t.trim_blank();
    t
}

fn csv(t: &Table) -> String {
    Csv { delimiter: ",".into(), eol: Some("\n".into()) }.render_table(t)
}

fn diff(parent: Option<&str>, a: &str, b: &str, flags: Flags) -> String {
    let (p, a, b) = (parent.map(parse), parse(a), parse(b));
    let align = compare_tables3(p.as_ref(), &a, &b, Some(&flags)).align();
    csv(&TableDiff::new(&align, flags).highlight())
}

fn patched(a: &str, b: &str, flags: Flags) -> String {
    let d = diff(None, a, b, flags);
    let mut t = parse(a);
    HighlightPatch::new(&mut t, &parse(&d)).apply().unwrap();
    csv(&t)
}

fn all() -> Flags {
    Flags { show_unchanged: true, show_unchanged_columns: true, ..Flags::default() }
}

fn merged(p: &str, l: &str, r: &str) -> (String, usize) {
    let (p, mut l, r) = (parse(p), parse(l), parse(r));
    let n = Merger::new(&p, &mut l, &r, Flags::default()).apply().unwrap();
    (csv(&l), n)
}

#[test]
fn b01_ndjson_ragged_rows() {
    let t = parse_ndjson("{\"a\":1,\"b\":2}\n{\"a\":3}\n{\"a\":5,\"c\":6}\n").unwrap();
    assert_eq!(csv(&t), "a,b,c\n1,2,NULL\n3,NULL,NULL\n5,NULL,6\n");
}

#[test]
fn b02_patch_with_duplicate_rows() {
    let a = "id,v\n1,a\n1,a\n2,b\n";
    let b = "id,v\n1,a\n2,b\n";
    assert_eq!(patched(a, b, Flags::default()), b);
}

#[test]
fn b03_patch_moved_row_with_deletion() {
    let a = "id,v\n1,a\n2,b\n3,c\n4,d\n5,e\n";
    let b = "id,v\n1,a\n5,e\n2,b\n3,c\n";
    assert_eq!(patched(a, b, Flags::default()), b);
}

#[test]
fn b04_ragged_csv_rows_keep_their_cells() {
    let t = Csv::default().parse_full("a,b\n1,2,3\n4\n").table;
    assert_eq!(csv(&t), "a,b,\n1,2,3\n4,,\n");
}

#[test]
fn b05_trim_keeps_columns_with_data() {
    let mut t = Csv::default().parse_full("a,b,\n1,2,\n3,4,\n5,6,7\n").table;
    t.trim_blank();
    assert_eq!(t.width(), 3);
    assert_eq!(t.get(2, 3), Some("7"));
}

#[test]
fn b06_null_like_strings_survive_csv() {
    let t = parse("x,y,z\nNULL,_NULL,__NULL\n");
    assert_eq!(t.get(0, 1), None);
    assert_eq!(t.get(1, 1), Some("NULL"));
    assert_eq!(csv(&t), "x,y,z\nNULL,_NULL,__NULL\n");
    // and through a diff
    assert_eq!(patched("id,v\n1,a\n", "id,v\n1,a\n2,_NULL\n", Flags::default()), "id,v\n1,a\n2,_NULL\n");
}

#[test]
fn b08_merge_keeps_edit_of_deleted_row() {
    let (out, n) = merged("id,v\n1,a\n2,b\n3,c\n", "id,v\n1,a\n2,B\n3,c\n", "id,v\n1,a\n3,c\n");
    assert_eq!((out.as_str(), n), ("id,v\n1,a\n2,B\n3,c\n", 1));
    let (out, n) = merged("id,v\n1,a\n2,b\n3,c\n", "id,v\n1,a\n3,c\n", "id,v\n1,a\n2,B\n3,c\n");
    assert_eq!((out.as_str(), n), ("id,v\n1,a\n2,B\n3,c\n", 1));
}

#[test]
fn b08_merge_keeps_edit_of_deleted_column() {
    let (out, n) = merged("id,v,w\n1,a,x\n", "id,v,w\n1,A,x\n", "id,w\n1,x\n");
    assert_eq!((out.as_str(), n), ("id,v,w\n1,A,x\n", 1));
}

#[test]
fn b09_merge_row_added_on_both_sides_once() {
    let (out, n) = merged("id,v\n1,a\n", "id,v\n1,a\n4,d\n", "id,v\n1,a\n4,d\n");
    assert_eq!((out.as_str(), n), ("id,v\n1,a\n4,d\n", 0));
}

#[test]
fn b10_patch_that_deletes_everything_does_not_abort() {
    let mut t = Table::default();
    let patch = parse("!,---\n@@,a\n---,x\n");
    HighlightPatch::new(&mut t, &patch).apply().unwrap();
}

#[test]
fn b11_absent_column_cells_are_blank() {
    let d = diff(None, "id,v,w\n1,a,z\n", "id,v\n1,a\n9,q\n", Flags::default());
    assert_eq!(d, "!,,,---\n@@,id,v,w\n,1,a,z\n+++,9,q,\n");
}

#[test]
fn b12_blank_values_of_an_added_column_stay_blank() {
    let (a, b) = ("c0,c1\n,x\n", "c0,c1,n\n,x,\n");
    assert_eq!(patched(a, b, all()), b);
    assert_eq!(patched(a, b, Flags::default()), b);
}

#[test]
fn b14_separator_is_chosen_per_row() {
    let d = diff(
        None,
        "a,b,c\nG,g,Green\nC,Toddlers -> Teenagers,White\n",
        "a,b,c\nG,g,Blue\nC,Toddlers -> Teenagers,Pale\n",
        Flags::default(),
    );
    assert_eq!(d, "@@,a,b,c\n->,G,g,Green->Blue\n-->,C,Toddlers -> Teenagers,White-->Pale\n");
}

#[test]
fn b16_literal_null_text_is_matched_like_any_text() {
    let d = diff(None, "id,v\nnull,x\n1,z\n", "id,v\nnull,X\n1,z\n", Flags::default());
    assert_eq!(d, "@@,id,v\n->,null,x->X\n,1,z\n");
}

#[test]
fn b17_epsilon_needs_whole_numbers() {
    let flags = Flags { ignore_epsilon: 0.01, ..Flags::default() };
    let d = diff(None, "id,v\n1,12abc\n2,3.14\n", "id,v\n1,12xyz\n2,3.1400001\n", flags);
    assert!(d.contains("12abc->12xyz"), "{d}");
    assert!(!d.contains("3.14->"), "{d}");
}

#[test]
fn b18_duplicate_header_names() {
    let d = diff(None, "a,a,b\n1,2,3\n", "a,a,b\n1,9,3\n", Flags::default());
    assert_eq!(d, "@@,a,a,b\n->,1,2->9,3\n");
}

#[test]
fn b19_json_columns_become_the_header_and_sheets_read_back() {
    let t = parse_table(r#"{"t":{"columns":["id","n"],"rows":[{"id":1,"n":"x"}]}}"#).unwrap().unwrap();
    assert_eq!((t.get(0, 0), t.get(1, 1)), (Some("id"), Some("x")));
    let back = parse_table(&render_json(&t)).unwrap().unwrap();
    assert!(back.is_similar(&t));
}

#[test]
fn b20_byte_order_mark_is_not_data() {
    let p = Csv::default().parse_full("\u{feff}id,v\n1,x\n");
    assert!(p.bom);
    assert_eq!(p.table.get(0, 0), Some("id"));
}

#[test]
fn b22_merge_honours_id_columns() {
    // rows matched by `id` although the first column differs in each version
    let (p, mut l, r) = (
        parse("v,id\nx,1\ny,2\n"),
        parse("v,id\nX,1\ny,2\n"),
        parse("v,id\nx,1\nZ,2\n"),
    );
    let mut flags = Flags::default();
    flags.add_primary_key("id");
    assert_eq!(Merger::new(&p, &mut l, &r, flags).apply().unwrap(), 0);
    assert_eq!(csv(&l), "v,id\nX,1\nZ,2\n");
}

#[test]
fn b26_table_names_are_escaped_in_html() {
    let mut tabs = daff::Tables::new();
    *tabs.add("base") = parse("@@,a\n");
    *tabs.add("<b>") = parse("@@,a\n,1\n");
    let mut r = daff::DiffRender::new();
    r.render_tables(&tabs);
    assert!(r.html().contains("<h3>&lt;b&gt;</h3>"));
}

fn apply(src: &str, diff_text: &str, force: bool) -> (String, Vec<daff::PatchIssue>) {
    let mut t = parse(src);
    let d = parse(diff_text);
    let mut p = HighlightPatch::new(&mut t, &d);
    p.force(force);
    p.apply().unwrap();
    let issues = p.issues().to_vec();
    (csv(&t), issues)
}

const BASE: &str = "id,v\n1,a\n2,b\n3,c\n";
const WANT: &str = "id,v\n1,a\n2,B\n3,c\n4,d\n";

fn base_diff() -> String {
    diff(None, BASE, WANT, Flags::default())
}

#[test]
fn b24_applying_a_diff_twice_changes_nothing_and_reports_nothing() {
    let (once, issues) = apply(BASE, &base_diff(), false);
    assert_eq!((once.as_str(), issues.len()), (WANT, 0));
    let (twice, issues) = apply(&once, &base_diff(), false);
    assert_eq!((twice.as_str(), issues.len()), (WANT, 0));
}

#[test]
fn b24_changed_cell_is_left_alone_and_reported() {
    let (out, issues) = apply("id,v\n1,a\n2,CHANGED\n3,c\n", &base_diff(), false);
    assert_eq!(out, "id,v\n1,a\n2,CHANGED\n3,c\n4,d\n");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].kind, daff::PatchIssueKind::CellChanged);
    assert_eq!(issues[0].patch_row, 2);
    // --force overwrites as the reference does
    let (out, issues) = apply("id,v\n1,a\n2,CHANGED\n3,c\n", &base_diff(), true);
    assert_eq!((out.as_str(), issues.len()), (WANT, 0));
}

#[test]
fn b24_update_of_a_missing_row_is_reported() {
    let (out, issues) = apply("id,v\n1,a\n3,c\n", &base_diff(), false);
    assert_eq!(out, "id,v\n1,a\n3,c\n4,d\n");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].kind, daff::PatchIssueKind::RowNotFound);
}

#[test]
fn b24_changed_row_is_not_deleted() {
    let d = diff(None, "id,v,w\n1,a,x\n2,b,y\n", "id,v,w\n1,a,x\n", Flags::default());
    let (out, issues) = apply("id,v,w\n1,a,x\n2,b,CHANGED\n", &d, false);
    assert_eq!(out, "id,v,w\n1,a,x\n2,b,CHANGED\n");
    assert_eq!(issues[0].kind, daff::PatchIssueKind::RowChanged);
    // a row that is already gone is fine
    let (out, issues) = apply("id,v,w\n1,a,x\n", &d, false);
    assert_eq!((out.as_str(), issues.len()), ("id,v,w\n1,a,x\n", 0));
}

#[test]
fn b24_conflict_in_a_three_way_diff_is_reported() {
    let d = diff(Some("id,v\n1,a\n"), "id,v\n1,L\n", "id,v\n1,R\n", Flags::default());
    let (out, issues) = apply("id,v\n1,L\n", &d, false);
    assert_eq!(out, "id,v\n1,L\n");
    assert_eq!(issues[0].kind, daff::PatchIssueKind::Conflict);
}

#[test]
fn b24_column_changes_are_not_made_twice() {
    let (a, b) = ("id,v\n1,a\n", "id,v,n\n1,a,x\n");
    let d = diff(None, a, b, Flags::default());
    let (once, _) = apply(a, &d, false);
    assert_eq!(once, b);
    let (twice, issues) = apply(&once, &d, false);
    assert_eq!((twice.as_str(), issues.len()), (b, 0));
}

#[test]
fn b28_index_diffs_apply() {
    let flags = Flags { always_show_order: true, never_show_order: false, ..Flags::default() };
    let d = diff(None, BASE, WANT, flags);
    assert!(d.starts_with("@:@"), "{d}");
    assert_eq!(apply(BASE, &d, false).0, WANT);
}

#[test]
fn b29_rows_and_columns_before_the_header_are_skipped() {
    let want = "id,v\n1,a\n2,B\n3,c\n";
    for d in [
        "title\n@@,id,v\n,1,a\n->,2,b->B\n",
        "title,,\n@@,id,v\n,1,a\n->,2,b->B\n",
        "note,@@,id,v\nx,,1,a\ny,->,2,b->B\n",
        "@@,id,v\n@mode@,ordered,\n,1,a\n->,2,b->B\n",
    ] {
        let (out, issues) = apply(BASE, d, false);
        assert_eq!((out.as_str(), issues.len()), (want, 0), "{d}");
    }
}

#[test]
fn b30_a_diff_without_a_header_is_reported() {
    let (out, issues) = apply(BASE, "just,junk\n1,2\n", false);
    assert_eq!(out, BASE);
    assert_eq!(issues[0].kind, daff::PatchIssueKind::NoHeader);
}
