//! Mode-aware recovery metadata without wrapping object payloads.
use crate::Error;
use rmcp::model::{CallToolResult, ContentBlock};
use serde_json::{json, Value};
use shrinkfit::{ArrayStrategy, Format, InputFormat, Options};
use std::io::Write;
use std::path::Path;

pub(super) const FULL_DESCRIPTION: &str = "Return the complete original only when the condensed response lacks information required for the task. Prefer the default preview to limit token consumption. Reuse the same tool_name and tool_input. Uses the session cache if available; a miss runs the backend again without shrinking, so do not repeat a write unless safe.";

pub(super) fn shrink_result(
    result: &CallToolResult,
    output: &str,
    max_chars: usize,
    directory: Option<&Path>,
) -> Result<Option<CallToolResult>, Error> {
    shrink_result_with_recovery(result, output, max_chars, directory, None)
}

/// Hosts supply recovery instructions because their consumers may not share a filesystem.
pub(crate) fn shrink_result_with_recovery(
    result: &CallToolResult,
    output: &str,
    max_chars: usize,
    directory: Option<&Path>,
    recovery_message: Option<&str>,
) -> Result<Option<CallToolResult>, Error> {
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
    let mut file = if let Some(dir) = directory {
        let mut f = tempfile::Builder::new()
            .prefix("mcp-compressor-response-")
            .suffix(".json")
            .tempfile_in(dir)?;
        serde_json::to_writer(f.as_file_mut(), result)?;
        f.flush()?;
        Some(f)
    } else {
        None
    };
    let path = file
        .as_ref()
        .map(|f| {
            f.path()
                .to_str()
                .ok_or_else(|| Error::Config("recovery path must be UTF-8".into()))
        })
        .transpose()?;
    let message = recovery_message.map(str::to_owned).unwrap_or_else(|| match path {
        Some(path) => format!("This tool response has been automatically shrunk to limit token consumption. An attempt has been made to retain important information. If the fully hydrated response is required, it can be read from {path}"),
        None => "This tool response has been automatically shrunk to limit token consumption. An attempt has been made to retain important information. Only if the fully hydrated response is required, recall this compressor invocation with the same tool_name and tool_input and full=true (outside tool_input). A cached original is used when available; otherwise the backend runs again without shrinking. Do not repeat a write unless safe.".into(),
    });
    let input = shrinkfit::json::parse(output)
        .unwrap_or_else(|_| shrinkfit::Value::String(output.to_owned()));
    let original_object = matches!(&input, shrinkfit::Value::Object(_));
    let collision = serde_json::from_str::<Value>(output)
        .ok()
        .is_some_and(|v| v.get("truncation").is_some());
    let mut options = Options::new(max_chars);
    options.format = Format::Json;
    options.input_format = InputFormat::Json;
    options.indent = 0;
    options.array_strategy = ArrayStrategy::Condense;
    if collision {
        options.protect = vec!["/truncation".to_string()];
    }
    let mut shrunk = shrinkfit::shrink(&input, &options);
    let mut final_result = None;
    for _ in 0..8 {
        // Direct shrinkfit counters, bounded instead of its unbounded field-path list.
        let mut truncation = json!({"message":message,"report":{
            "pruned":shrunk.report.pruned,"fits":shrunk.report.fits,
            "items_omitted":shrunk.report.stats.items_omitted,"keys_omitted":shrunk.report.stats.keys_omitted,
            "strings_truncated":shrunk.report.stats.strings_truncated,
        }});
        if let Some(path) = path {
            truncation["original_response_file"] = json!(path);
        }
        let object = original_object && !collision && shrunk.text.starts_with('{');
        let mut output_result = result.clone();
        if object {
            let body = shrunk
                .text
                .trim_end()
                .strip_suffix('}')
                .ok_or_else(|| Error::Config("invalid shrinkfit object".into()))?;
            let separator = if body.trim() == "{" { "" } else { "," };
            let text = format!(
                "{body}{separator}\"truncation\":{}}}",
                serde_json::to_string(&truncation)?
            );
            output_result.content = vec![ContentBlock::text(&text)];
            if result.structured_content.is_some() {
                output_result.structured_content = Some(serde_json::from_str(&text)?);
            }
        } else {
            // Arrays/scalars cannot accept sibling fields. Preserve their value type,
            // and use MCP metadata plus a text notice visible to protocol clients.
            output_result.content = vec![ContentBlock::text(&shrunk.text)];
            if directory.is_none() {
                output_result
                    .content
                    .push(ContentBlock::text(serde_json::to_string(
                        &json!({"truncation":truncation}),
                    )?));
            }
            if result.structured_content.is_some() {
                output_result.structured_content = Some(serde_json::from_str(&shrunk.text)?);
            }
            let mut encoded = serde_json::to_value(&output_result)?;
            let meta = encoded
                .as_object_mut()
                .unwrap()
                .entry("_meta")
                .or_insert_with(|| json!({}));
            if let Some(meta) = meta.as_object_mut() {
                if meta.contains_key("truncation") {
                    return Err(Error::Config(
                        "response already has _meta.truncation; refusing to overwrite metadata"
                            .into(),
                    ));
                }
                meta.insert("truncation".into(), truncation);
            } else {
                return Err(Error::Config("response _meta must be an object".into()));
            }
            output_result = serde_json::from_value(encoded)?;
        }
        let text = string_output(&output_result)?;
        let excess = text.chars().count().saturating_sub(max_chars);
        if excess == 0 && shrunk.report.fits {
            final_result = Some(output_result);
            break;
        }
        if options.max_chars <= 1 {
            break;
        }
        // Structured content and protocol metadata can duplicate the body. Scale
        // proportionally rather than assuming one output char per preview char.
        let proportional =
            options.max_chars.saturating_mul(max_chars) / text.chars().count().max(1);
        options.max_chars = proportional
            .saturating_sub(16)
            .max(1)
            .min(options.max_chars - 1);
        shrunk = shrinkfit::shrink(&input, &options);
    }
    let result = final_result.ok_or_else(|| {
        Error::Config(
            "shrinkfit could not fit response and truncation instructions in budget".into(),
        )
    })?;
    if let Some(file) = file.take() {
        file.keep().map_err(|e| Error::Io(e.error))?;
    }
    Ok(Some(result))
}

pub(super) fn string_output(result: &CallToolResult) -> Result<String, Error> {
    let encoded = serde_json::to_value(result)?;
    if encoded
        .get("_meta")
        .and_then(|m| m.get("truncation"))
        .is_some()
    {
        return Ok(serde_json::to_string(result)?);
    }
    super::compressed::call_tool_result_to_string(result.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn object_is_not_wrapped_and_full_result_is_saved() {
        let dir = tempfile::tempdir().unwrap();
        let text = json!({"data":{"items":(0..50).map(|i|json!({"id":i,"text":"detail ".repeat(100)})).collect::<Vec<_>>()},"nextPageToken":"opaque"}).to_string();
        let mut original = CallToolResult::success(vec![ContentBlock::text(&text)]);
        original.structured_content = Some(serde_json::from_str(&text).unwrap());
        let result = shrink_result(&original, &text, 4000, Some(dir.path()))
            .unwrap()
            .unwrap();
        let output = string_output(&result).unwrap();
        let value: Value = serde_json::from_str(&output).unwrap();
        assert!(value.get("data").is_some());
        assert!(value.get("preview").is_none());
        assert!(output.chars().count() <= 4000);
        let path = value["truncation"]["original_response_file"]
            .as_str()
            .unwrap();
        let saved: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(saved, serde_json::to_value(&original).unwrap());
        assert_eq!(result.structured_content, Some(value.clone()));
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
    fn non_filesystem_message_requests_full_without_local_path() {
        let text = json!({"items":vec!["x".repeat(2000);20]}).to_string();
        let original = CallToolResult::success(vec![ContentBlock::text(&text)]);
        let result = shrink_result(&original, &text, 2000, None)
            .unwrap()
            .unwrap();
        let value: Value = serde_json::from_str(&string_output(&result).unwrap()).unwrap();
        assert!(value["truncation"]["message"]
            .as_str()
            .unwrap()
            .contains("full=true"));
        assert!(value["truncation"].get("original_response_file").is_none());
    }
    #[test]
    fn arrays_text_and_truncation_collisions_keep_type_and_metadata() {
        for text in [
            json!(vec!["x".repeat(100); 100]).to_string(),
            "Description: ".repeat(1000),
            json!({"truncation":"backend-owned","data":"long ".repeat(1000)}).to_string(),
        ] {
            let original = CallToolResult::success(vec![ContentBlock::text(&text)]);
            let result = shrink_result(&original, &text, 3000, None)
                .unwrap()
                .unwrap();
            let encoded = serde_json::to_value(&result).unwrap();
            assert!(encoded["_meta"]["truncation"]["message"].is_string());
            let body: Value =
                serde_json::from_str(encoded["content"][0]["text"].as_str().unwrap()).unwrap();
            if text.starts_with('[') {
                assert!(body.is_array());
            }
            if text.starts_with("Description") {
                assert!(body.is_string());
            }
            assert!(string_output(&result).unwrap().chars().count() <= 3000);
        }
    }
    #[test]
    fn small_unicode_and_errors_do_not_create_files() {
        let dir = tempfile::tempdir().unwrap();
        let text = "😀".repeat(1000);
        let result = CallToolResult::success(vec![ContentBlock::text(&text)]);
        assert!(shrink_result(&result, &text, 1000, Some(dir.path()))
            .unwrap()
            .is_none());
        let error = CallToolResult::error(vec![ContentBlock::text(&text)]);
        assert!(shrink_result(&error, &text, 100, Some(dir.path()))
            .unwrap()
            .is_none());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
    #[test]
    fn media_and_storage_failures_are_safe() {
        let dir = tempfile::tempdir().unwrap();
        let text = "long ".repeat(1000);
        let result = CallToolResult::success(vec![ContentBlock::text(&text)]);
        assert!(shrink_result(&result, &text, 1, Some(dir.path())).is_err());
        assert!(shrink_result(&result, &text, 2000, Some(&dir.path().join("missing"))).is_err());
        let media: CallToolResult = serde_json::from_value(
            json!({"content":[{"type":"image","data":"aW1hZ2U=","mimeType":"image/png"}]}),
        )
        .unwrap();
        assert!(shrink_result(&media, &text, 2000, None).unwrap().is_none());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
