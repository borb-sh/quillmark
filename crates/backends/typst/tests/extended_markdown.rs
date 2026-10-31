//! A footnote renders its note at the page foot, an equation sets in a math
//! face whatever fonts the quill ships, a command outside the TeX vocabulary
//! drawing as its name under `typst::unsupported_tex`.

use quillmark_core::{error::Diagnostic, quill::Quill};

mod common;
use common::glyphs;

const PAGE: &str = r#"
#import "@local/quillmark-helper:0.2.0" as qm
#set page(width: 300pt, height: auto, margin: 10pt)
#qm.data.at("$body", default: [])
"#;

fn quill() -> Quill {
    common::quill_with_plate(&common::yaml("main:\n  fields: {}\n"), PAGE)
}

fn render(quill: &Quill, markdown: &str) -> (String, Vec<Diagnostic>) {
    common::body_page(quill, markdown)
}

#[test]
fn a_footnote_renders_its_note() {
    let (bare, _) = render(&quill(), "Text.");
    let (noted, warnings) = render(&quill(), "Text.[^1]\n\n[^1]: The **note** text.");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(
        glyphs(&noted) >= glyphs(&bare) + "Thenotetext.".len() + 2,
        "the note and its two numbers render"
    );
}

const EQUATIONS: &[&str] = &[
    r"x^2 + y_i^{n+1} - \frac{a}{b}",
    r"\sqrt{2} \sqrt[3]{x} \binom{n}{k}",
    r"\sum_{i=1}^{n} i = \frac{n(n+1)}{2}",
    r"\int_0^\infty e^{-x^2}\,dx = \frac{\sqrt\pi}{2}",
    r"\lim_{x\to 0} \frac{\sin x}{x} = 1",
    r"\left( \frac{a}{b} \right) \left\{ x \middle| x > 0 \right\} \langle v \rangle",
    r"\alpha\beta\Gamma \leq \infty \neq \emptyset \in \mathbb{R} \subseteq \mathcal{F}",
    r"\hat{x} \vec{v} \bar{z} \dot a \overline{AB} \underbrace{x+y}",
    r"\mathbf{A} \boldsymbol{\mu} \mathrm{d}x \operatorname{rank} M \text{ if } x \ne 0",
    r"\begin{pmatrix} a & b \\ c & d \end{pmatrix} \begin{vmatrix} 1 & 0 \\ 0 & 1 \end{vmatrix}",
    r"f(x) = \begin{cases} 1 & x > 0 \\ 0 & \text{otherwise} \end{cases}",
    r"\begin{aligned} a &= b + c \\ &= d \end{aligned}",
    r"f'(x) \quad g''(x) \; a \bmod b \pmod{n} \not\equiv \cdots \ldots",
    r"\displaystyle\sum\limits_{k} k \mapsto \Rightarrow \iff",
    r"{}^{14}C \{ a, b \} \$ \% \# \& \_ 3.14 x^{\prime}",
];

/// Every equation in the vocabulary compiles and draws, inline and display,
/// with no warning; a quill shipping its own text fonts still has a math face.
#[test]
fn equations_set_in_a_math_face_with_no_warning() {
    let hosted = common::host_with_plate(PAGE);
    for tex in EQUATIONS {
        for md in [format!("Inline ${tex}$ math."), format!("$${tex}$$")] {
            for quill in [quill(), hosted.clone()] {
                let (svg, warnings) = render(&quill, &md);
                assert!(warnings.is_empty(), "{md:?}: {warnings:?}");
                assert!(glyphs(&svg) > 0, "{md:?} draws");
            }
        }
    }
}

#[test]
fn an_unknown_command_draws_its_name_and_warns() {
    let (svg, warnings) = render(&quill(), r"$\foo{x} + \bar{y} + \baz$ and $\foo$");
    let codes: Vec<_> = warnings.iter().map(|w| w.code.as_deref()).collect();
    assert_eq!(codes, [Some("typst::unsupported_tex")], "{warnings:?}");
    assert!(
        warnings[0].message.contains(r"`\baz`, `\foo`"),
        "{}",
        warnings[0].message
    );
    assert_eq!(warnings[0].path.as_deref(), Some("main.body"));
    assert!(glyphs(&svg) > "foo+baz".len());
}
