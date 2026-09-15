use crate::types::Diagnostic as WasmDiagnostic;
use quillmark_core::error::{Diagnostic, ParseError, RenderError, Severity};
use serde::Serialize;
use wasm_bindgen::prelude::*;

/// Every error crossing to JS as one shape: a non-empty list of diagnostics,
/// surfaced as the thrown `Error`'s `.diagnostics` property.
#[derive(Debug, Clone)]
pub struct WasmError {
    pub diagnostics: Vec<Diagnostic>,
}

impl WasmError {
    /// The single diagnostic's message, or a `"… N error(s)"` aggregate.
    pub fn message(&self) -> String {
        match self.diagnostics.as_slice() {
            [] => "Unknown error".to_string(),
            diags => RenderError::summary_message(diags),
        }
    }

    /// A real JS `Error` whose `.diagnostics` array uses the same diagnostic
    /// shape as `RenderResult.warnings`.
    pub fn to_js_value(&self) -> JsValue {
        let err = js_sys::Error::new(&self.message());
        let serializer = serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true);
        let wasm_diags: Vec<WasmDiagnostic> =
            self.diagnostics.iter().cloned().map(Into::into).collect();
        if let Ok(data) = wasm_diags.serialize(&serializer) {
            let _ = js_sys::Reflect::set(&err, &JsValue::from_str("diagnostics"), &data);
        }
        err.into()
    }
}

impl From<ParseError> for WasmError {
    fn from(error: ParseError) -> Self {
        WasmError {
            diagnostics: vec![error.to_diagnostic()],
        }
    }
}

impl From<RenderError> for WasmError {
    fn from(error: RenderError) -> Self {
        WasmError {
            diagnostics: error.into_diagnostics(),
        }
    }
}

impl From<Vec<Diagnostic>> for WasmError {
    fn from(diagnostics: Vec<Diagnostic>) -> Self {
        WasmError { diagnostics }
    }
}

impl From<String> for WasmError {
    fn from(message: String) -> Self {
        WasmError {
            diagnostics: vec![Diagnostic::new(Severity::Error, message)],
        }
    }
}

impl From<&str> for WasmError {
    fn from(message: &str) -> Self {
        WasmError::from(message.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compilation_failed_carries_all_diagnostics() {
        let diag1 = Diagnostic::new(Severity::Error, "Error 1".to_string());
        let diag2 = Diagnostic::new(Severity::Error, "Error 2".to_string());
        let render_err = RenderError::new(vec![diag1, diag2]);
        let wasm_err: WasmError = render_err.into();

        assert_eq!(wasm_err.diagnostics.len(), 2);
        assert_eq!(wasm_err.diagnostics[0].message, "Error 1");
        assert_eq!(wasm_err.diagnostics[1].message, "Error 2");
        let summary = wasm_err.message();
        assert!(summary.contains("2"));
        assert!(summary.contains("Error 1"));
    }

}
