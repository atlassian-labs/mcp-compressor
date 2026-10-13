//! Opt-in, recoverable previews for string-based command transports.
use std::io::Write;
use std::path::Path;

use rmcp::model::{CallToolResult, ContentBlock};
use serde::Serialize;
use serde_json::value::RawValue;
use shrinkfit::{ArrayStrategy, Format, InputFormat, Options};

use crate::Error;

#[derive(Serialize)]
struct Preview<'a> {
    preview: &'a RawValue,
    recovery: Recovery<'a>,
}

#[derive(Serialize)]
struct Recovery<'a> {
    original_response_file: &'a str,
    original_format: &'static str,
    max_chars: usize,
    fits: bool,
    pruned: bool,
    notice: &'static str,
}

/// Keep the original MCP envelope, including typed blocks and structured content.
/// No backup is created for small outputs, errors, or unsupported media content.
pub(super) fn shrink_output(
    result: &CallToolResult,
    output: &str,
    max_chars: usize,
    directory: &Path,
) -> Result<Option<String>, Error> {
    if result.is_error == Some(true)
        || output.chars().count() <= max_chars
        || (result.structured_content.is_none()
            && result
                .content
                .iter()
                .any(|c| !matches!(c, ContentBlock::Text(_))))
    {
        return Ok(None);
    }

    let mut file = tempfile::Builder::new()
        .prefix("mcp-compressor-response-")
        .suffix(".json")
        .tempfile_in(directory)?;
    serde_json::to_writer(file.as_file_mut(), result)?;
    file.flush()?;
    let path = file
        .path()
        .to_str()
        .ok_or_else(|| Error::Config("recovery file path must be valid UTF-8".to_string()))?
        .to_owned();

    let mut options = Options::new(max_chars);
    options.format = Format::Json;
    options.input_format = InputFormat::Json;
    options.indent = 0;
    options.array_strategy = ArrayStrategy::Condense;
    // Never interpret prose or malformed JSON as YAML. A JSON string still gets
    // shrinkfit's section-aware text pruning and preserves its original backup.
    let input = match shrinkfit::json::parse(output) {
        Ok(value) => value,
        Err(_) => shrinkfit::Value::String(output.to_owned()),
    };
    let mut shrunk = shrinkfit::shrink(&input, &options);
    let notice = "Lossy preview; read original_response_file for the complete MCP result. Delete that private file when finished.";
    let render = |shrunk: &shrinkfit::Shrunk| -> Result<String, Error> {
        let raw = RawValue::from_string(shrunk.text.clone())?;
        Ok(serde_json::to_string(&Preview {
            preview: &raw,
            recovery: Recovery {
                original_response_file: &path,
                original_format: "mcp-call-tool-result",
                max_chars,
                fits: shrunk.report.fits,
                pruned: shrunk.report.pruned,
                notice,
            },
        })?)
    };
    // Reserve the wrapper and recovery path too, not just the preview body.
    let first = render(&shrunk)?;
    let overhead = first
        .chars()
        .count()
        .saturating_sub(shrunk.text.chars().count());
    if overhead >= max_chars {
        return Err(Error::Config(format!(
            "--shrinkfit budget {max_chars} is too small for recovery metadata"
        )));
    }
    options.max_chars = max_chars - overhead;
    shrunk = shrinkfit::shrink(&input, &options);
    let mut response = render(&shrunk)?;
    // Boolean lengths in the report can change by one character after pruning.
    if response.chars().count() > max_chars && options.max_chars > 1 {
        options.max_chars = options
            .max_chars
            .saturating_sub(response.chars().count() - max_chars + 1);
        shrunk = shrinkfit::shrink(&input, &options);
        response = render(&shrunk)?;
    }
    if !shrunk.report.fits || response.chars().count() > max_chars {
        return Err(Error::Config(
            "shrinkfit could not fit the response and recovery metadata into the requested budget"
                .to_string(),
        ));
    }
    // Persist only after successful serialization; on errors NamedTempFile cleans up.
    // keep() retains the file after the proxy session exits for later recovery.
    file.keep().map_err(|e| Error::Io(e.error))?;
    Ok(Some(response))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::ContentBlock as Content;
    use serde_json::json;

    #[test]
    fn oversized_response_is_recoverable_and_wrapper_fits() {
        let dir = tempfile::tempdir().unwrap();
        let text = json!({"items": (0..100).map(|i| json!({"id":i,"text":"detail ".repeat(100)})).collect::<Vec<_>>()}).to_string();
        let mut result = CallToolResult::success(vec![Content::text(&text)]);
        result.structured_content = Some(json!({"original": "structured"}));
        let output = shrink_output(&result, &text, 4000, dir.path())
            .unwrap()
            .unwrap();
        assert!(output.chars().count() <= 4000);
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        let path = value["recovery"]["original_response_file"]
            .as_str()
            .unwrap();
        let recovered: CallToolResult =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(recovered).unwrap(),
            serde_json::to_value(result).unwrap()
        );
        assert_eq!(value["recovery"]["fits"], true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn small_unicode_output_and_errors_are_unchanged_without_files() {
        let dir = tempfile::tempdir().unwrap();
        let text = "😀".repeat(1000);
        let result = CallToolResult::success(vec![Content::text(&text)]);
        assert!(shrink_output(&result, &text, 1000, dir.path())
            .unwrap()
            .is_none());
        let error = CallToolResult::error(vec![Content::text(&text)]);
        assert!(shrink_output(&error, &text, 100, dir.path())
            .unwrap()
            .is_none());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn plain_text_and_malformed_json_remain_strings() {
        let dir = tempfile::tempdir().unwrap();
        for prefix in ["Description: ", "{invalid JSON: "] {
            let text = prefix.to_string() + &"hello world ".repeat(1000);
            let result = CallToolResult::success(vec![Content::text(&text)]);
            let output = shrink_output(&result, &text, 2000, dir.path())
                .unwrap()
                .unwrap();
            let value: serde_json::Value = serde_json::from_str(&output).unwrap();
            assert!(value["preview"].is_string());
            assert!(value["recovery"]["pruned"].as_bool().unwrap());
        }
    }

    #[test]
    fn media_bypasses_pruning_unless_structured_content_is_available() {
        let dir = tempfile::tempdir().unwrap();
        let mut result: CallToolResult = serde_json::from_value(json!({
            "content": [{"type":"image", "data":"aW1hZ2U=", "mimeType":"image/png"}],
            "isError": false
        }))
        .unwrap();
        let text = "large ".repeat(1000);
        assert!(shrink_output(&result, &text, 2000, dir.path())
            .unwrap()
            .is_none());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
        result.structured_content = Some(json!({"text": text}));
        assert!(shrink_output(&result, &text, 2000, dir.path())
            .unwrap()
            .is_some());
    }

    #[test]
    fn storage_failures_and_impossible_budgets_do_not_return_lossy_success() {
        let dir = tempfile::tempdir().unwrap();
        let text = "long ".repeat(1000);
        let result = CallToolResult::success(vec![Content::text(&text)]);
        assert!(shrink_output(&result, &text, 2000, &dir.path().join("missing")).is_err());
        assert!(shrink_output(&result, &text, 1, dir.path()).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn repeated_calls_have_distinct_files_and_preserve_large_json_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let text = format!(
            "{{\"id\":9007199254740993,\"body\":\"{}\"}}",
            "x".repeat(10000)
        );
        let result = CallToolResult::success(vec![Content::text(&text)]);
        let mut paths = Vec::new();
        for _ in 0..2 {
            let output = shrink_output(&result, &text, 2000, dir.path())
                .unwrap()
                .unwrap();
            assert!(output.contains("9007199254740993"));
            let value: serde_json::Value = serde_json::from_str(&output).unwrap();
            paths.push(
                value["recovery"]["original_response_file"]
                    .as_str()
                    .unwrap()
                    .to_string(),
            );
        }
        assert_ne!(paths[0], paths[1]);
    }
}
