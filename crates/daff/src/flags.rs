use std::collections::BTreeMap;

/// Options controlling diff/patch/merge, ported from `coopy.CompareFlags`.
#[derive(Debug, Clone)]
pub struct Flags {
    pub ordered: bool,
    pub show_unchanged: bool,
    pub unchanged_context: usize,
    pub always_show_order: bool,
    pub never_show_order: bool,
    pub show_unchanged_columns: bool,
    pub unchanged_column_context: usize,
    pub always_show_header: bool,
    /// `None` means every kind of change is allowed.
    pub acts: Option<BTreeMap<String, bool>>,
    pub ids: Option<Vec<String>>,
    pub columns_to_ignore: Option<Vec<String>>,
    pub tables: Option<Vec<String>>,
    pub allow_nested_cells: bool,
    pub show_meta: bool,
    pub show_unchanged_meta: bool,
    pub count_like_a_spreadsheet: bool,
    pub ignore_whitespace: bool,
    pub ignore_case: bool,
    /// Negative disables epsilon comparison.
    pub ignore_epsilon: f64,
    pub use_glyphs: bool,
    pub quote_html: bool,
}

impl Default for Flags {
    fn default() -> Self {
        Flags {
            ordered: true,
            show_unchanged: false,
            unchanged_context: 1,
            always_show_order: false,
            never_show_order: true,
            show_unchanged_columns: false,
            unchanged_column_context: 1,
            always_show_header: true,
            acts: None,
            ids: None,
            columns_to_ignore: None,
            tables: None,
            allow_nested_cells: false,
            show_meta: true,
            show_unchanged_meta: false,
            count_like_a_spreadsheet: true,
            ignore_whitespace: false,
            ignore_case: false,
            ignore_epsilon: -1.0,
            use_glyphs: true,
            quote_html: true,
        }
    }
}

impl Flags {
    /// Allow or forbid one kind of change (`update`, `insert`, `delete`, `column`).
    /// The first call initialises every kind to the opposite of `allow`.
    pub fn filter(&mut self, act: &str, allow: bool) -> bool {
        let acts = self.acts.get_or_insert_with(|| {
            ["update", "insert", "delete", "column"]
                .iter()
                .map(|a| (a.to_string(), !allow))
                .collect()
        });
        match acts.get_mut(act) {
            Some(v) => {
                *v = allow;
                true
            }
            None => false,
        }
    }

    fn allows(&self, act: &str) -> bool {
        match &self.acts {
            None => true,
            Some(m) => m.get(act).copied().unwrap_or(false),
        }
    }

    pub fn allow_update(&self) -> bool {
        self.allows("update")
    }
    pub fn allow_insert(&self) -> bool {
        self.allows("insert")
    }
    pub fn allow_delete(&self) -> bool {
        self.allows("delete")
    }
    pub fn allow_column(&self) -> bool {
        self.allows("column")
    }

    pub fn add_primary_key(&mut self, column: &str) {
        self.ids.get_or_insert_with(Vec::new).push(column.into());
    }

    pub fn ignore_column(&mut self, column: &str) {
        self.columns_to_ignore.get_or_insert_with(Vec::new).push(column.into());
    }

    pub fn add_table(&mut self, table: &str) {
        self.tables.get_or_insert_with(Vec::new).push(table.into());
    }

    pub(crate) fn is_ignored(&self, name: &str) -> bool {
        self.columns_to_ignore.as_ref().is_some_and(|v| v.iter().any(|c| c == name))
    }
}
