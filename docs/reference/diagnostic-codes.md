# Diagnostic Codes

Every `code` a `Diagnostic` can carry, by namespace. Route on these, not on `message` ([Error Handling](../integration/error-handling.md)). The list is complete. A code leaves this page only in a release whose migration guide names it.

## `parse::`

Reading a document: card-yaml, limits, body import.

- `parse::anchor_id_collision`
- `parse::body_import`
- `parse::card_fence_missing_blank`
- `parse::card_id_reminted`
- `parse::dropped_construct`
- `parse::empty_input`
- `parse::input_too_large`
- `parse::invalid_quill_reference`
- `parse::invalid_structure`
- `parse::missing_kind`
- `parse::missing_quill`
- `parse::payload_not_mapping`
- `parse::too_many_cards`
- `parse::too_many_fields`
- `parse::unclosed_code_block`
- `parse::unsupported_yaml_tag`
- `parse::yaml_error_with_location`

## `export::`

Writing a content to markdown.

- `export::lost_construct`

## `validation::`

A document checked against its quill's schema.

- `validation::body_disabled`
- `validation::cardinality`
- `validation::coercion_failed`
- `validation::enum_violation`
- `validation::format_violation`
- `validation::held_stored`
- `validation::line_break_folded`
- `validation::not_single_line`
- `validation::not_plain`
- `validation::out_of_variant`
- `validation::over_length`
- `validation::seed_overlay_shape`
- `validation::seed_unknown_field`
- `validation::seed_unknown_kind`
- `validation::type_mismatch`
- `validation::unknown_card`
- `validation::unknown_field`

## `quill::`

Loading a quill: `Quill.yaml`, its files, its seeds, the `.quill` bundle that carries them.

- `quill::array_missing_items`
- `quill::array_properties_not_supported`
- `quill::bodiless_card_kind`
- `quill::bundle_invalid`
- `quill::bundle_path`
- `quill::bundle_too_large`
- `quill::bundle_unsupported`
- `quill::default_format_violation`
- `quill::default_held_stored`
- `quill::default_not_in_enum`
- `quill::default_on_namespace`
- `quill::default_over_length`
- `quill::default_over_max`
- `quill::default_type_mismatch`
- `quill::deprecated_flat`
- `quill::deprecated_groups`
- `quill::deprecated_license`
- `quill::duplicate_group`
- `quill::empty_description`
- `quill::enum_blank_member`
- `quill::example_format_violation`
- `quill::example_held_stored`
- `quill::example_not_in_enum`
- `quill::example_on_namespace`
- `quill::example_over_length`
- `quill::example_over_max`
- `quill::example_type_mismatch`
- `quill::field_after_object`
- `quill::field_parse_error`
- `quill::implicit_group`
- `quill::invalid_backend_section`
- `quill::invalid_body`
- `quill::invalid_card_kinds`
- `quill::invalid_card_name`
- `quill::invalid_card_schema`
- `quill::invalid_field_name`
- `quill::invalid_group_id`
- `quill::invalid_homepage`
- `quill::invalid_license`
- `quill::invalid_matrix_member`
- `quill::invalid_name`
- `quill::invalid_requires`
- `quill::invalid_ui`
- `quill::invalid_utf8`
- `quill::invalid_version`
- `quill::items_not_supported`
- `quill::load_failed`
- `quill::matrix_reserved_column`
- `quill::missing_backend`
- `quill::missing_description`
- `quill::missing_file`
- `quill::missing_name`
- `quill::missing_section`
- `quill::missing_version`
- `quill::name_mismatch`
- `quill::nested_group_not_supported`
- `quill::object_empty_properties`
- `quill::object_missing_properties`
- `quill::open_matrix_unsupported`
- `quill::optional_default`
- `quill::optional_namespace`
- `quill::requires_unmet`
- `quill::reserved_field_name`
- `quill::richtext_default_import`
- `quill::richtext_example_import`
- `quill::seed_cardinality`
- `quill::seed_enum_violation`
- `quill::seed_format_violation`
- `quill::seed_held_stored`
- `quill::seed_not_plain`
- `quill::seed_not_single_line`
- `quill::seed_out_of_variant`
- `quill::seed_over_length`
- `quill::seed_overlay_shape`
- `quill::seed_redundant`
- `quill::seed_type_mismatch`
- `quill::seed_unknown_field`
- `quill::table_column_not_leaf`
- `quill::title_on_items`
- `quill::title_template`
- `quill::too_many_fields`
- `quill::unknown_backend_key`
- `quill::unknown_group`
- `quill::unknown_key`
- `quill::unknown_section`
- `quill::variant_empty`
- `quill::variant_field_collision`
- `quill::variant_placement`
- `quill::variant_reserved_field_name`
- `quill::variant_unknown_value`
- `quill::variants_on_non_enum`
- `quill::version_mismatch`
- `quill::yaml_parse_error`

## `edit::`

A mutator or typed write the document refuses.

- `edit::content_apply`
- `edit::duplicate_card_id`
- `edit::field_coercion_failed`
- `edit::field_decode`
- `edit::field_not_content`
- `edit::field_not_single_line`
- `edit::import`
- `edit::index_out_of_range`
- `edit::invalid_card_id`
- `edit::invalid_field_name`
- `edit::invalid_kind_name`
- `edit::invalid_payload`
- `edit::rebase_conflict`
- `edit::reserved_kind`
- `edit::root_only_entry`
- `edit::stale_base`
- `edit::too_many_cards`
- `edit::unknown_field`
- `edit::value_too_deep`

## `conform::`

A `Quill::conform` warning: a value the typed write refuses, left as authored.

- `conform::field_coercion_failed`
- `conform::field_decode`
- `conform::field_not_single_line`
- `conform::invalid_field_name`
- `conform::value_too_deep`

## `authoring::`

Importing an [authoring document](../integration/llm-authoring.md): a value that is not its shape. It carries no `path`; `args.pointer` is the JSON Pointer to the offending value (`/cards/1/kind`).

- `authoring::invalid_shape`

## `typst::`

Opening a Typst quill and compiling its plate.

- `typst::compile`
- `typst::duplicate_form_field`
- `typst::emit`
- `typst::file_not_found`
- `typst::font_collection_skipped`
- `typst::form_field_page_out_of_range`
- `typst::helper_import`
- `typst::helper_source`
- `typst::invalid_utf8`
- `typst::missing_glyph`
- `typst::overlay_internal`
- `typst::package_entrypoint_missing`
- `typst::package_manifest`
- `typst::path_skipped`
- `typst::pdf_generation`
- `typst::plate_missing`
- `typst::plate_path_invalid`
- `typst::png_encoding`
- `typst::type_error`
- `typst::unclosed_region`
- `typst::unknown_font_family`
- `typst::unknown_variable`
- `typst::unsupported_tex`
- `typst::world_creation`
- `typst::wrong_backend`

## `acroform::`

Opening an `acroform` quill and binding `Form.yaml`.

- `acroform::dangling_binding`
- `acroform::field_page_out_of_range`
- `acroform::invalid_field_rect`
- `acroform::invalid_form_spec`
- `acroform::invalid_option`
- `acroform::legacy_form_json`
- `acroform::legacy_form_yaml`
- `acroform::missing_form_pdf`
- `acroform::missing_form_spec`
- `acroform::stamped_parse_failed`
- `acroform::text_overflow`
- `acroform::unbindable_field`
- `acroform::unencodable_text`

## `pdf::`

The PDF reader and stamp spine both PDF-producing backends share.

- `pdf::bad_rect`
- `pdf::degenerate_page_box`
- `pdf::encrypted`
- `pdf::existing_acroform`
- `pdf::indirect_annots`
- `pdf::nonzero_generation`
- `pdf::parse`
- `pdf::rotated_page`
- `pdf::stamp_parse`
- `pdf::update_parse`
- `pdf::write`
- `pdf::xref_stream`

## `backend::`

The render contract every backend shares.

- `backend::declined_construct`
- `backend::format_not_supported`
- `backend::invalid_date`
- `backend::invalid_raster_scale`
- `backend::page_index_out_of_bounds`
- `backend::page_selection_not_supported`

## `engine::`

Backend dispatch and the host's font library.

- `engine::backend_not_found`
- `engine::font_unreadable`
- `engine::font_unsupported`

## `cli::`

The `quillmark` command line.

- `cli::backend_unresolved`
- `cli::canonical_document_failed`
- `cli::example_file_ignored`
- `cli::example_not_clean`
- `cli::example_render_failed`
- `cli::missing_description`
- `cli::unreadable_document`

## `runtime::`

The `@quillmark/wasm` JavaScript runtime.

- `runtime::backend_load_failed`
- `runtime::backend_trapped`
- `runtime::init_conflict`
- `runtime::init_failed`
- `runtime::not_a_document`
- `runtime::not_a_quill`
