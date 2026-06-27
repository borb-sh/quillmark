//! Spike: latency of Option C (flatten → hayro) vs. the original canvas
//! strategy (stamp → hayro, background-only raster).
//!
//! Both strategies produce the same deliverable PDF via `stamp()`. The
//! question is the rasterisation-input step:
//!
//! - Original strategy: rasterise the `stamp()` output with hayro (fields
//!   blank under Technique A; values come from the `regions` sidecar).
//! - Option C: call `flatten()` on the same background → hayro rasterises
//!   values-included content. Same `stamp()` deliverable, different preview
//!   source.
//!
//! This bench measures `stamp()`, `flatten()`, and their combination to
//! quantify the extra latency Option C adds to the render pipeline.
//! Byte-size outputs (proxies for hayro rasterisation input complexity) are
//! printed before the timed loops.
//!
//! hayro rasterisation itself is excluded — it is not yet wired (#754). Add
//! a `rasterise` bench here when hayro lands.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use quillmark_pdf::{
    flatten, page_media_boxes, stamp, FieldSpec, FieldType, StampOptions, CHECKBOX_ON_STATE,
};

// The stripped background shipped with the gov_form fixture.
const FORM_PDF: &[u8] =
    include_bytes!("../../fixtures/resources/quills/gov_form/0.1.0/form.pdf");

/// Build FieldSpecs from form.json geometry, converting top-left {x,y,w,h}
/// to bottom-left [x0,y0,x1,y1] using the page's actual height.
fn sample_fields(page_height: f32) -> Vec<FieldSpec> {
    // flip(y_top, field_h) → y_bottom in PDF user space
    let flip = |y: f32, fh: f32| page_height - y - fh;

    vec![
        FieldSpec {
            name: "FullName".into(),
            page: 0,
            rect: {
                let y0 = flip(100.0, 20.0);
                [180.0, y0, 520.0, y0 + 20.0]
            },
            field_type: FieldType::Text { multiline: false },
            value: Some("Jane Doe".into()),
            tooltip: Some("Full legal name".into()),
        },
        FieldSpec {
            name: "Comments".into(),
            page: 0,
            rect: {
                let y0 = flip(140.0, 80.0);
                [180.0, y0, 520.0, y0 + 80.0]
            },
            field_type: FieldType::Text { multiline: true },
            value: Some("No additional comments at this time.".into()),
            tooltip: None,
        },
        FieldSpec {
            name: "Agree".into(),
            page: 0,
            rect: {
                let y0 = flip(240.0, 14.0);
                [180.0, y0, 194.0, y0 + 14.0]
            },
            field_type: FieldType::Checkbox,
            value: Some(CHECKBOX_ON_STATE.into()),
            tooltip: None,
        },
        FieldSpec {
            name: "FavoriteColor".into(),
            page: 0,
            rect: {
                let y0 = flip(280.0, 20.0);
                [180.0, y0, 520.0, y0 + 20.0]
            },
            field_type: FieldType::Choice {
                options: vec!["red".into(), "green".into(), "blue".into()],
            },
            value: Some("blue".into()),
            tooltip: None,
        },
        FieldSpec {
            name: "Signature".into(),
            page: 0,
            rect: {
                let y0 = flip(330.0, 40.0);
                [180.0, y0, 520.0, y0 + 40.0]
            },
            field_type: FieldType::Signature,
            value: None,
            tooltip: None,
        },
    ]
}

fn bench_preview_path(c: &mut Criterion) {
    let boxes = page_media_boxes(FORM_PDF).expect("gov_form media boxes");
    let [_x0, y0, _x1, y1] = boxes[0];
    let page_height = y1 - y0;
    let fields = sample_fields(page_height);

    // ── Byte sizes: rasterisation input complexity proxy ─────────────────
    let stamped = stamp(FORM_PDF.to_vec(), &fields, &StampOptions::default())
        .expect("stamp");
    let flattened = flatten(FORM_PDF.to_vec(), &fields).expect("flatten");
    let bg = FORM_PDF.len();
    let st = stamped.pdf.len();
    let fl = flattened.len();
    println!(
        "\n  pdf sizes (gov_form, 5 fields, 1 page)\
         \n    background (raster input, original strategy): {bg:>7} B\
         \n    stamp output (raster input, original strategy): {st:>7} B  (+{} B vs bg)\
         \n    flatten output (raster input, Option C):        {fl:>7} B  (+{} B vs bg)\n",
        st.saturating_sub(bg),
        fl.saturating_sub(bg),
    );

    let mut group = c.benchmark_group("preview_path");

    // stamp() — produces the deliverable PDF (Technique A, NeedAppearances).
    // Also the rasterisation input for the original canvas strategy (fields
    // render blank; values overlay from `regions` in JS).
    group.bench_function("stamp", |b| {
        b.iter(|| {
            stamp(
                black_box(FORM_PDF.to_vec()),
                black_box(&fields),
                &StampOptions::default(),
            )
            .unwrap()
        })
    });

    // flatten() — Option C rasterisation input. Extra step beyond stamp().
    // hayro renders this with values baked into content streams.
    group.bench_function("flatten", |b| {
        b.iter(|| flatten(black_box(FORM_PDF.to_vec()), black_box(&fields)).unwrap())
    });

    // Combined: both stamp (deliverable) and flatten (preview/PNG) from the
    // same background. In practice they run independently and can be
    // parallelised; this measures the serial ceiling.
    group.bench_function("stamp_plus_flatten", |b| {
        b.iter(|| {
            let s =
                stamp(black_box(FORM_PDF.to_vec()), black_box(&fields), &StampOptions::default())
                    .unwrap();
            let f = flatten(black_box(FORM_PDF.to_vec()), black_box(&fields)).unwrap();
            (s, f)
        })
    });

    group.finish();
}

criterion_group!(benches, bench_preview_path);
criterion_main!(benches);
