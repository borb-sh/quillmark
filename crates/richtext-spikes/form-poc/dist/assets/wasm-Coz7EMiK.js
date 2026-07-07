import { _ as $, __tla as __tla_0 } from "./index-BNuI42xC.js";
let l, q, h, D, Y_;
let __tla = Promise.all([
  (() => {
    try {
      return __tla_0;
    } catch {
    }
  })()
]).then(async () => {
  const L = "/assets/wasm_bg-hD2fj0aQ.wasm";
  l = class {
    static __wrap(t) {
      t = t >>> 0;
      const _ = Object.create(l.prototype);
      return _.__wbg_ptr = t, O.register(_, _.__wbg_ptr, _), _;
    }
    __destroy_into_raw() {
      const t = this.__wbg_ptr;
      return this.__wbg_ptr = 0, O.unregister(this), t;
    }
    free() {
      const t = this.__destroy_into_raw();
      e.__wbg_document_free(t, 0);
    }
    static blueprintInstruction(t) {
      let _, o;
      try {
        const d = e.__wbindgen_add_to_stack_pointer(-16), g = w(t, e.__wbindgen_export, e.__wbindgen_export2), f = u;
        e.document_blueprintInstruction(d, g, f);
        var r = a().getInt32(d + 0, true), i = a().getInt32(d + 4, true);
        return _ = r, o = i, p(r, i);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16), e.__wbindgen_export4(_, o, 1);
      }
    }
    get cardCount() {
      return e.document_cardCount(this.__wbg_ptr) >>> 0;
    }
    get cards() {
      try {
        const r = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_cards(r, this.__wbg_ptr);
        var t = a().getInt32(r + 0, true), _ = a().getInt32(r + 4, true), o = a().getInt32(r + 8, true);
        if (o) throw s(_);
        return s(t);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    clone() {
      const t = e.document_clone(this.__wbg_ptr);
      return l.__wrap(t);
    }
    static currentSchemaVersion() {
      let t, _;
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_currentSchemaVersion(i);
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        return t = o, _ = r, p(o, r);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16), e.__wbindgen_export4(t, _, 1);
      }
    }
    equals(t) {
      return y(t, l), e.document_equals(this.__wbg_ptr, t.__wbg_ptr) !== 0;
    }
    static formatDiagnostic(t) {
      let _, o;
      try {
        const d = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_formatDiagnostic(d, b(t));
        var r = a().getInt32(d + 0, true), i = a().getInt32(d + 4, true);
        return _ = r, o = i, p(r, i);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16), e.__wbindgen_export4(_, o, 1);
      }
    }
    static formatRules() {
      let t, _;
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_formatRules(i);
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        return t = o, _ = r, p(o, r);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16), e.__wbindgen_export4(t, _, 1);
      }
    }
    static fromJson(t) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16), d = w(t, e.__wbindgen_export, e.__wbindgen_export2), g = u;
        e.document_fromJson(i, d, g);
        var _ = a().getInt32(i + 0, true), o = a().getInt32(i + 4, true), r = a().getInt32(i + 8, true);
        if (r) throw s(o);
        return l.__wrap(_);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    static fromMarkdown(t) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16), d = w(t, e.__wbindgen_export, e.__wbindgen_export2), g = u;
        e.document_fromMarkdown(i, d, g);
        var _ = a().getInt32(i + 0, true), o = a().getInt32(i + 4, true), r = a().getInt32(i + 8, true);
        if (r) throw s(o);
        return l.__wrap(_);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    insertCard(t, _) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_insertCard(i, this.__wbg_ptr, t, b(_));
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        if (r) throw s(o);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    get main() {
      try {
        const r = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_main(r, this.__wbg_ptr);
        var t = a().getInt32(r + 0, true), _ = a().getInt32(r + 4, true), o = a().getInt32(r + 8, true);
        if (o) throw s(_);
        return s(t);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    static makeCard(t, _, o) {
      try {
        const S = e.__wbindgen_add_to_stack_pointer(-16), V = w(t, e.__wbindgen_export, e.__wbindgen_export2), J = u;
        var r = m(o) ? 0 : w(o, e.__wbindgen_export, e.__wbindgen_export2), i = u;
        e.document_makeCard(S, V, J, m(_) ? 0 : b(_), r, i);
        var d = a().getInt32(S + 0, true), g = a().getInt32(S + 4, true), f = a().getInt32(S + 8, true);
        if (f) throw s(g);
        return s(d);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    moveCard(t, _) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_moveCard(i, this.__wbg_ptr, t, _);
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        if (r) throw s(o);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    constructor(t) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16), d = w(t, e.__wbindgen_export, e.__wbindgen_export2), g = u;
        e.document_new(i, d, g);
        var _ = a().getInt32(i + 0, true), o = a().getInt32(i + 4, true), r = a().getInt32(i + 8, true);
        if (r) throw s(o);
        return this.__wbg_ptr = _ >>> 0, O.register(this, this.__wbg_ptr, this), this;
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    pushCard(t) {
      try {
        const r = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_pushCard(r, this.__wbg_ptr, b(t));
        var _ = a().getInt32(r + 0, true), o = a().getInt32(r + 4, true);
        if (o) throw s(_);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    get quillRef() {
      let t, _;
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_quillRef(i, this.__wbg_ptr);
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        return t = o, _ = r, p(o, r);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16), e.__wbindgen_export4(t, _, 1);
      }
    }
    static quillRefHint() {
      let t, _;
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_quillRefHint(i);
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        return t = o, _ = r, p(o, r);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16), e.__wbindgen_export4(t, _, 1);
      }
    }
    removeCard(t) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_removeCard(i, this.__wbg_ptr, t);
        var _ = a().getInt32(i + 0, true), o = a().getInt32(i + 4, true), r = a().getInt32(i + 8, true);
        if (r) throw s(o);
        return s(_);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    removeCardExt(t) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_removeCardExt(i, this.__wbg_ptr, t);
        var _ = a().getInt32(i + 0, true), o = a().getInt32(i + 4, true), r = a().getInt32(i + 8, true);
        if (r) throw s(o);
        return s(_);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    removeCardExtNamespace(t, _) {
      try {
        const d = e.__wbindgen_add_to_stack_pointer(-16), g = w(_, e.__wbindgen_export, e.__wbindgen_export2), f = u;
        e.document_removeCardExtNamespace(d, this.__wbg_ptr, t, g, f);
        var o = a().getInt32(d + 0, true), r = a().getInt32(d + 4, true), i = a().getInt32(d + 8, true);
        if (i) throw s(r);
        return s(o);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    removeCardField(t, _) {
      try {
        const d = e.__wbindgen_add_to_stack_pointer(-16), g = w(_, e.__wbindgen_export, e.__wbindgen_export2), f = u;
        e.document_removeCardField(d, this.__wbg_ptr, t, g, f);
        var o = a().getInt32(d + 0, true), r = a().getInt32(d + 4, true), i = a().getInt32(d + 8, true);
        if (i) throw s(r);
        return s(o);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    removeExt() {
      try {
        const r = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_removeExt(r, this.__wbg_ptr);
        var t = a().getInt32(r + 0, true), _ = a().getInt32(r + 4, true), o = a().getInt32(r + 8, true);
        if (o) throw s(_);
        return s(t);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    removeExtNamespace(t) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16), d = w(t, e.__wbindgen_export, e.__wbindgen_export2), g = u;
        e.document_removeExtNamespace(i, this.__wbg_ptr, d, g);
        var _ = a().getInt32(i + 0, true), o = a().getInt32(i + 4, true), r = a().getInt32(i + 8, true);
        if (r) throw s(o);
        return s(_);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    removeField(t) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16), d = w(t, e.__wbindgen_export, e.__wbindgen_export2), g = u;
        e.document_removeField(i, this.__wbg_ptr, d, g);
        var _ = a().getInt32(i + 0, true), o = a().getInt32(i + 4, true), r = a().getInt32(i + 8, true);
        if (r) throw s(o);
        return s(_);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    removeSeedNamespace(t) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16), d = w(t, e.__wbindgen_export, e.__wbindgen_export2), g = u;
        e.document_removeSeedNamespace(i, this.__wbg_ptr, d, g);
        var _ = a().getInt32(i + 0, true), o = a().getInt32(i + 4, true), r = a().getInt32(i + 8, true);
        if (r) throw s(o);
        return s(_);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    replaceBody(t) {
      const _ = w(t, e.__wbindgen_export, e.__wbindgen_export2), o = u;
      e.document_replaceBody(this.__wbg_ptr, _, o);
    }
    static schemaVersionOf(t) {
      try {
        const r = e.__wbindgen_add_to_stack_pointer(-16), i = w(t, e.__wbindgen_export, e.__wbindgen_export2), d = u;
        e.document_schemaVersionOf(r, i, d);
        var _ = a().getInt32(r + 0, true), o = a().getInt32(r + 4, true);
        let g;
        return _ !== 0 && (g = p(_, o).slice(), e.__wbindgen_export4(_, o * 1, 1)), g;
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setCardExt(t, _) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_setCardExt(i, this.__wbg_ptr, t, b(_));
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        if (r) throw s(o);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setCardExtNamespace(t, _, o) {
      try {
        const d = e.__wbindgen_add_to_stack_pointer(-16), g = w(_, e.__wbindgen_export, e.__wbindgen_export2), f = u;
        e.document_setCardExtNamespace(d, this.__wbg_ptr, t, g, f, b(o));
        var r = a().getInt32(d + 0, true), i = a().getInt32(d + 4, true);
        if (i) throw s(r);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setCardKind(t, _) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16), d = w(_, e.__wbindgen_export, e.__wbindgen_export2), g = u;
        e.document_setCardKind(i, this.__wbg_ptr, t, d, g);
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        if (r) throw s(o);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setExt(t) {
      try {
        const r = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_setExt(r, this.__wbg_ptr, b(t));
        var _ = a().getInt32(r + 0, true), o = a().getInt32(r + 4, true);
        if (o) throw s(_);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setExtNamespace(t, _) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16), d = w(t, e.__wbindgen_export, e.__wbindgen_export2), g = u;
        e.document_setExtNamespace(i, this.__wbg_ptr, d, g, b(_));
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        if (r) throw s(o);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setField(t, _) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16), d = w(t, e.__wbindgen_export, e.__wbindgen_export2), g = u;
        e.document_setField(i, this.__wbg_ptr, d, g, b(_));
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        if (r) throw s(o);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setFields(t) {
      try {
        const r = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_setFields(r, this.__wbg_ptr, b(t));
        var _ = a().getInt32(r + 0, true), o = a().getInt32(r + 4, true);
        if (o) throw s(_);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setFill(t, _) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16), d = w(t, e.__wbindgen_export, e.__wbindgen_export2), g = u;
        e.document_setFill(i, this.__wbg_ptr, d, g, b(_));
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        if (r) throw s(o);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setQuillRef(t) {
      try {
        const r = e.__wbindgen_add_to_stack_pointer(-16), i = w(t, e.__wbindgen_export, e.__wbindgen_export2), d = u;
        e.document_setQuillRef(r, this.__wbg_ptr, i, d);
        var _ = a().getInt32(r + 0, true), o = a().getInt32(r + 4, true);
        if (o) throw s(_);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setSeedNamespace(t, _) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16), d = w(t, e.__wbindgen_export, e.__wbindgen_export2), g = u;
        e.document_setSeedNamespace(i, this.__wbg_ptr, d, g, b(_));
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        if (r) throw s(o);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    toJson() {
      let t, _;
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_toJson(i, this.__wbg_ptr);
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        return t = o, _ = r, p(o, r);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16), e.__wbindgen_export4(t, _, 1);
      }
    }
    toMarkdown() {
      let t, _;
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_toMarkdown(i, this.__wbg_ptr);
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        return t = o, _ = r, p(o, r);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16), e.__wbindgen_export4(t, _, 1);
      }
    }
    static tryFromJson(t) {
      const _ = w(t, e.__wbindgen_export, e.__wbindgen_export2), o = u, r = e.document_tryFromJson(_, o);
      return r === 0 ? void 0 : l.__wrap(r);
    }
    updateCardBody(t, _) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16), d = w(_, e.__wbindgen_export, e.__wbindgen_export2), g = u;
        e.document_updateCardBody(i, this.__wbg_ptr, t, d, g);
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        if (r) throw s(o);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    updateCardField(t, _, o) {
      try {
        const d = e.__wbindgen_add_to_stack_pointer(-16), g = w(_, e.__wbindgen_export, e.__wbindgen_export2), f = u;
        e.document_updateCardField(d, this.__wbg_ptr, t, g, f, b(o));
        var r = a().getInt32(d + 0, true), i = a().getInt32(d + 4, true);
        if (i) throw s(r);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    updateCardFields(t, _) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_updateCardFields(i, this.__wbg_ptr, t, b(_));
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        if (r) throw s(o);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    get warnings() {
      try {
        const r = e.__wbindgen_add_to_stack_pointer(-16);
        e.document_warnings(r, this.__wbg_ptr);
        var t = a().getInt32(r + 0, true), _ = a().getInt32(r + 4, true), o = a().getInt32(r + 8, true);
        if (o) throw s(_);
        return s(t);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
  };
  Symbol.dispose && (l.prototype[Symbol.dispose] = l.prototype.free);
  q = class {
    static __wrap(t) {
      t = t >>> 0;
      const _ = Object.create(q.prototype);
      return _.__wbg_ptr = t, T.register(_, _.__wbg_ptr, _), _;
    }
    __destroy_into_raw() {
      const t = this.__wbg_ptr;
      return this.__wbg_ptr = 0, T.unregister(this), t;
    }
    free() {
      const t = this.__destroy_into_raw();
      e.__wbg_livesession_free(t, 0);
    }
    apply(t) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        y(t, l), e.livesession_apply(i, this.__wbg_ptr, t.__wbg_ptr);
        var _ = a().getInt32(i + 0, true), o = a().getInt32(i + 4, true), r = a().getInt32(i + 8, true);
        if (r) throw s(o);
        return s(_);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    get backendId() {
      let t, _;
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        e.livesession_backendId(i, this.__wbg_ptr);
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        return t = o, _ = r, p(o, r);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16), e.__wbindgen_export4(t, _, 1);
      }
    }
    fieldAt(t, _, o) {
      try {
        const d = e.__wbindgen_add_to_stack_pointer(-16);
        e.livesession_fieldAt(d, this.__wbg_ptr, t, _, o);
        var r = a().getInt32(d + 0, true), i = a().getInt32(d + 4, true);
        let g;
        return r !== 0 && (g = p(r, i).slice(), e.__wbindgen_export4(r, i * 1, 1)), g;
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    locate(t, _) {
      const o = w(t, e.__wbindgen_export, e.__wbindgen_export2), r = u, i = e.livesession_locate(this.__wbg_ptr, o, r, _);
      return s(i);
    }
    get pageCount() {
      return e.livesession_pageCount(this.__wbg_ptr) >>> 0;
    }
    pageSize(t) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        e.livesession_pageSize(i, this.__wbg_ptr, t);
        var _ = a().getInt32(i + 0, true), o = a().getInt32(i + 4, true), r = a().getInt32(i + 8, true);
        if (r) throw s(o);
        return s(_);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    paint(t, _, o) {
      try {
        const g = e.__wbindgen_add_to_stack_pointer(-16);
        e.livesession_paint(g, this.__wbg_ptr, b(t), _, b(o));
        var r = a().getInt32(g + 0, true), i = a().getInt32(g + 4, true), d = a().getInt32(g + 8, true);
        if (d) throw s(i);
        return s(r);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    positionAt(t, _, o) {
      const r = e.livesession_positionAt(this.__wbg_ptr, t, _, o);
      return s(r);
    }
    regions() {
      try {
        const r = e.__wbindgen_add_to_stack_pointer(-16);
        e.livesession_regions(r, this.__wbg_ptr);
        var t = a().getInt32(r + 0, true), _ = a().getInt32(r + 4, true), o = a().getInt32(r + 8, true);
        if (o) throw s(_);
        return s(t);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    render(t) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        e.livesession_render(i, this.__wbg_ptr, m(t) ? 0 : b(t));
        var _ = a().getInt32(i + 0, true), o = a().getInt32(i + 4, true), r = a().getInt32(i + 8, true);
        if (r) throw s(o);
        return s(_);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    get supportsCanvas() {
      return e.livesession_supportsCanvas(this.__wbg_ptr) !== 0;
    }
    get warnings() {
      try {
        const r = e.__wbindgen_add_to_stack_pointer(-16);
        e.livesession_warnings(r, this.__wbg_ptr);
        var t = a().getInt32(r + 0, true), _ = a().getInt32(r + 4, true), o = a().getInt32(r + 8, true);
        if (o) throw s(_);
        return s(t);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
  };
  Symbol.dispose && (q.prototype[Symbol.dispose] = q.prototype.free);
  h = class {
    static __wrap(t) {
      t = t >>> 0;
      const _ = Object.create(h.prototype);
      return _.__wbg_ptr = t, U.register(_, _.__wbg_ptr, _), _;
    }
    __destroy_into_raw() {
      const t = this.__wbg_ptr;
      return this.__wbg_ptr = 0, U.unregister(this), t;
    }
    free() {
      const t = this.__destroy_into_raw();
      e.__wbg_quill_free(t, 0);
    }
    get backendId() {
      let t, _;
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        e.quill_backendId(i, this.__wbg_ptr);
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        return t = o, _ = r, p(o, r);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16), e.__wbindgen_export4(t, _, 1);
      }
    }
    get blueprint() {
      let t, _;
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        e.quill_blueprint(i, this.__wbg_ptr);
        var o = a().getInt32(i + 0, true), r = a().getInt32(i + 4, true);
        return t = o, _ = r, p(o, r);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16), e.__wbindgen_export4(t, _, 1);
      }
    }
    static fromTree(t) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        e.quill_fromTree(i, b(t));
        var _ = a().getInt32(i + 0, true), o = a().getInt32(i + 4, true), r = a().getInt32(i + 8, true);
        if (r) throw s(o);
        return h.__wrap(_);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    get metadata() {
      try {
        const r = e.__wbindgen_add_to_stack_pointer(-16);
        e.quill_metadata(r, this.__wbg_ptr);
        var t = a().getInt32(r + 0, true), _ = a().getInt32(r + 4, true), o = a().getInt32(r + 8, true);
        if (o) throw s(_);
        return s(t);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    get schema() {
      try {
        const r = e.__wbindgen_add_to_stack_pointer(-16);
        e.quill_schema(r, this.__wbg_ptr);
        var t = a().getInt32(r + 0, true), _ = a().getInt32(r + 4, true), o = a().getInt32(r + 8, true);
        if (o) throw s(_);
        return s(t);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    seedCard(t, _) {
      try {
        const d = e.__wbindgen_add_to_stack_pointer(-16), g = w(t, e.__wbindgen_export, e.__wbindgen_export2), f = u;
        e.quill_seedCard(d, this.__wbg_ptr, g, f, b(_));
        var o = a().getInt32(d + 0, true), r = a().getInt32(d + 4, true), i = a().getInt32(d + 8, true);
        if (i) throw s(r);
        return s(o);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    seedDocument() {
      const t = e.quill_seedDocument(this.__wbg_ptr);
      return l.__wrap(t);
    }
    seedMain() {
      try {
        const r = e.__wbindgen_add_to_stack_pointer(-16);
        e.quill_seedMain(r, this.__wbg_ptr);
        var t = a().getInt32(r + 0, true), _ = a().getInt32(r + 4, true), o = a().getInt32(r + 8, true);
        if (o) throw s(_);
        return s(t);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    toTree() {
      const t = e.quill_toTree(this.__wbg_ptr);
      return s(t);
    }
    validate(t) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        y(t, l), e.quill_validate(i, this.__wbg_ptr, t.__wbg_ptr);
        var _ = a().getInt32(i + 0, true), o = a().getInt32(i + 4, true), r = a().getInt32(i + 8, true);
        if (r) throw s(o);
        return s(_);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
  };
  Symbol.dispose && (h.prototype[Symbol.dispose] = h.prototype.free);
  D = class {
    __destroy_into_raw() {
      const t = this.__wbg_ptr;
      return this.__wbg_ptr = 0, z.unregister(this), t;
    }
    free() {
      const t = this.__destroy_into_raw();
      e.__wbg_quillmark_free(t, 0);
    }
    constructor() {
      const t = e.quillmark_new();
      return this.__wbg_ptr = t >>> 0, z.register(this, this.__wbg_ptr, this), this;
    }
    open(t, _) {
      try {
        const d = e.__wbindgen_add_to_stack_pointer(-16);
        y(t, h), y(_, l), e.quillmark_open(d, this.__wbg_ptr, t.__wbg_ptr, _.__wbg_ptr);
        var o = a().getInt32(d + 0, true), r = a().getInt32(d + 4, true), i = a().getInt32(d + 8, true);
        if (i) throw s(r);
        return q.__wrap(o);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    render(t, _, o) {
      try {
        const g = e.__wbindgen_add_to_stack_pointer(-16);
        y(t, h), y(_, l), e.quillmark_render(g, this.__wbg_ptr, t.__wbg_ptr, _.__wbg_ptr, m(o) ? 0 : b(o));
        var r = a().getInt32(g + 0, true), i = a().getInt32(g + 4, true), d = a().getInt32(g + 8, true);
        if (d) throw s(i);
        return s(r);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    supportedFormats(t) {
      try {
        const i = e.__wbindgen_add_to_stack_pointer(-16);
        y(t, h), e.quillmark_supportedFormats(i, this.__wbg_ptr, t.__wbg_ptr);
        var _ = a().getInt32(i + 0, true), o = a().getInt32(i + 4, true), r = a().getInt32(i + 8, true);
        if (r) throw s(o);
        return s(_);
      } finally {
        e.__wbindgen_add_to_stack_pointer(16);
      }
    }
    supportsCanvas(t) {
      return y(t, h), e.quillmark_supportsCanvas(this.__wbg_ptr, t.__wbg_ptr) !== 0;
    }
  };
  Symbol.dispose && (D.prototype[Symbol.dispose] = D.prototype.free);
  Y_ = function() {
    e.init();
  };
  function Q(n, t) {
    const _ = Error(p(n, t));
    return b(_);
  }
  function W(n, t) {
    const _ = String(c(t)), o = w(_, e.__wbindgen_export, e.__wbindgen_export2), r = u;
    a().setInt32(n + 4, r, true), a().setInt32(n + 0, o, true);
  }
  function H(n, t) {
    const _ = String(c(t)), o = w(_, e.__wbindgen_export, e.__wbindgen_export2), r = u;
    a().setInt32(n + 4, r, true), a().setInt32(n + 0, o, true);
  }
  function K(n, t) {
    const _ = c(t), o = typeof _ == "bigint" ? _ : void 0;
    a().setBigInt64(n + 8, m(o) ? BigInt(0) : o, true), a().setInt32(n + 0, !m(o), true);
  }
  function P(n) {
    const t = c(n), _ = typeof t == "boolean" ? t : void 0;
    return m(_) ? 16777215 : _ ? 1 : 0;
  }
  function X(n, t) {
    const _ = N(c(t)), o = w(_, e.__wbindgen_export, e.__wbindgen_export2), r = u;
    a().setInt32(n + 4, r, true), a().setInt32(n + 0, o, true);
  }
  function Y(n, t) {
    return c(n) in c(t);
  }
  function G(n) {
    return typeof c(n) == "bigint";
  }
  function Z(n) {
    return typeof c(n) == "function";
  }
  function tt(n) {
    return c(n) === null;
  }
  function et(n) {
    const t = c(n);
    return typeof t == "object" && t !== null;
  }
  function _t(n) {
    return typeof c(n) == "string";
  }
  function nt(n) {
    return c(n) === void 0;
  }
  function rt(n, t) {
    return c(n) === c(t);
  }
  function ot(n, t) {
    return c(n) == c(t);
  }
  function it(n, t) {
    const _ = c(t), o = typeof _ == "number" ? _ : void 0;
    a().setFloat64(n + 8, m(o) ? 0 : o, true), a().setInt32(n + 0, !m(o), true);
  }
  function at(n, t) {
    const _ = c(t), o = typeof _ == "string" ? _ : void 0;
    var r = m(o) ? 0 : w(o, e.__wbindgen_export, e.__wbindgen_export2), i = u;
    a().setInt32(n + 4, i, true), a().setInt32(n + 0, r, true);
  }
  function dt(n, t) {
    throw new Error(p(n, t));
  }
  function st() {
    return I(function(n, t) {
      const _ = c(n).call(c(t));
      return b(_);
    }, arguments);
  }
  function ct(n) {
    const t = c(n).canvas;
    return m(t) ? 0 : b(t);
  }
  function gt(n) {
    const t = c(n).canvas;
    return b(t);
  }
  function bt(n) {
    return c(n).done;
  }
  function ut(n) {
    const t = c(n).entries();
    return b(t);
  }
  function wt(n) {
    const t = Object.entries(c(n));
    return b(t);
  }
  function ft(n, t) {
    let _, o;
    try {
      _ = n, o = t, console.error(p(n, t));
    } finally {
      e.__wbindgen_export4(_, o, 1);
    }
  }
  function pt(n) {
    const t = Array.from(c(n));
    return b(t);
  }
  function lt() {
    return I(function(n, t) {
      globalThis.crypto.getRandomValues(R(n, t));
    }, arguments);
  }
  function mt() {
    return I(function(n, t) {
      const _ = Reflect.get(c(n), c(t));
      return b(_);
    }, arguments);
  }
  function yt(n, t) {
    const _ = c(n)[t >>> 0];
    return b(_);
  }
  function vt(n, t) {
    const _ = c(n)[t >>> 0];
    return b(_);
  }
  function ht(n, t) {
    const _ = c(n)[c(t)];
    return b(_);
  }
  function It(n, t) {
    const _ = c(n)[c(t)];
    return b(_);
  }
  function kt(n) {
    let t;
    try {
      t = c(n) instanceof ArrayBuffer;
    } catch {
      t = false;
    }
    return t;
  }
  function xt(n) {
    let t;
    try {
      t = c(n) instanceof CanvasRenderingContext2D;
    } catch {
      t = false;
    }
    return t;
  }
  function Ct(n) {
    let t;
    try {
      t = c(n) instanceof Map;
    } catch {
      t = false;
    }
    return t;
  }
  function Ft(n) {
    let t;
    try {
      t = c(n) instanceof Object;
    } catch {
      t = false;
    }
    return t;
  }
  function qt(n) {
    let t;
    try {
      t = c(n) instanceof OffscreenCanvasRenderingContext2D;
    } catch {
      t = false;
    }
    return t;
  }
  function St(n) {
    let t;
    try {
      t = c(n) instanceof Uint8Array;
    } catch {
      t = false;
    }
    return t;
  }
  function Et(n) {
    return Array.isArray(c(n));
  }
  function jt(n) {
    return Number.isSafeInteger(c(n));
  }
  function At() {
    return b(Symbol.iterator);
  }
  function Rt(n) {
    const t = Object.keys(c(n));
    return b(t);
  }
  function Ot(n) {
    return c(n).length;
  }
  function Mt(n) {
    return c(n).length;
  }
  function Nt(n) {
    const t = new Uint8Array(c(n));
    return b(t);
  }
  function Dt() {
    const n = new Error();
    return b(n);
  }
  function Tt() {
    return b(/* @__PURE__ */ new Map());
  }
  function Ut(n, t) {
    const _ = new Error(p(n, t));
    return b(_);
  }
  function zt() {
    const n = new Array();
    return b(n);
  }
  function Bt() {
    const n = new Object();
    return b(n);
  }
  function Vt(n, t) {
    const _ = new Uint8Array(R(n, t));
    return b(_);
  }
  function Jt() {
    return I(function(n, t, _, o) {
      const r = new ImageData(we(n, t), _ >>> 0, o >>> 0);
      return b(r);
    }, arguments);
  }
  function $t() {
    return I(function(n) {
      const t = c(n).next();
      return b(t);
    }, arguments);
  }
  function Lt(n) {
    const t = c(n).next;
    return b(t);
  }
  function Qt() {
    return Date.now();
  }
  function Wt(n, t, _) {
    Uint8Array.prototype.set.call(R(n, t), c(_));
  }
  function Ht() {
    return I(function(n, t, _, o) {
      c(n).putImageData(c(t), _, o);
    }, arguments);
  }
  function Kt() {
    return I(function(n, t, _, o) {
      c(n).putImageData(c(t), _, o);
    }, arguments);
  }
  function Pt() {
    return I(function(n, t, _) {
      return Reflect.set(c(n), c(t), c(_));
    }, arguments);
  }
  function Xt(n, t, _) {
    c(n)[t >>> 0] = s(_);
  }
  function Yt(n, t, _) {
    c(n)[s(t)] = s(_);
  }
  function Gt(n, t, _) {
    c(n)[s(t)] = s(_);
  }
  function Zt(n, t, _) {
    const o = c(n).set(c(t), c(_));
    return b(o);
  }
  function te(n, t) {
    c(n).height = t >>> 0;
  }
  function ee(n, t) {
    c(n).height = t >>> 0;
  }
  function _e(n, t) {
    c(n).width = t >>> 0;
  }
  function ne(n, t) {
    c(n).width = t >>> 0;
  }
  function re(n, t) {
    const _ = c(t).stack, o = w(_, e.__wbindgen_export, e.__wbindgen_export2), r = u;
    a().setInt32(n + 4, r, true), a().setInt32(n + 0, o, true);
  }
  function oe(n) {
    const t = c(n).value;
    return b(t);
  }
  function ie(n) {
    return b(n);
  }
  function ae(n) {
    return b(n);
  }
  function de(n, t) {
    const _ = R(n, t);
    return b(_);
  }
  function se(n, t) {
    const _ = p(n, t);
    return b(_);
  }
  function ce(n) {
    const t = BigInt.asUintN(64, n);
    return b(t);
  }
  function ge(n) {
    const t = c(n);
    return b(t);
  }
  function be(n) {
    s(n);
  }
  const O = typeof FinalizationRegistry > "u" ? {
    register: () => {
    },
    unregister: () => {
    }
  } : new FinalizationRegistry((n) => e.__wbg_document_free(n >>> 0, 1)), T = typeof FinalizationRegistry > "u" ? {
    register: () => {
    },
    unregister: () => {
    }
  } : new FinalizationRegistry((n) => e.__wbg_livesession_free(n >>> 0, 1)), U = typeof FinalizationRegistry > "u" ? {
    register: () => {
    },
    unregister: () => {
    }
  } : new FinalizationRegistry((n) => e.__wbg_quill_free(n >>> 0, 1)), z = typeof FinalizationRegistry > "u" ? {
    register: () => {
    },
    unregister: () => {
    }
  } : new FinalizationRegistry((n) => e.__wbg_quillmark_free(n >>> 0, 1));
  function b(n) {
    C === v.length && v.push(v.length + 1);
    const t = C;
    return C = v[t], v[t] = n, t;
  }
  function y(n, t) {
    if (!(n instanceof t)) throw new Error(`expected instance of ${t.name}`);
  }
  function N(n) {
    const t = typeof n;
    if (t == "number" || t == "boolean" || n == null) return `${n}`;
    if (t == "string") return `"${n}"`;
    if (t == "symbol") {
      const r = n.description;
      return r == null ? "Symbol" : `Symbol(${r})`;
    }
    if (t == "function") {
      const r = n.name;
      return typeof r == "string" && r.length > 0 ? `Function(${r})` : "Function";
    }
    if (Array.isArray(n)) {
      const r = n.length;
      let i = "[";
      r > 0 && (i += N(n[0]));
      for (let d = 1; d < r; d++) i += ", " + N(n[d]);
      return i += "]", i;
    }
    const _ = /\[object ([^\]]+)\]/.exec(toString.call(n));
    let o;
    if (_ && _.length > 1) o = _[1];
    else return toString.call(n);
    if (o == "Object") try {
      return "Object(" + JSON.stringify(n) + ")";
    } catch {
      return "Object";
    }
    return n instanceof Error ? `${n.name}: ${n.message}
${n.stack}` : o;
  }
  function ue(n) {
    n < 1028 || (v[n] = C, C = n);
  }
  function R(n, t) {
    return n = n >>> 0, x().subarray(n / 1, n / 1 + t);
  }
  function we(n, t) {
    return n = n >>> 0, fe().subarray(n / 1, n / 1 + t);
  }
  let k = null;
  function a() {
    return (k === null || k.buffer.detached === true || k.buffer.detached === void 0 && k.buffer !== e.memory.buffer) && (k = new DataView(e.memory.buffer)), k;
  }
  function p(n, t) {
    return n = n >>> 0, le(n, t);
  }
  let E = null;
  function x() {
    return (E === null || E.byteLength === 0) && (E = new Uint8Array(e.memory.buffer)), E;
  }
  let j = null;
  function fe() {
    return (j === null || j.byteLength === 0) && (j = new Uint8ClampedArray(e.memory.buffer)), j;
  }
  function c(n) {
    return v[n];
  }
  function I(n, t) {
    try {
      return n.apply(this, t);
    } catch (_) {
      e.__wbindgen_export3(b(_));
    }
  }
  let v = new Array(1024).fill(void 0);
  v.push(void 0, null, true, false);
  let C = v.length;
  function m(n) {
    return n == null;
  }
  function w(n, t, _) {
    if (_ === void 0) {
      const g = F.encode(n), f = t(g.length, 1) >>> 0;
      return x().subarray(f, f + g.length).set(g), u = g.length, f;
    }
    let o = n.length, r = t(o, 1) >>> 0;
    const i = x();
    let d = 0;
    for (; d < o; d++) {
      const g = n.charCodeAt(d);
      if (g > 127) break;
      i[r + d] = g;
    }
    if (d !== o) {
      d !== 0 && (n = n.slice(d)), r = _(r, o, o = d + n.length * 3, 1) >>> 0;
      const g = x().subarray(r + d, r + o), f = F.encodeInto(n, g);
      d += f.written, r = _(r, o, d, 1) >>> 0;
    }
    return u = d, r;
  }
  function s(n) {
    const t = c(n);
    return ue(n), t;
  }
  let A = new TextDecoder("utf-8", {
    ignoreBOM: true,
    fatal: true
  });
  A.decode();
  const pe = 2146435072;
  let M = 0;
  function le(n, t) {
    return M += t, M >= pe && (A = new TextDecoder("utf-8", {
      ignoreBOM: true,
      fatal: true
    }), A.decode(), M = t), A.decode(x().subarray(n, n + t));
  }
  const F = new TextEncoder();
  "encodeInto" in F || (F.encodeInto = function(n, t) {
    const _ = F.encode(n);
    return t.set(_), {
      read: n.length,
      written: _.length
    };
  });
  let u = 0, e;
  function me(n) {
    e = n;
  }
  URL = globalThis.URL;
  const ye = await $({
    "./wasm_bg.js": {
      __wbindgen_object_clone_ref: ge,
      __wbindgen_object_drop_ref: be,
      __wbg_get_unchecked_17f53dad852b9588: vt,
      __wbg_set_3bf1de9fab0cd644: Xt,
      __wbg_length_3d4ecd04bd8d22f1: Ot,
      __wbg_set_fde2cec06c23692b: Zt,
      __wbg_entries_2bf997cf82353e47: ut,
      __wbg_next_0340c4ae324393c3: $t,
      __wbg_instanceof_Object_7c99480a1cdfb911: Ft,
      __wbg_instanceof_Map_1b76fd4635be43eb: Ct,
      __wbg_done_9158f7cc8751ba32: bt,
      __wbg_value_ee3a06f4579184fa: oe,
      __wbg_keys_2fd1bfdda7e278ca: Rt,
      __wbg_new_227d7c05414eb861: Dt,
      __wbg_stack_3b0d974bbf31e44f: re,
      __wbg_error_a6fa202b58aa1cd3: ft,
      __wbg_new_with_u8_clamped_array_and_sh_fe957411824b5158: Jt,
      __wbg_set_height_24d07d982f176ac6: te,
      __wbg_set_width_adc925bca9c5351a: ne,
      __wbg_set_height_be9b2b920bd68401: ee,
      __wbg_set_width_5cda41d4d06a14dd: _e,
      __wbg_instanceof_CanvasRenderingContext2d_24a3fe06e62b98d7: xt,
      __wbg_putImageData_c810e62ea70e761d: Ht,
      __wbg_canvas_2c0c6d263d4c52ad: ct,
      __wbg_instanceof_OffscreenCanvasRenderingContext2d_285a274020b4f230: qt,
      __wbg_putImageData_cb4de9afd58963be: Kt,
      __wbg_canvas_374da9f3c5b3dd0e: gt,
      __wbg_get_with_ref_key_6412cf3094599694: ht,
      __wbg_set_6be42768c690e380: Yt,
      __wbg_get_8360291721e2339f: yt,
      __wbg_String_8564e559799eccda: W,
      __wbg_get_with_ref_key_f64427178466f623: It,
      __wbg_set_f071dbb3bd088e0e: Gt,
      __wbg_String_b51de6b05a10845b: H,
      __wbg_getRandomValues_3f44b700395062e5: lt,
      __wbg_new_from_slice_b5ea43e23f6008c0: Vt,
      __wbg_new_0c7403db6e782f19: Nt,
      __wbg_length_9f1775224cf1d815: Mt,
      __wbg_prototypesetcall_a6b02eb00b0f4ce2: Wt,
      __wbg_call_14b169f759b26747: st,
      __wbg_instanceof_Uint8Array_152ba1f289edcf3f: St,
      __wbg_instanceof_ArrayBuffer_7c8433c6ed14ffe3: kt,
      __wbg_new_34d45cc8e36aaead: Tt,
      __wbg_now_a9b7df1cbee90986: Qt,
      __wbg_new_682678e2f47e32bc: zt,
      __wbg_from_0dbf29f09e7fb200: pt,
      __wbg_isArray_c3109d14ffc06469: Et,
      __wbg_new_5e360d2ff7b9e1c3: Ut,
      __wbg_isSafeInteger_4fc213d1989d6d2a: jt,
      __wbg_new_aa8d0fa9762c29bd: Bt,
      __wbg_entries_e0b73aa8571ddb56: wt,
      __wbg_iterator_013bc09ec998c2a7: At,
      __wbg_get_1affdbdd5573b16a: mt,
      __wbg_set_022bee52d0b05b19: Pt,
      __wbg_next_7646edaa39458ef7: Lt,
      __wbg___wbindgen_in_a5d8b22e52b24dd1: Y,
      __wbg___wbindgen_throw_6b64449b9b9ed33c: dt,
      __wbg___wbindgen_is_null_52ff4ec04186736f: tt,
      __wbg___wbindgen_jsval_eq_d3465d8a07697228: rt,
      __wbg_Error_960c155d3d49e4c2: Q,
      __wbg___wbindgen_is_bigint_ec25c7f91b4d9e93: G,
      __wbg___wbindgen_is_object_63322ec0cd6ea4ef: et,
      __wbg___wbindgen_is_string_6df3bf7ef1164ed3: _t,
      __wbg___wbindgen_number_get_c7f42aed0525c451: it,
      __wbg___wbindgen_string_get_7ed5322991caaec5: at,
      __wbg___wbindgen_boolean_get_6ea149f0a8dcc5ff: P,
      __wbg___wbindgen_is_function_3baa9db1a987f47d: Z,
      __wbg___wbindgen_is_undefined_29a43b4d42920abd: nt,
      __wbg___wbindgen_jsval_loose_eq_cac3565e89b4134c: ot,
      __wbg___wbindgen_bigint_get_as_i64_3d3aba5d616c6a51: K,
      __wbg___wbindgen_debug_string_ab4b34d23d6778bd: X,
      __wbindgen_cast_0000000000000001: ie,
      __wbindgen_cast_0000000000000002: ae,
      __wbindgen_cast_0000000000000003: de,
      __wbindgen_cast_0000000000000004: se,
      __wbindgen_cast_0000000000000005: ce
    }
  }, L), { memory: ve, __wbg_document_free: he, __wbg_livesession_free: Ie, __wbg_quill_free: ke, __wbg_quillmark_free: xe, document_blueprintInstruction: Ce, document_cardCount: Fe, document_cards: qe, document_clone: Se, document_currentSchemaVersion: Ee, document_equals: je, document_formatDiagnostic: Ae, document_formatRules: Re, document_fromJson: Oe, document_fromMarkdown: Me, document_insertCard: Ne, document_main: De, document_makeCard: Te, document_moveCard: Ue, document_new: ze, document_pushCard: Be, document_quillRef: Ve, document_quillRefHint: Je, document_removeCard: $e, document_removeCardExt: Le, document_removeCardExtNamespace: Qe, document_removeCardField: We, document_removeExt: He, document_removeExtNamespace: Ke, document_removeField: Pe, document_removeSeedNamespace: Xe, document_replaceBody: Ye, document_schemaVersionOf: Ge, document_setCardExt: Ze, document_setCardExtNamespace: t_, document_setCardKind: e_, document_setExt: __, document_setExtNamespace: n_, document_setField: r_, document_setFields: o_, document_setFill: i_, document_setQuillRef: a_, document_setSeedNamespace: d_, document_toJson: s_, document_toMarkdown: c_, document_tryFromJson: g_, document_updateCardBody: b_, document_updateCardField: u_, document_updateCardFields: w_, document_warnings: f_, init: p_, livesession_apply: l_, livesession_backendId: m_, livesession_fieldAt: y_, livesession_locate: v_, livesession_pageCount: h_, livesession_pageSize: I_, livesession_paint: k_, livesession_positionAt: x_, livesession_regions: C_, livesession_render: F_, livesession_supportsCanvas: q_, livesession_warnings: S_, quill_backendId: E_, quill_blueprint: j_, quill_fromTree: A_, quill_metadata: R_, quill_schema: O_, quill_seedCard: M_, quill_seedDocument: N_, quill_seedMain: D_, quill_toTree: T_, quill_validate: U_, quillmark_new: z_, quillmark_open: B_, quillmark_render: V_, quillmark_supportedFormats: J_, quillmark_supportsCanvas: $_, __wbindgen_export: L_, __wbindgen_export2: Q_, __wbindgen_export3: W_, __wbindgen_export4: H_, __wbindgen_add_to_stack_pointer: K_, __wbindgen_start: B } = ye, P_ = Object.freeze(Object.defineProperty({
    __proto__: null,
    __wbg_document_free: he,
    __wbg_livesession_free: Ie,
    __wbg_quill_free: ke,
    __wbg_quillmark_free: xe,
    __wbindgen_add_to_stack_pointer: K_,
    __wbindgen_export: L_,
    __wbindgen_export2: Q_,
    __wbindgen_export3: W_,
    __wbindgen_export4: H_,
    __wbindgen_start: B,
    document_blueprintInstruction: Ce,
    document_cardCount: Fe,
    document_cards: qe,
    document_clone: Se,
    document_currentSchemaVersion: Ee,
    document_equals: je,
    document_formatDiagnostic: Ae,
    document_formatRules: Re,
    document_fromJson: Oe,
    document_fromMarkdown: Me,
    document_insertCard: Ne,
    document_main: De,
    document_makeCard: Te,
    document_moveCard: Ue,
    document_new: ze,
    document_pushCard: Be,
    document_quillRef: Ve,
    document_quillRefHint: Je,
    document_removeCard: $e,
    document_removeCardExt: Le,
    document_removeCardExtNamespace: Qe,
    document_removeCardField: We,
    document_removeExt: He,
    document_removeExtNamespace: Ke,
    document_removeField: Pe,
    document_removeSeedNamespace: Xe,
    document_replaceBody: Ye,
    document_schemaVersionOf: Ge,
    document_setCardExt: Ze,
    document_setCardExtNamespace: t_,
    document_setCardKind: e_,
    document_setExt: __,
    document_setExtNamespace: n_,
    document_setField: r_,
    document_setFields: o_,
    document_setFill: i_,
    document_setQuillRef: a_,
    document_setSeedNamespace: d_,
    document_toJson: s_,
    document_toMarkdown: c_,
    document_tryFromJson: g_,
    document_updateCardBody: b_,
    document_updateCardField: u_,
    document_updateCardFields: w_,
    document_warnings: f_,
    init: p_,
    livesession_apply: l_,
    livesession_backendId: m_,
    livesession_fieldAt: y_,
    livesession_locate: v_,
    livesession_pageCount: h_,
    livesession_pageSize: I_,
    livesession_paint: k_,
    livesession_positionAt: x_,
    livesession_regions: C_,
    livesession_render: F_,
    livesession_supportsCanvas: q_,
    livesession_warnings: S_,
    memory: ve,
    quill_backendId: E_,
    quill_blueprint: j_,
    quill_fromTree: A_,
    quill_metadata: R_,
    quill_schema: O_,
    quill_seedCard: M_,
    quill_seedDocument: N_,
    quill_seedMain: D_,
    quill_toTree: T_,
    quill_validate: U_,
    quillmark_new: z_,
    quillmark_open: B_,
    quillmark_render: V_,
    quillmark_supportedFormats: J_,
    quillmark_supportsCanvas: $_
  }, Symbol.toStringTag, {
    value: "Module"
  }));
  me(P_);
  B();
});
export {
  l as Document,
  q as LiveSession,
  h as Quill,
  D as Quillmark,
  __tla,
  Y_ as init
};
