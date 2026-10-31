//! `quill.license` and `quill.homepage`: package metadata a quill declares
//! about itself. Both are optional, and a value present is held to its shape.

use std::net::{Ipv4Addr, Ipv6Addr};
use std::ops::Range;

use spdx::lexer::{Lexer, LexerToken, Token};
use spdx::{Expression, ParseMode};

use crate::error::{Diagnostic, Severity};

/// The crate's strict mode, except that a deprecated id loads: the license
/// list rides a caret dependency, so a deprecation arriving in a patch release
/// would otherwise refuse on one build a quill another build loads.
const MODE: ParseMode = ParseMode {
    allow_deprecated: true,
    ..ParseMode::STRICT
};

const LICENSE_HINT: &str = "Write an SPDX license expression from https://spdx.org/licenses/, \
                            such as 'MIT' or 'Apache-2.0 OR MIT'; a license off that list is \
                            'LicenseRef-<name>'.";

const EXCEPTION_HINT: &str = "Write an exception id from \
                              https://spdx.org/licenses/exceptions-index.html after WITH, such \
                              as 'LLVM-exception'.";

/// `quill.license`: `None` when absent, else one line holding an SPDX license
/// expression, stored trimmed. A deprecated id loads with
/// `quill::deprecated_license`.
pub(super) fn parse_license(
    quill_section: &serde_json::Value,
    errors: &mut Vec<Diagnostic>,
    warnings: &mut Vec<Diagnostic>,
) -> Option<String> {
    let value = quill_section.get("license")?;
    let refuse = |reason: &str, hint: String| {
        invalid_quill_value("license", value, reason, "quill::invalid_license", hint)
    };
    let Some(text) = value.as_str() else {
        errors.push(refuse("not a string", LICENSE_HINT.to_string()));
        return None;
    };
    if text.contains([
        '\n', '\r', '\u{0B}', '\u{0C}', '\u{85}', '\u{2028}', '\u{2029}',
    ]) {
        errors.push(refuse(
            "a line break",
            "Write the expression on one line.".to_string(),
        ));
        return None;
    }
    let raw = text.trim();
    match check_license(raw) {
        Ok(deprecated) => {
            if !deprecated.is_empty() {
                warnings.push(deprecated_license(raw, &deprecated));
            }
            Some(raw.to_string())
        }
        Err((reason, span)) => {
            let term = raw.get(span.clone()).unwrap_or_default();
            let reason = if term.is_empty() || term == raw {
                reason.to_string()
            } else {
                format!("{reason} at '{term}'")
            };
            errors.push(refuse(&reason, license_hint(raw, span)));
            None
        }
    }
}

/// `quill.homepage`: `None` when absent, else an absolute `http` or `https`
/// URL, stored as written.
pub(super) fn parse_homepage(
    quill_section: &serde_json::Value,
    errors: &mut Vec<Diagnostic>,
) -> Option<String> {
    let value = quill_section.get("homepage")?;
    let reason = match value.as_str() {
        Some(raw) if is_web_url(raw) => return Some(raw.to_string()),
        Some(_) => "not an absolute http or https URL with a host and no user name",
        None => "not a string",
    };
    errors.push(invalid_quill_value(
        "homepage",
        value,
        reason,
        "quill::invalid_homepage",
        "Write an absolute http or https URL, such as 'https://github.com/org/quills'.".to_string(),
    ));
    None
}

/// The deprecated ids `raw` names, or why it is refused and the span refused.
///
/// The crate's strict mode admits three things the SPDX grammar does not:
/// a lowercase operator, an empty `LicenseRef-`, and `NOASSERTION`, which is
/// on its license list.
fn check_license(raw: &str) -> Result<Vec<&'static str>, (String, Range<usize>)> {
    Expression::parse_mode(raw, MODE).map_err(|e| (e.reason.to_string(), e.span))?;
    let mut deprecated = Vec::new();
    for LexerToken { token, span } in Lexer::new_mode(raw, MODE).flatten() {
        let lowercase = raw
            .get(span.clone())
            .is_some_and(|text| text.bytes().any(|b| b.is_ascii_lowercase()));
        let refused = match token {
            Token::And | Token::Or | Token::With if lowercase => Some("an operator in lowercase"),
            Token::LicenseRef {
                doc_ref,
                lic_ref: name,
            }
            | Token::AdditionRef {
                doc_ref,
                add_ref: name,
            } if name.is_empty() || doc_ref == Some("") => Some("an empty reference"),
            Token::Spdx(id) if matches!(id.name, "NOASSERTION" | "NONE") => Some("not a license"),
            Token::Spdx(id) if id.is_deprecated() => {
                deprecated.push(id.name);
                None
            }
            Token::Exception(id) if id.is_deprecated() => {
                deprecated.push(id.name);
                None
            }
            _ => None,
        };
        if let Some(reason) = refused {
            return Err((reason.to_string(), span));
        }
    }
    Ok(deprecated)
}

/// `Expression::canonicalize`'s rewrite of `raw` when it loads with no
/// refusal or deprecated id and writes the same version digits: the crate's
/// imprecise-name table maps some names to a version the author never wrote,
/// `gnu gpl v2` to `GPL-3.0-only` among them.
fn canonical(raw: &str) -> Option<String> {
    let fixed = Expression::canonicalize(raw).ok()??;
    let versions = |s: &str| -> String { s.chars().filter(|c| matches!(c, '1'..='9')).collect() };
    (versions(&fixed) == versions(raw)
        && check_license(&fixed).is_ok_and(|deprecated| deprecated.is_empty()))
    .then_some(fixed)
}

/// The fix for a refused license: its canonical rewrite, else the list's
/// spelling of the refused term, else where the ids come from.
fn license_hint(raw: &str, span: Range<usize>) -> String {
    if let Some(fixed) = canonical(raw) {
        return format!("Did you mean '{fixed}'?");
    }
    let term = raw.get(span.clone()).unwrap_or_default();
    if let Some(id) = listed_spelling(term) {
        return format!("Did you mean '{id}'? Write the id as the SPDX list spells it.");
    }
    let after_with = raw.get(..span.start).is_some_and(|before| {
        before
            .trim_end()
            .rsplit(|c: char| c.is_whitespace() || c == '(')
            .next()
            .is_some_and(|word| word.eq_ignore_ascii_case("WITH"))
    });
    if after_with {
        EXCEPTION_HINT
    } else {
        LICENSE_HINT
    }
    .to_string()
}

/// The SPDX list's spelling of `term` when it differs from `term` only in
/// case, and the license or exception it names is not deprecated.
fn listed_spelling(term: &str) -> Option<&'static str> {
    let licenses = spdx::identifiers::LICENSES
        .iter()
        .map(|l| (l.name, l.flags));
    let exceptions = spdx::identifiers::EXCEPTIONS
        .iter()
        .map(|e| (e.name, e.flags));
    licenses
        .chain(exceptions)
        .find(|(name, _)| *name != term && name.eq_ignore_ascii_case(term))
        .filter(|(_, flags)| flags & spdx::flags::IS_DEPRECATED == 0)
        .map(|(name, _)| name)
}

fn deprecated_license(raw: &str, ids: &[&str]) -> Diagnostic {
    let named = ids
        .iter()
        .map(|id| format!("'{id}'"))
        .collect::<Vec<_>>()
        .join(", ");
    let (noun, replace) = match ids.len() {
        1 => ("id", "Replace it with a current id"),
        _ => ("ids", "Replace them with current ids"),
    };
    let hint = match canonical(raw) {
        Some(fixed) => format!("Write '{fixed}'."),
        None => format!("{replace} from https://spdx.org/licenses/."),
    };
    Diagnostic::new(
        Severity::Warning,
        format!(
            "'license' value '{raw}' in 'quill' section uses the deprecated SPDX {noun} {named}"
        ),
    )
    .with_code("quill::deprecated_license".to_string())
    .with_hint(hint)
}

/// A refused `quill:` value: a string shown quoted, anything else as JSON.
fn invalid_quill_value(
    key: &str,
    value: &serde_json::Value,
    reason: &str,
    code: &str,
    hint: String,
) -> Diagnostic {
    let shown = match value.as_str() {
        Some(raw) => format!("'{raw}'"),
        None => value.to_string(),
    };
    Diagnostic::new(
        Severity::Error,
        format!("Invalid '{key}' value {shown} in 'quill' section: {reason}"),
    )
    .with_code(code.to_string())
    .with_hint(hint)
}

/// An `http` or `https` URL naming a host and no user, its port, IPv6
/// brackets and host code points as a WHATWG URL parser accepts them; a
/// Unicode host is not run through IDNA. A user is refused because a
/// credential in a published URL leaks, and
/// `https://trusted.example@evil.example/` names `evil.example`.
fn is_web_url(raw: &str) -> bool {
    let Some((scheme, rest)) = raw.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        && !raw
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || is_format(c))
        && is_host_and_port(authority)
}

/// A host, bracketed when IPv6, then an optional `:` and a port of 0-65535.
fn is_host_and_port(authority: &str) -> bool {
    let (host, port) = match authority.strip_prefix('[') {
        Some(rest) => match rest.split_once(']') {
            Some((ip, port)) => (ip.parse::<Ipv6Addr>().is_ok(), port),
            None => return false,
        },
        None => {
            let (host, port) = authority.split_at(authority.find(':').unwrap_or(authority.len()));
            (is_domain(host), port)
        }
    };
    host && (port.is_empty()
        || port.strip_prefix(':').is_some_and(|digits| {
            !digits.is_empty()
                && digits.bytes().all(|b| b.is_ascii_digit())
                && digits.parse::<u16>().is_ok()
        }))
}

/// A domain or dotted IPv4 host: not empty or only dots, free of the code
/// points a WHATWG parser forbids in one, and a dotted-decimal IPv4 address
/// when its last label is a number, where that parser reads the host as IPv4.
fn is_domain(host: &str) -> bool {
    const FORBIDDEN: &[char] = &['<', '>', '^', '|', '"', '%', '\\', '[', ']', '@', ':'];
    let labels = host.strip_suffix('.').unwrap_or(host);
    let last = labels.rsplit('.').next().unwrap_or_default();
    let numeric = !last.is_empty()
        && (last.bytes().all(|b| b.is_ascii_digit())
            || last.starts_with("0x")
            || last.starts_with("0X"));
    !host.trim_matches('.').is_empty()
        && !host.contains(FORBIDDEN)
        && (!numeric || labels.parse::<Ipv4Addr>().is_ok())
}

/// Unicode's format characters (general category Cf, Unicode 15.1): invisible,
/// so a URL holding one reads as another.
fn is_format(c: char) -> bool {
    matches!(c,
        '\u{AD}' | '\u{600}'..='\u{605}' | '\u{61C}' | '\u{6DD}' | '\u{70F}'
        | '\u{890}'..='\u{891}' | '\u{8E2}' | '\u{180E}' | '\u{200B}'..='\u{200F}'
        | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{206F}'
        | '\u{FEFF}' | '\u{FFF9}'..='\u{FFFB}' | '\u{110BD}' | '\u{110CD}'
        | '\u{13430}'..='\u{1343F}' | '\u{1BCA0}'..='\u{1BCA3}' | '\u{1D173}'..='\u{1D17A}'
        | '\u{E0001}' | '\u{E0020}'..='\u{E007F}')
}
