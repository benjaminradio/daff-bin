/// Options controlling diff/patch/merge, mirroring `daff.CompareFlags`.
#[derive(Debug, Clone, Default)]
pub struct Flags {
    /// Columns that identify a row (`--id`).
    pub ids: Vec<String>,
    pub ordered: bool,
    pub show_unchanged: bool,
    pub show_unchanged_columns: bool,
    pub unchanged_context: usize,
    pub always_show_header: bool,
    pub allow_nested_cells: bool,
}
