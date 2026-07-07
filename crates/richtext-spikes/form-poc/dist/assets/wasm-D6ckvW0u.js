import { _ as $, __tla as __tla_0 } from "./index-BNuI42xC.js";
let l, q, h, U, nn;
let __tla = Promise.all([
  (() => {
    try {
      return __tla_0;
    } catch {
    }
  })()
]).then(async () => {
  const L = "/assets/wasm_bg-C67N7GW0.wasm";
  l = class {
    static __wrap(t) {
      t = t >>> 0;
      const _ = Object.create(l.prototype);
      return _.__wbg_ptr = t, T.register(_, _.__wbg_ptr, _), _;
    }
    __destroy_into_raw() {
      const t = this.__wbg_ptr;
      return this.__wbg_ptr = 0, T.unregister(this), t;
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
        return this.__wbg_ptr = _ >>> 0, T.register(this, this.__wbg_ptr, this), this;
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
      return _.__wbg_ptr = t, D.register(_, _.__wbg_ptr, _), _;
    }
    __destroy_into_raw() {
      const t = this.__wbg_ptr;
      return this.__wbg_ptr = 0, D.unregister(this), t;
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
      return _.__wbg_ptr = t, N.register(_, _.__wbg_ptr, _), _;
    }
    __destroy_into_raw() {
      const t = this.__wbg_ptr;
      return this.__wbg_ptr = 0, N.unregister(this), t;
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
  U = class {
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
  Symbol.dispose && (U.prototype[Symbol.dispose] = U.prototype.free);
  nn = function() {
    e.init();
  };
  function W(n, t) {
    const _ = Error(p(n, t));
    return b(_);
  }
  function Q(n, t) {
    const _ = String(c(t)), o = w(_, e.__wbindgen_export, e.__wbindgen_export2), r = u;
    a().setInt32(n + 4, r, true), a().setInt32(n + 0, o, true);
  }
  function H(n, t) {
    const _ = String(c(t)), o = w(_, e.__wbindgen_export, e.__wbindgen_export2), r = u;
    a().setInt32(n + 4, r, true), a().setInt32(n + 0, o, true);
  }
  function Y(n, t) {
    const _ = c(t), o = typeof _ == "bigint" ? _ : void 0;
    a().setBigInt64(n + 8, m(o) ? BigInt(0) : o, true), a().setInt32(n + 0, !m(o), true);
  }
  function K(n) {
    const t = c(n), _ = typeof t == "boolean" ? t : void 0;
    return m(_) ? 16777215 : _ ? 1 : 0;
  }
  function G(n, t) {
    const _ = O(c(t)), o = w(_, e.__wbindgen_export, e.__wbindgen_export2), r = u;
    a().setInt32(n + 4, r, true), a().setInt32(n + 0, o, true);
  }
  function P(n, t) {
    return c(n) in c(t);
  }
  function X(n) {
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
  function mt(n) {
    return c(n).getTime();
  }
  function yt(n) {
    return c(n).getUTCDate();
  }
  function vt(n) {
    return c(n).getUTCFullYear();
  }
  function ht(n) {
    return c(n).getUTCMonth();
  }
  function It() {
    return I(function(n, t) {
      const _ = Reflect.get(c(n), c(t));
      return b(_);
    }, arguments);
  }
  function kt(n, t) {
    const _ = c(n)[t >>> 0];
    return b(_);
  }
  function xt(n, t) {
    const _ = c(n)[t >>> 0];
    return b(_);
  }
  function Ct(n, t) {
    const _ = c(n)[c(t)];
    return b(_);
  }
  function Ft(n, t) {
    const _ = c(n)[c(t)];
    return b(_);
  }
  function qt(n) {
    let t;
    try {
      t = c(n) instanceof ArrayBuffer;
    } catch {
      t = false;
    }
    return t;
  }
  function St(n) {
    let t;
    try {
      t = c(n) instanceof CanvasRenderingContext2D;
    } catch {
      t = false;
    }
    return t;
  }
  function Et(n) {
    let t;
    try {
      t = c(n) instanceof Map;
    } catch {
      t = false;
    }
    return t;
  }
  function At(n) {
    let t;
    try {
      t = c(n) instanceof Object;
    } catch {
      t = false;
    }
    return t;
  }
  function jt(n) {
    let t;
    try {
      t = c(n) instanceof OffscreenCanvasRenderingContext2D;
    } catch {
      t = false;
    }
    return t;
  }
  function Rt(n) {
    let t;
    try {
      t = c(n) instanceof Uint8Array;
    } catch {
      t = false;
    }
    return t;
  }
  function Tt(n) {
    return Array.isArray(c(n));
  }
  function Mt(n) {
    return Number.isSafeInteger(c(n));
  }
  function Ot() {
    return b(Symbol.iterator);
  }
  function Ut(n) {
    const t = Object.keys(c(n));
    return b(t);
  }
  function Dt(n) {
    return c(n).length;
  }
  function Nt(n) {
    return c(n).length;
  }
  function zt() {
    return b(/* @__PURE__ */ new Date());
  }
  function Bt(n) {
    const t = new Uint8Array(c(n));
    return b(t);
  }
  function Vt() {
    const n = new Error();
    return b(n);
  }
  function Jt() {
    return b(/* @__PURE__ */ new Map());
  }
  function $t(n, t) {
    const _ = new Error(p(n, t));
    return b(_);
  }
  function Lt() {
    const n = new Array();
    return b(n);
  }
  function Wt(n) {
    const t = new Date(c(n));
    return b(t);
  }
  function Qt() {
    const n = new Object();
    return b(n);
  }
  function Ht(n, t) {
    const _ = new Uint8Array(R(n, t));
    return b(_);
  }
  function Yt() {
    return I(function(n, t, _, o) {
      const r = new ImageData(ve(n, t), _ >>> 0, o >>> 0);
      return b(r);
    }, arguments);
  }
  function Kt() {
    return I(function(n) {
      const t = c(n).next();
      return b(t);
    }, arguments);
  }
  function Gt(n) {
    const t = c(n).next;
    return b(t);
  }
  function Pt() {
    return Date.now();
  }
  function Xt(n, t, _) {
    Uint8Array.prototype.set.call(R(n, t), c(_));
  }
  function Zt() {
    return I(function(n, t, _, o) {
      c(n).putImageData(c(t), _, o);
    }, arguments);
  }
  function te() {
    return I(function(n, t, _, o) {
      c(n).putImageData(c(t), _, o);
    }, arguments);
  }
  function ee() {
    return I(function(n, t, _) {
      return Reflect.set(c(n), c(t), c(_));
    }, arguments);
  }
  function _e(n, t, _) {
    c(n)[t >>> 0] = s(_);
  }
  function ne(n, t, _) {
    c(n)[s(t)] = s(_);
  }
  function re(n, t, _) {
    c(n)[s(t)] = s(_);
  }
  function oe(n, t, _) {
    const o = c(n).set(c(t), c(_));
    return b(o);
  }
  function ie(n, t) {
    c(n).height = t >>> 0;
  }
  function ae(n, t) {
    c(n).height = t >>> 0;
  }
  function de(n, t) {
    c(n).width = t >>> 0;
  }
  function se(n, t) {
    c(n).width = t >>> 0;
  }
  function ce(n, t) {
    const _ = c(t).stack, o = w(_, e.__wbindgen_export, e.__wbindgen_export2), r = u;
    a().setInt32(n + 4, r, true), a().setInt32(n + 0, o, true);
  }
  function ge(n) {
    const t = c(n).value;
    return b(t);
  }
  function be(n) {
    return b(n);
  }
  function ue(n) {
    return b(n);
  }
  function we(n, t) {
    const _ = R(n, t);
    return b(_);
  }
  function fe(n, t) {
    const _ = p(n, t);
    return b(_);
  }
  function pe(n) {
    const t = BigInt.asUintN(64, n);
    return b(t);
  }
  function le(n) {
    const t = c(n);
    return b(t);
  }
  function me(n) {
    s(n);
  }
  const T = typeof FinalizationRegistry > "u" ? {
    register: () => {
    },
    unregister: () => {
    }
  } : new FinalizationRegistry((n) => e.__wbg_document_free(n >>> 0, 1)), D = typeof FinalizationRegistry > "u" ? {
    register: () => {
    },
    unregister: () => {
    }
  } : new FinalizationRegistry((n) => e.__wbg_livesession_free(n >>> 0, 1)), N = typeof FinalizationRegistry > "u" ? {
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
  function O(n) {
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
      r > 0 && (i += O(n[0]));
      for (let d = 1; d < r; d++) i += ", " + O(n[d]);
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
  function ye(n) {
    n < 1028 || (v[n] = C, C = n);
  }
  function R(n, t) {
    return n = n >>> 0, x().subarray(n / 1, n / 1 + t);
  }
  function ve(n, t) {
    return n = n >>> 0, he().subarray(n / 1, n / 1 + t);
  }
  let k = null;
  function a() {
    return (k === null || k.buffer.detached === true || k.buffer.detached === void 0 && k.buffer !== e.memory.buffer) && (k = new DataView(e.memory.buffer)), k;
  }
  function p(n, t) {
    return n = n >>> 0, ke(n, t);
  }
  let E = null;
  function x() {
    return (E === null || E.byteLength === 0) && (E = new Uint8Array(e.memory.buffer)), E;
  }
  let A = null;
  function he() {
    return (A === null || A.byteLength === 0) && (A = new Uint8ClampedArray(e.memory.buffer)), A;
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
    return ye(n), t;
  }
  let j = new TextDecoder("utf-8", {
    ignoreBOM: true,
    fatal: true
  });
  j.decode();
  const Ie = 2146435072;
  let M = 0;
  function ke(n, t) {
    return M += t, M >= Ie && (j = new TextDecoder("utf-8", {
      ignoreBOM: true,
      fatal: true
    }), j.decode(), M = t), j.decode(x().subarray(n, n + t));
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
  function xe(n) {
    e = n;
  }
  URL = globalThis.URL;
  const Ce = await $({
    "./wasm_bg.js": {
      __wbindgen_object_clone_ref: le,
      __wbindgen_object_drop_ref: me,
      __wbg_get_unchecked_17f53dad852b9588: xt,
      __wbg_set_3bf1de9fab0cd644: _e,
      __wbg_length_3d4ecd04bd8d22f1: Dt,
      __wbg_set_fde2cec06c23692b: oe,
      __wbg_entries_2bf997cf82353e47: ut,
      __wbg_next_0340c4ae324393c3: Kt,
      __wbg_instanceof_Object_7c99480a1cdfb911: At,
      __wbg_instanceof_Map_1b76fd4635be43eb: Et,
      __wbg_done_9158f7cc8751ba32: bt,
      __wbg_value_ee3a06f4579184fa: ge,
      __wbg_keys_2fd1bfdda7e278ca: Ut,
      __wbg_new_227d7c05414eb861: Vt,
      __wbg_stack_3b0d974bbf31e44f: ce,
      __wbg_error_a6fa202b58aa1cd3: ft,
      __wbg_new_with_u8_clamped_array_and_sh_fe957411824b5158: Yt,
      __wbg_set_height_24d07d982f176ac6: ie,
      __wbg_set_width_adc925bca9c5351a: se,
      __wbg_set_height_be9b2b920bd68401: ae,
      __wbg_set_width_5cda41d4d06a14dd: de,
      __wbg_instanceof_CanvasRenderingContext2d_24a3fe06e62b98d7: St,
      __wbg_putImageData_c810e62ea70e761d: Zt,
      __wbg_canvas_2c0c6d263d4c52ad: ct,
      __wbg_instanceof_OffscreenCanvasRenderingContext2d_285a274020b4f230: jt,
      __wbg_putImageData_cb4de9afd58963be: te,
      __wbg_canvas_374da9f3c5b3dd0e: gt,
      __wbg_get_with_ref_key_6412cf3094599694: Ct,
      __wbg_set_6be42768c690e380: ne,
      __wbg_get_8360291721e2339f: kt,
      __wbg_String_8564e559799eccda: Q,
      __wbg_get_with_ref_key_f64427178466f623: Ft,
      __wbg_set_f071dbb3bd088e0e: re,
      __wbg_String_b51de6b05a10845b: H,
      __wbg_getRandomValues_3f44b700395062e5: lt,
      __wbg_new_from_slice_b5ea43e23f6008c0: Ht,
      __wbg_new_0c7403db6e782f19: Bt,
      __wbg_length_9f1775224cf1d815: Nt,
      __wbg_prototypesetcall_a6b02eb00b0f4ce2: Xt,
      __wbg_call_14b169f759b26747: st,
      __wbg_instanceof_Uint8Array_152ba1f289edcf3f: Rt,
      __wbg_instanceof_ArrayBuffer_7c8433c6ed14ffe3: qt,
      __wbg_new_34d45cc8e36aaead: Jt,
      __wbg_getUTCDate_5cd8b68e971333f7: yt,
      __wbg_getUTCMonth_62fa72a7522ef806: ht,
      __wbg_getUTCFullYear_f3b3950a0ccb9165: vt,
      __wbg_new_7913666fe5070684: Wt,
      __wbg_now_a9b7df1cbee90986: Pt,
      __wbg_new_0_4d657201ced14de3: zt,
      __wbg_getTime_da7c55f52b71e8c6: mt,
      __wbg_new_682678e2f47e32bc: Lt,
      __wbg_from_0dbf29f09e7fb200: pt,
      __wbg_isArray_c3109d14ffc06469: Tt,
      __wbg_new_5e360d2ff7b9e1c3: $t,
      __wbg_isSafeInteger_4fc213d1989d6d2a: Mt,
      __wbg_new_aa8d0fa9762c29bd: Qt,
      __wbg_entries_e0b73aa8571ddb56: wt,
      __wbg_iterator_013bc09ec998c2a7: Ot,
      __wbg_get_1affdbdd5573b16a: It,
      __wbg_set_022bee52d0b05b19: ee,
      __wbg_next_7646edaa39458ef7: Gt,
      __wbg___wbindgen_in_a5d8b22e52b24dd1: P,
      __wbg___wbindgen_throw_6b64449b9b9ed33c: dt,
      __wbg___wbindgen_is_null_52ff4ec04186736f: tt,
      __wbg___wbindgen_jsval_eq_d3465d8a07697228: rt,
      __wbg_Error_960c155d3d49e4c2: W,
      __wbg___wbindgen_is_bigint_ec25c7f91b4d9e93: X,
      __wbg___wbindgen_is_object_63322ec0cd6ea4ef: et,
      __wbg___wbindgen_is_string_6df3bf7ef1164ed3: _t,
      __wbg___wbindgen_number_get_c7f42aed0525c451: it,
      __wbg___wbindgen_string_get_7ed5322991caaec5: at,
      __wbg___wbindgen_boolean_get_6ea149f0a8dcc5ff: K,
      __wbg___wbindgen_is_function_3baa9db1a987f47d: Z,
      __wbg___wbindgen_is_undefined_29a43b4d42920abd: nt,
      __wbg___wbindgen_jsval_loose_eq_cac3565e89b4134c: ot,
      __wbg___wbindgen_bigint_get_as_i64_3d3aba5d616c6a51: Y,
      __wbg___wbindgen_debug_string_ab4b34d23d6778bd: G,
      __wbindgen_cast_0000000000000001: be,
      __wbindgen_cast_0000000000000002: ue,
      __wbindgen_cast_0000000000000003: we,
      __wbindgen_cast_0000000000000004: fe,
      __wbindgen_cast_0000000000000005: pe
    }
  }, L), { memory: Fe, __wbg_document_free: qe, __wbg_livesession_free: Se, __wbg_quill_free: Ee, __wbg_quillmark_free: Ae, document_blueprintInstruction: je, document_cardCount: Re, document_cards: Te, document_clone: Me, document_currentSchemaVersion: Oe, document_equals: Ue, document_formatDiagnostic: De, document_formatRules: Ne, document_fromJson: ze, document_fromMarkdown: Be, document_insertCard: Ve, document_main: Je, document_makeCard: $e, document_moveCard: Le, document_new: We, document_pushCard: Qe, document_quillRef: He, document_quillRefHint: Ye, document_removeCard: Ke, document_removeCardExt: Ge, document_removeCardExtNamespace: Pe, document_removeCardField: Xe, document_removeExt: Ze, document_removeExtNamespace: t_, document_removeField: e_, document_removeSeedNamespace: __, document_replaceBody: n_, document_schemaVersionOf: r_, document_setCardExt: o_, document_setCardExtNamespace: i_, document_setCardKind: a_, document_setExt: d_, document_setExtNamespace: s_, document_setField: c_, document_setFields: g_, document_setFill: b_, document_setQuillRef: u_, document_setSeedNamespace: w_, document_toJson: f_, document_toMarkdown: p_, document_tryFromJson: l_, document_updateCardBody: m_, document_updateCardField: y_, document_updateCardFields: v_, document_warnings: h_, init: I_, livesession_apply: k_, livesession_backendId: x_, livesession_fieldAt: C_, livesession_locate: F_, livesession_pageCount: q_, livesession_pageSize: S_, livesession_paint: E_, livesession_positionAt: A_, livesession_regions: j_, livesession_render: R_, livesession_supportsCanvas: T_, livesession_warnings: M_, quill_backendId: O_, quill_blueprint: U_, quill_fromTree: D_, quill_metadata: N_, quill_schema: z_, quill_seedCard: B_, quill_seedDocument: V_, quill_seedMain: J_, quill_toTree: $_, quill_validate: L_, quillmark_new: W_, quillmark_open: Q_, quillmark_render: H_, quillmark_supportedFormats: Y_, quillmark_supportsCanvas: K_, __wbindgen_export: G_, __wbindgen_export2: P_, __wbindgen_export3: X_, __wbindgen_export4: Z_, __wbindgen_add_to_stack_pointer: tn, __wbindgen_start: B } = Ce, en = Object.freeze(Object.defineProperty({
    __proto__: null,
    __wbg_document_free: qe,
    __wbg_livesession_free: Se,
    __wbg_quill_free: Ee,
    __wbg_quillmark_free: Ae,
    __wbindgen_add_to_stack_pointer: tn,
    __wbindgen_export: G_,
    __wbindgen_export2: P_,
    __wbindgen_export3: X_,
    __wbindgen_export4: Z_,
    __wbindgen_start: B,
    document_blueprintInstruction: je,
    document_cardCount: Re,
    document_cards: Te,
    document_clone: Me,
    document_currentSchemaVersion: Oe,
    document_equals: Ue,
    document_formatDiagnostic: De,
    document_formatRules: Ne,
    document_fromJson: ze,
    document_fromMarkdown: Be,
    document_insertCard: Ve,
    document_main: Je,
    document_makeCard: $e,
    document_moveCard: Le,
    document_new: We,
    document_pushCard: Qe,
    document_quillRef: He,
    document_quillRefHint: Ye,
    document_removeCard: Ke,
    document_removeCardExt: Ge,
    document_removeCardExtNamespace: Pe,
    document_removeCardField: Xe,
    document_removeExt: Ze,
    document_removeExtNamespace: t_,
    document_removeField: e_,
    document_removeSeedNamespace: __,
    document_replaceBody: n_,
    document_schemaVersionOf: r_,
    document_setCardExt: o_,
    document_setCardExtNamespace: i_,
    document_setCardKind: a_,
    document_setExt: d_,
    document_setExtNamespace: s_,
    document_setField: c_,
    document_setFields: g_,
    document_setFill: b_,
    document_setQuillRef: u_,
    document_setSeedNamespace: w_,
    document_toJson: f_,
    document_toMarkdown: p_,
    document_tryFromJson: l_,
    document_updateCardBody: m_,
    document_updateCardField: y_,
    document_updateCardFields: v_,
    document_warnings: h_,
    init: I_,
    livesession_apply: k_,
    livesession_backendId: x_,
    livesession_fieldAt: C_,
    livesession_locate: F_,
    livesession_pageCount: q_,
    livesession_pageSize: S_,
    livesession_paint: E_,
    livesession_positionAt: A_,
    livesession_regions: j_,
    livesession_render: R_,
    livesession_supportsCanvas: T_,
    livesession_warnings: M_,
    memory: Fe,
    quill_backendId: O_,
    quill_blueprint: U_,
    quill_fromTree: D_,
    quill_metadata: N_,
    quill_schema: z_,
    quill_seedCard: B_,
    quill_seedDocument: V_,
    quill_seedMain: J_,
    quill_toTree: $_,
    quill_validate: L_,
    quillmark_new: W_,
    quillmark_open: Q_,
    quillmark_render: H_,
    quillmark_supportedFormats: Y_,
    quillmark_supportsCanvas: K_
  }, Symbol.toStringTag, {
    value: "Module"
  }));
  xe(en);
  B();
});
export {
  l as Document,
  q as LiveSession,
  h as Quill,
  U as Quillmark,
  __tla,
  nn as init
};
