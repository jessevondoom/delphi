//! Jupyter notebooks as a readable transcript: Markdown cells as text, code cells fenced, outputs trimmed,
//! images counted and not shown (`specs/014` §4).

use crate::sniff::Format;
use crate::{DocError, Extract, Limits, Unit};
use serde_json::Value;

const MAX_OUTPUT_LINES: usize = 40;

fn source(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter_map(|x| x.as_str()).collect::<String>(),
        _ => String::new(),
    }
}

fn clip(s: &str) -> (String, bool) {
    let lines: Vec<&str> = s.lines().collect();
    if lines.len() > MAX_OUTPUT_LINES {
        (format!("{}\n… {} more line(s)", lines[..MAX_OUTPUT_LINES].join("\n"), lines.len() - MAX_OUTPUT_LINES), true)
    } else {
        (s.trim_end().to_string(), false)
    }
}

pub fn extract(bytes: &[u8], limits: &Limits) -> Result<Extract, DocError> {
    let body = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    let v: Value = serde_json::from_slice(body).map_err(|e| DocError(format!("not a readable notebook (invalid JSON: {})", e)))?;
    // nbformat 4 keeps cells at the top; nbformat 3 nests them in worksheets
    let cells: Vec<Value> = match v.get("cells").and_then(|c| c.as_array()) {
        Some(c) => c.clone(),
        None => v.pointer("/worksheets/0/cells").and_then(|c| c.as_array()).cloned().unwrap_or_default(),
    };
    let lang = v.pointer("/metadata/kernelspec/language").and_then(|l| l.as_str()).or_else(|| v.pointer("/metadata/language_info/name").and_then(|l| l.as_str())).unwrap_or("python").to_string();
    let mut out = format!("Notebook · {} cell{} · {}\n\n", cells.len(), if cells.len() == 1 { "" } else { "s" }, lang);
    let (mut images, mut other, mut clipped) = (0usize, 0usize, 0usize);
    for (i, c) in cells.iter().enumerate() {
        let kind = c.get("cell_type").and_then(|k| k.as_str()).unwrap_or("raw");
        let src = source(c.get("source").or_else(|| c.get("input")).unwrap_or(&Value::Null));
        match kind {
            "markdown" | "heading" => out.push_str(&format!("{}\n\n", src.trim_end())),
            "code" => {
                let n = c.get("execution_count").or_else(|| c.get("prompt_number")).and_then(|n| n.as_u64());
                out.push_str(&format!("[{}] code{}\n```{}\n{}\n```\n", i, n.map(|n| format!(" (run {})", n)).unwrap_or_default(), lang, src.trim_end()));
                for o in c.get("outputs").and_then(|o| o.as_array()).into_iter().flatten() {
                    match o.get("output_type").and_then(|t| t.as_str()).unwrap_or("") {
                        "stream" => {
                            let (t, cut) = clip(&source(o.get("text").unwrap_or(&Value::Null)));
                            clipped += cut as usize;
                            out.push_str(&format!("output:\n```\n{}\n```\n", t));
                        }
                        "execute_result" | "display_data" | "pyout" => {
                            let data = o.get("data").unwrap_or(o);
                            if let Some(t) = data.get("text/plain").or_else(|| data.get("text")) {
                                let (t, cut) = clip(&source(t));
                                clipped += cut as usize;
                                out.push_str(&format!("result:\n```\n{}\n```\n", t));
                            }
                            if let Some(m) = data.as_object() {
                                images += m.keys().filter(|k| k.starts_with("image/")).count();
                                other += m.keys().filter(|k| !k.starts_with("image/") && *k != "text/plain" && *k != "text").count();
                            }
                        }
                        "error" | "pyerr" => {
                            out.push_str(&format!("error: {}: {}\n", o.get("ename").and_then(|e| e.as_str()).unwrap_or("Error"), o.get("evalue").and_then(|e| e.as_str()).unwrap_or("")));
                        }
                        _ => other += 1,
                    }
                }
                out.push('\n');
            }
            _ => out.push_str(&format!("{}\n\n", src.trim_end())),
        }
    }
    let mut loss = vec![];
    if images > 0 {
        loss.push(format!("{} image output(s) not shown", images));
    }
    if other > 0 {
        loss.push(format!("{} rich output(s) (HTML, widgets, JSON) not shown", other));
    }
    if clipped > 0 {
        loss.push(format!("{} long output(s) cut at {} lines", clipped, MAX_OUTPUT_LINES));
    }
    let mut text = out.trim_end().to_string();
    let truncated = text.len() > limits.max_output;
    if truncated {
        let mut n = limits.max_output;
        while !text.is_char_boundary(n) {
            n -= 1;
        }
        text.truncate(n);
    }
    Ok(Extract { format: Format::Notebook, text, unit: Unit::Line, total_units: Some(cells.len()), truncated, loss, supported: true })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cells_outputs_errors_and_images() {
        let nb = r##"{"metadata":{"kernelspec":{"language":"python"}},"cells":[
          {"cell_type":"markdown","source":["# Title\n","text"]},
          {"cell_type":"code","execution_count":3,"source":"print(1)\nx=2","outputs":[
             {"output_type":"stream","name":"stdout","text":["1\n"]},
             {"output_type":"execute_result","data":{"text/plain":"2","image/png":"AAAA","text/html":"<b>2</b>"}},
             {"output_type":"error","ename":"ValueError","evalue":"bad","traceback":[]}]},
          {"cell_type":"code","source":"y","outputs":[{"output_type":"stream","text":"l\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n\n"}]}]}"##;
        let e = extract(nb.as_bytes(), &Limits::default()).unwrap();
        assert!(e.text.starts_with("Notebook · 3 cells · python\n\n# Title\ntext\n\n[1] code (run 3)\n```python\nprint(1)\nx=2\n```\noutput:\n```\n1\n```\nresult:\n```\n2\n```\nerror: ValueError: bad"), "{}", e.text);
        let loss = e.loss.join(" | ");
        assert!(loss.contains("1 image output(s)") && loss.contains("1 rich output(s)") && loss.contains("cut at 40 lines"), "{}", loss);
    }

    #[test]
    fn old_formats_and_junk() {
        let v3 = r#"{"worksheets":[{"cells":[{"cell_type":"code","input":["a"],"prompt_number":1,"outputs":[{"output_type":"pyout","text":["b"]}]}]}]}"#;
        assert!(extract(v3.as_bytes(), &Limits::default()).unwrap().text.contains("[0] code (run 1)\n```python\na\n```\nresult:\n```\nb"));
        assert!(extract(b"not json", &Limits::default()).unwrap_err().0.contains("invalid JSON"));
        assert!(extract(b"{}", &Limits::default()).unwrap().text.contains("0 cells"));
    }
}
