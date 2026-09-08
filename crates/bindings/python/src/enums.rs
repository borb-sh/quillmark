use pyo3::prelude::*;
use quillmark_core::{OutputFormat, Severity};

macro_rules! py_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident / $py_name:literal {
            $($variant:ident),* $(,)?
        }
    ) => {
        #[pyclass(name = $py_name, eq, eq_int, hash, frozen, from_py_object)]
        #[derive(Clone, Copy, PartialEq, Eq, Hash)]
        $(#[$meta])*
        $vis enum $name {
            $($variant),*
        }

        #[pymethods]
        impl $name {
            fn __repr__(&self) -> String {
                format!("<{}.{}>", $py_name, self.name())
            }

            #[getter]
            fn name(&self) -> &'static str {
                match self {
                    $(Self::$variant => stringify!($variant)),*
                }
            }

            #[staticmethod]
            fn all() -> Vec<Self> {
                vec![$(Self::$variant),*]
            }
        }
    };
}

py_enum! {
    pub enum PyOutputFormat / "OutputFormat" {
        PDF,
        SVG,
        PNG,
    }
}

py_enum! {
    pub enum PySeverity / "Severity" {
        ERROR,
        WARNING,
    }
}

impl From<PyOutputFormat> for OutputFormat {
    fn from(val: PyOutputFormat) -> Self {
        match val {
            PyOutputFormat::PDF => OutputFormat::Pdf,
            PyOutputFormat::SVG => OutputFormat::Svg,
            PyOutputFormat::PNG => OutputFormat::Png,
        }
    }
}

impl From<OutputFormat> for PyOutputFormat {
    fn from(val: OutputFormat) -> Self {
        match val {
            OutputFormat::Pdf => PyOutputFormat::PDF,
            OutputFormat::Svg => PyOutputFormat::SVG,
            OutputFormat::Png => PyOutputFormat::PNG,
        }
    }
}

impl From<Severity> for PySeverity {
    fn from(val: Severity) -> Self {
        match val {
            Severity::Warning => PySeverity::WARNING,
            Severity::Error => PySeverity::ERROR,
        }
    }
}
