//! Golden tests; expected outputs were produced by the reference daff 1.4.2
//! (`scripts/fuzz_oracle.py` compares against it on random inputs).
use daff::format::csv::Csv;
use daff::{compare_tables3, Flags, Table, TableDiff};

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
    let got = diff(None, "a,b\n1,2\n", "a,b\n1,2\n", Flags::default());
    assert_eq!(got, "@@,a,b\n");
}

#[test]
fn three_way_column_reorder_matches_reference() {
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
    let mut f = Flags::default();
    f.add_primary_key("id");
    let got = diff(None, "id,x,y\n1,a,b\n2,c,d\n", "id,x,y\n1,a,b\n2,c,e\n", f);
    assert_eq!(got, "@@,id,x,y\n,1,a,b\n->,2,c,d->e\n");
}
