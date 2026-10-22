//! Classification of a diff cell, ported from `coopy.CellInfo` and
//! `DiffRender.examineCell` (non-nested tables only).

#[derive(Debug, Clone, Default)]
pub struct CellInfo {
    pub value: Option<String>,
    pub pretty_value: Option<String>,
    pub category: String,
    pub category_given_tr: String,
    pub separator: String,
    pub pretty_separator: String,
    pub updated: bool,
    pub conflicted: bool,
    pub pvalue: Option<String>,
    pub lvalue: Option<String>,
    pub rvalue: Option<String>,
    pub meta: Option<String>,
}

fn mark_spaces(sl: &str, sr: &str) -> String {
    if sl == sr {
        return sl.into();
    }
    if sl.replace(' ', "") != sr.replace(' ', "") {
        return sl.into();
    }
    let (l, r): (Vec<char>, Vec<char>) = (sl.chars().collect(), sr.chars().collect());
    let (mut il, mut ir) = (0, 0);
    let mut out = String::new();
    while il < l.len() {
        let cr = r.get(ir).copied();
        if Some(l[il]) == cr {
            out.push(l[il]);
            il += 1;
            ir += 1;
        } else if cr == Some(' ') {
            ir += 1;
        } else {
            out.push('\u{2423}');
            il += 1;
        }
    }
    out
}

/// Work out what a cell means given its column header (`vcol`), the action in
/// its row (`vrow`), and its content (`raw`). Mirrors `DiffRender.examineCell`.
pub fn examine_cell(
    x: usize,
    y: usize,
    raw: Option<&str>,
    vcol: &str,
    vrow: &str,
    offset: usize,
) -> CellInfo {
    let mut cell = CellInfo { value: raw.map(String::from), pretty_value: raw.map(String::from), ..Default::default() };
    let mut vrow = vrow.to_string();
    let chars: Vec<char> = vrow.chars().collect();
    if chars.len() >= 3 && chars[0] == '@' && chars[1] != '@' {
        if let Some(idx) = chars.iter().skip(1).position(|&c| c == '@').map(|i| i + 1) {
            cell.meta = Some(chars[1..idx].iter().collect());
            vrow = chars[idx + 1..].iter().collect();
            cell.category = "meta".into();
        }
    }
    let mut removed_column = false;
    if vrow == ":" {
        cell.category = "move".into();
    }
    if vrow.is_empty() && offset == 1 && y == 0 {
        cell.category = "index".into();
    }
    if vcol.contains("+++") {
        cell.category = "add".into();
        cell.category_given_tr = "add".into();
    } else if vcol.contains("---") {
        cell.category = "remove".into();
        cell.category_given_tr = "remove".into();
        removed_column = true;
    }
    match vrow.as_str() {
        "!" => cell.category = "spec".into(),
        "@@" => cell.category = "header".into(),
        "..." => cell.category = "gap".into(),
        "+++" => {
            if !removed_column {
                cell.category = "add".into();
            }
        }
        "---" => cell.category = "remove".into(),
        _ if vrow.contains("->") => {
            if !removed_column {
                let full = vrow.as_str();
                let part = full.split('!').nth(1).unwrap_or(full);
                let s = raw.unwrap_or("");
                if s.contains(part) {
                    let mut cat = "modify";
                    let mut div = part;
                    if part != full {
                        cell.conflicted = s.contains(full);
                        if cell.conflicted {
                            div = full;
                            cat = "conflict";
                        }
                    }
                    cell.updated = true;
                    cell.separator = div.into();
                    cell.pretty_separator = div.into();
                    let pv = raw.unwrap_or("");
                    let tokens: Vec<String> =
                        if pv == div { vec![String::new(), String::new()] } else { pv.split(div).map(String::from).collect() };
                    let mut pretty = tokens.clone();
                    if tokens.len() >= 2 {
                        pretty[0] = mark_spaces(&tokens[0], &tokens[1]);
                        pretty[1] = mark_spaces(&tokens[1], &tokens[0]);
                    }
                    if tokens.len() >= 3 {
                        let r = pretty[0].clone();
                        pretty[0] = mark_spaces(&r, &tokens[2]);
                        pretty[2] = mark_spaces(&tokens[2], &r);
                    }
                    cell.pretty_separator = "\u{2192}".into();
                    cell.pretty_value = Some(pretty.join(&cell.pretty_separator));
                    cell.category = cat.into();
                    cell.category_given_tr = cat.into();
                    let off = usize::from(cell.conflicted);
                    cell.lvalue = tokens.get(off).cloned();
                    cell.rvalue = tokens.get(off + 1).cloned();
                    if cell.conflicted {
                        cell.pvalue = tokens.first().cloned();
                    }
                }
            }
        }
        _ => {}
    }
    if x == 0 && offset > 0 {
        cell.category = "index".into();
        cell.category_given_tr = "index".into();
    }
    cell
}
