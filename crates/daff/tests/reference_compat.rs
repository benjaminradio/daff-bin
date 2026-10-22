//! Golden tests of reference-compatible behaviour: expected outputs were
//! produced by the reference daff 1.4.2 (`scripts/fuzz_oracle.py` compares
//! against it on random inputs). Every test turns the compat switch on, and
//! this file is its own test binary so the switch cannot leak.
use daff::format::csv::Csv;
use daff::{compare_tables3, Flags, Table, TableDiff};

fn compat() {
    daff::set_reference_compat(true);
}

fn parse(s: &str) -> Table {
    let (mut t, _) = Csv::default().parse(s);
    t.trim_blank();
    t
}

fn diff(parent: Option<&str>, a: &str, b: &str, flags: Flags) -> String {
    let (p, a, b) = (parent.map(parse), parse(a), parse(b));
    let align = compare_tables3(p.as_ref(), &a, &b, Some(&flags)).align();
    let out = TableDiff::new(&align, flags).highlight();
    Csv { delimiter: ",".into(), eol: Some("\n".into()) }.render_table(&out)
}

#[test]
fn basic_update_insert_delete_and_column_add() {
        compat();
    let got = diff(
        None,
        "name,age\nann,3\nbob,4\ncat,5\n",
        "name,age,x\nann,3,1\nbob,5,2\ndan,9,3\n",
        Flags::default(),
    );
    assert_eq!(
        got,
        "!,,,+++\n@@,name,age,x\n+,ann,3,1\n->,bob,4->5,2\n+++,dan,9,3\n---,cat,5,NULL\n"
    );
}

#[test]
fn identical_tables_show_only_header() {
        compat();
    let got = diff(None, "a,b\n1,2\n", "a,b\n1,2\n", Flags::default());
    assert_eq!(got, "@@,a,b\n");
}

#[test]
fn three_way_column_reorder_matches_reference() {
        compat();
    let got = diff(
        Some("c0,c1,c2\n,b,NULL\n"),
        "c1,ren7,new3\nb,NULL,a\n",
        "c0,c1,new4,c2\n",
        Flags::default(),
    );
    assert_eq!(got, "!,,+++,,,,...\n@@,c0,new4,c2,c1,ren7,...\n---,,NULL,NULL,b,NULL,...\n");
}

#[test]
fn id_column_diff_with_context() {
        compat();
    let mut f = Flags::default();
    f.add_primary_key("id");
    let got = diff(None, "id,x,y\n1,a,b\n2,c,d\n", "id,x,y\n1,a,b\n2,c,e\n", f);
    assert_eq!(got, "@@,id,x,y\n,1,a,b\n->,2,c,d->e\n");
}

mod patch_and_merge {
    use super::*;
    use super::compat;
    use daff::{HighlightPatch, Merger};

    fn csv(t: &Table) -> String {
        Csv { delimiter: ",".into(), eol: Some("\n".into()) }.render_table(t)
    }

    #[test]
    fn patch_round_trips_a_diff() {
        compat();
        let a = "name,age\nann,3\nbob,4\ncat,5\n";
        let b = "name,age,x\nann,3,1\nbob,5,2\ndan,9,3\n";
        let d = diff(None, a, b, Flags::default());
        let (mut src, patch) = (parse(a), parse(&d));
        HighlightPatch::new(&mut src, &patch).apply().unwrap();
        assert_eq!(csv(&src), "name,age,x\nann,3,1\nbob,5,2\ndan,9,3\n");
    }

    #[test]
    fn merge_applies_remote_changes_and_marks_conflicts() {
        compat();
        let (p, mut l, r) = (
            parse("id,v\n1,a\n2,b\n"),
            parse("id,v\n1,A\n2,b\n"),
            parse("id,v\n1,Z\n2,c\n"),
        );
        let mut m = Merger::new(&p, &mut l, &r, Flags::default());
        assert_eq!(m.apply().unwrap(), 1);
        assert_eq!(m.conflict_infos().len(), 1);
        assert_eq!(csv(&l), "id,v\n1,((( a ))) A /// Z\n2,c\n");
    }

    #[test]
    fn empty_table_is_written_as_one_line() {
        compat();
        assert_eq!(csv(&Table::new(0, 0)), "\n");
    }
}

mod json {
    use super::compat;
    use daff::format::json::{parse_table, render_json};
    use daff::{compare_tables, Flags, TableDiff};

    #[test]
    fn nested_update_cells_keep_types() {
        compat();
        let src = |n: &str| {
            format!(r#"{{"t":{{"columns":["id","n"],"rows":[{{"id":"id","n":"n"}},{{"id":1,"n":{n}}}]}}}}"#)
        };
        let (a, b) = (parse_table(&src("1.5")).unwrap().unwrap(), parse_table(&src("2")).unwrap().unwrap());
        let flags = Flags { allow_nested_cells: true, ordered: false, unchanged_context: 0, ..Flags::default() };
        let align = compare_tables(&a, &b, Some(&flags)).align();
        let out = render_json(&TableDiff::new(&align, flags).highlight());
        let compact: String = out.chars().filter(|c| !c.is_whitespace()).collect();
        assert_eq!(compact, r#"{"sheet":[["@@","id","n"],["->",1,{"before":1.5,"after":2}]]}"#);
    }
}
