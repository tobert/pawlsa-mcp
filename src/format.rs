/// Columnar text format using `, ` as column separator, ` ▌ ` as row separator.
/// Header row states field names once, then data rows follow.
/// Designed to be token-efficient and readable in raw JSON strings
/// where \n renders as literal backslash-n.
pub struct Table {
    headers: Vec<&'static str>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new(headers: &[&'static str]) -> Self {
        Self {
            headers: headers.to_vec(),
            rows: Vec::new(),
        }
    }

    pub fn row(&mut self, values: &[&str]) {
        self.rows.push(
            values
                .iter()
                .map(|v| {
                    if v.is_empty() {
                        "N/A".to_string()
                    } else {
                        (*v).to_string()
                    }
                })
                .collect(),
        );
    }

    pub fn render(&self) -> String {
        let mut out = self.headers.join(", ");
        for row in &self.rows {
            out.push_str(" ▌ ");
            out.push_str(&row.join(", "));
        }
        out
    }
}
