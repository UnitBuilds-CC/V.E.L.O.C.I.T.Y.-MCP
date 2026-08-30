//! Native Rust NDA payload executor.
//!
//! Ported from C# `Velocity.Core.NdaExecutor`. Handles two payload types:
//!
//! - **BinaryPayload**: Decodes base64 assembly bytes from the NDA string pool,
//!   writes to a temp file, and executes via the .NET runtime (`dotnet exec`).
//!   Note: The C# version loads assemblies in-memory via `Assembly.Load()`.
//!   The Rust port uses the `dotnet` CLI as a subprocess since Rust cannot
//!   load .NET assemblies directly.
//!
//! - **SourceCode**: Extracts code text from display commands, writes to a temp
//!   file, and executes via the appropriate interpreter (python, node, etc.).

use crate::nda_document::NdaDocument;
use crate::sandbox::{self, Sandbox};

/// Execute an NDA document's payload.
///
/// Reads the semantic triples to determine the payload type, then executes
/// accordingly. Returns the captured stdout output.
pub fn execute_nda(nda_bytes: &[u8], args: &[String]) -> Result<String, String> {
    let doc = NdaDocument::read(nda_bytes)?;

    // First pass: find the payload type and asset ID
    let mut payload_type: Option<String> = None;
    let mut asset_id: Option<String> = None;

    for triple in &doc.triples {
        let s = doc.get_string(triple.subject_offset)?;
        let p = doc.get_string(triple.predicate_offset)?;
        let o = doc.get_string(triple.object_offset)?;

        if p == "TYPE" && (o == "BinaryPayload" || o == "SourceCode") {
            payload_type = Some(o);
            asset_id = Some(s);
            break;
        }
    }

    let asset_id = asset_id.ok_or_else(|| {
        "NDA file does not contain a runnable payload type (BinaryPayload or SourceCode).".to_string()
    })?;
    let payload_type = payload_type.ok_or_else(|| {
        "NDA file does not contain a runnable payload type (BinaryPayload or SourceCode).".to_string()
    })?;

    // Second pass: extract payload metadata
    let mut filename: Option<String> = None;
    let mut base64_data: Option<String> = None;
    let mut language: Option<String> = None;

    for triple in &doc.triples {
        let s = doc.get_string(triple.subject_offset)?;
        if s != asset_id { continue; }
        let p = doc.get_string(triple.predicate_offset)?;
        let o = doc.get_string(triple.object_offset)?;

        match p.as_str() {
            "FILENAME" => filename = Some(o),
            "BASE64_DATA" => base64_data = Some(o),
            "LANGUAGE" => language = Some(o),
            _ => {}
        }
    }

    match payload_type.as_str() {
        "BinaryPayload" => execute_binary_payload(&base64_data, args),
        "SourceCode" => execute_source_code(&doc, &filename, &language, args),
        _ => Err(format!("Unsupported execution payload type: {}", payload_type)),
    }
}

/// Execute a BinaryPayload by decoding base64 and running via .NET runtime.
fn execute_binary_payload(base64_data: &Option<String>, args: &[String]) -> Result<String, String> {
    let b64 = base64_data.as_ref()
        .ok_or("BinaryPayload is missing BASE64_DATA.")?;

    use base64::{Engine as _, engine::general_purpose};
    let assembly_bytes = general_purpose::STANDARD.decode(b64)
        .map_err(|e| format!("Failed to decode BASE64_DATA: {}", e))?;

    // Execute inside sandbox with isolated temp directory
    let mut sandbox = Sandbox::new()?;
    let dll_name = format!("nda_run_{}.dll", random_suffix());
    sandbox.write_file(&dll_name, &assembly_bytes)?;

    let dll_path = sandbox.work_dir().join(&dll_name);
    let mut cmd_args = vec!["exec".to_string(), dll_path.to_string_lossy().to_string()];
    cmd_args.extend_from_slice(args);

    let result = sandbox.execute("dotnet", &cmd_args);

    if result.timed_out {
        return Err(format!("Execution timed out after {} seconds", 30));
    }

    if !result.stderr.is_empty() {
        Ok(format!("{}\nError Output:\n{}", result.stdout, sandbox::sanitize_error(&result.stderr)))
    } else {
        Ok(result.stdout)
    }
}

/// Execute SourceCode by extracting code from display commands and running via interpreter.
fn execute_source_code(
    doc: &NdaDocument,
    filename: &Option<String>,
    _language: &Option<String>,
    args: &[String],
) -> Result<String, String> {
    // Extract code from DrawText commands at X=85 (matching the code converter layout)
    let mut code_lines = Vec::new();
    for cmd in &doc.commands {
        if cmd.command_type == 1 && cmd.x == 85 {
            let text = doc.get_string(cmd.content_offset)?;
            code_lines.push(text);
        }
    }

    let code = code_lines.join("\n");
    if code.trim().is_empty() {
        return Err("SourceCode payload contains no code lines in the command buffer.".to_string());
    }

    // Determine file extension from filename
    let ext = filename.as_deref()
        .and_then(|f| std::path::Path::new(f).extension())
        .and_then(|e| e.to_str())
        .unwrap_or("py");

    // Execute inside sandbox with isolated temp directory
    let mut sandbox = Sandbox::new()?;
    let file_name = format!("nda_run_{}.{}", random_suffix(), ext);
    sandbox.write_file(&file_name, code.as_bytes())?;

    let script_path = sandbox.work_dir().join(&file_name);
    let (program, mut cmd_args) = interpreter_command(ext, &script_path);
    cmd_args.extend_from_slice(args);

    let result = sandbox.execute(program, &cmd_args);

    if result.timed_out {
        return Err(format!("Execution timed out after {} seconds", 30));
    }

    if !result.stderr.is_empty() {
        Ok(format!("{}\nError Output:\n{}", result.stdout, sandbox::sanitize_error(&result.stderr)))
    } else {
        Ok(result.stdout)
    }
}

/// Map a file extension to the interpreter program and initial arguments.
fn interpreter_command(ext: &str, script_path: &std::path::Path) -> (&'static str, Vec<String>) {
    match ext.to_lowercase().as_str() {
        "py" => ("python", vec![script_path.to_string_lossy().to_string()]),
        "js" => ("node", vec![script_path.to_string_lossy().to_string()]),
        "ps1" => ("powershell", vec![
            "-NoProfile".to_string(),
            "-ExecutionPolicy".to_string(),
            "Bypass".to_string(),
            "-File".to_string(),
            script_path.to_string_lossy().to_string(),
        ]),
        "sh" => ("bash", vec![script_path.to_string_lossy().to_string()]),
        "cmd" | "bat" => ("cmd.exe", vec![
            "/c".to_string(),
            script_path.to_string_lossy().to_string(),
        ]),
        other => ("echo", vec![format!("Script execution for extension '{}' is not supported.", other)]),
    }
}

/// Generate a short random suffix for temp file names.
fn random_suffix() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    format!("{:08x}", nanos.wrapping_mul(2654435761))
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nda_document::NdaCompiler;

    #[test]
    fn test_execute_non_runnable_nda_returns_error() {
        // Create an NDA with no runnable payload type
        let mut compiler = NdaCompiler::new();
        compiler.add_triple("DOC_1", "TYPE", "PdfDocument");
        compiler.add_command(1, 0xFFFFFFFF, 0, 0, 100, 20, "test");
        let data = compiler.compile();

        let result = execute_nda(&data, &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("runnable payload"));
    }

    #[test]
    fn test_execute_source_code_missing_code() {
        // SourceCode type but no DrawText commands at X=85
        let mut compiler = NdaCompiler::new();
        let code_id = "CODE_TEST";
        compiler.add_triple(code_id, "TYPE", "SourceCode");
        compiler.add_triple(code_id, "FILENAME", "test.py");
        compiler.add_command(1, 0xFFFFFFFF, 0, 0, 100, 20, "not code"); // X != 85
        let data = compiler.compile();

        let result = execute_nda(&data, &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("no code lines"));
    }

    #[test]
    fn test_execute_binary_missing_base64() {
        let mut compiler = NdaCompiler::new();
        let bin_id = "BIN_TEST";
        compiler.add_triple(bin_id, "TYPE", "BinaryPayload");
        compiler.add_triple(bin_id, "FILENAME", "test.dll");
        // No BASE64_DATA triple
        let data = compiler.compile();

        let result = execute_nda(&data, &[]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("BASE64_DATA"));
    }
}
