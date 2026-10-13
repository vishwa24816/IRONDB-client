//! Output renderers: jsonl|json|csv|md over QueryResult rows.

use irondb_contracts::QueryResult;

pub fn render(result: &QueryResult, format: &str) -> Result<String, String> {
    match format.to_ascii_lowercase().as_str() {
        "json" => serde_json::to_string_pretty(result).map_err(|e| e.to_string()),
        "jsonl" => {
            let rows = rows_of(result);
            let mut out = String::new();
            for r in rows {
                out.push_str(&serde_json::to_string(&r).map_err(|e| e.to_string())?);
                out.push('\n');
            }
            Ok(out)
        }
        "csv" => Ok(render_csv(result)),
        "md" => Ok(render_md(result)),
        other => Err(format!("unsupported format {other}")),
    }
}

fn rows_of(result: &QueryResult) -> Vec<serde_json::Value> {
    result
        .data
        .as_ref()
        .and_then(|d| d.get("rows"))
        .and_then(|r| r.as_array())
        .cloned()
        .unwrap_or_default()
}

fn columns_of(result: &QueryResult) -> Vec<String> {
    result.fields.clone().unwrap_or_default()
}

fn cell(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn render_csv(result: &QueryResult) -> String {
    let cols = columns_of(result);
    let mut out = String::new();
    out.push_str(&cols.join(","));
    out.push('\n');
    for row in rows_of(result) {
        let line: Vec<String> = cols
            .iter()
            .map(|c| {
                let v = row.get(c).unwrap_or(&serde_json::Value::Null);
                let s = cell(v);
                if s.contains([',', '"', '\n']) {
                    format!("\"{}\"", s.replace('"', "\"\""))
                } else {
                    s
                }
            })
            .collect();
        out.push_str(&line.join(","));
        out.push('\n');
    }
    out
}

fn render_md(result: &QueryResult) -> String {
    let cols = columns_of(result);
    let mut out = String::new();
    out.push_str(&format!("| {} |\n", cols.join(" | ")));
    out.push_str(&format!("|{}|\n", cols.iter().map(|_| "---").collect::<Vec<_>>().join("|")));
    for row in rows_of(result) {
        let line: Vec<String> = cols
            .iter()
            .map(|c| cell(row.get(c).unwrap_or(&serde_json::Value::Null)))
            .collect();
        out.push_str(&format!("| {} |\n", line.join(" | ")));
    }
    out
}
