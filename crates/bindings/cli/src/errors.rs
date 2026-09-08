use quillmark_core::RenderError;

/// [`print_cli_error`] renders full diagnostics for the render and parse
/// variants, a plain line for `Io` and `InvalidArgument`, and nothing for
/// `Reported`, whose diagnostics the command has already written.
#[derive(Debug)]
pub enum CliError {
    Io(std::io::Error),
    Render(RenderError),
    Parse(quillmark_core::ParseError),
    InvalidArgument(String),
    Reported,
}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        CliError::Io(err)
    }
}

impl From<RenderError> for CliError {
    fn from(err: RenderError) -> Self {
        CliError::Render(err)
    }
}

impl From<quillmark_core::ParseError> for CliError {
    fn from(err: quillmark_core::ParseError) -> Self {
        CliError::Parse(err)
    }
}

impl From<quillmark_core::BoundParseError> for CliError {
    fn from(err: quillmark_core::BoundParseError) -> Self {
        use quillmark_core::BoundParseError as E;
        match err {
            E::Parse(e) => CliError::Parse(e),
            E::Mismatch(e) => CliError::Render(e),
        }
    }
}

pub type Result<T> = std::result::Result<T, CliError>;

pub fn print_cli_error(err: &CliError) {
    match err {
        CliError::Render(render_err) => {
            for diag in render_err.diagnostics() {
                eprintln!("{}", diag.fmt_pretty());
            }
        }
        CliError::Parse(parse_err) => {
            eprintln!("{}", parse_err.to_diagnostic().fmt_pretty());
        }
        CliError::Io(io_err) => {
            eprintln!("[ERROR] I/O error: {}", io_err);
        }
        CliError::InvalidArgument(msg) => {
            eprintln!("[ERROR] Invalid argument: {}", msg);
        }
        CliError::Reported => {}
    }
}

pub fn print_warnings(warnings: &[quillmark_core::Diagnostic]) {
    if warnings.is_empty() {
        return;
    }

    eprintln!("\nWarnings:");
    for warning in warnings {
        eprintln!("{}", warning.fmt_pretty());
    }
}
