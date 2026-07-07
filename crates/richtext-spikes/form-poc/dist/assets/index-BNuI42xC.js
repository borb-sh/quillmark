var __typeError = (msg) => {
  throw TypeError(msg);
};
var __accessCheck = (obj, member, msg) => member.has(obj) || __typeError("Cannot " + msg);
var __privateGet = (obj, member, getter) => (__accessCheck(obj, member, "read from private field"), getter ? getter.call(obj) : member.get(obj));
var __privateAdd = (obj, member, value) => member.has(obj) ? __typeError("Cannot add the same private member more than once") : member instanceof WeakSet ? member.add(obj) : member.set(obj, value);
var __privateSet = (obj, member, value, setter) => (__accessCheck(obj, member, "write to private field"), setter ? setter.call(obj, value) : member.set(obj, value), value);
var __privateMethod = (obj, member, method) => (__accessCheck(obj, member, "access private method"), method);
let po;
let __tla = (async () => {
  var _e2, _t2, _n2, _r2, _Wa_instances, i_fn, o_fn, l_fn, s_fn, _e3, _t3;
  (function() {
    const e = document.createElement("link").relList;
    if (e && e.supports && e.supports("modulepreload")) return;
    for (const i of document.querySelectorAll('link[rel="modulepreload"]')) n(i);
    new MutationObserver((i) => {
      for (const s of i) if (s.type === "childList") for (const o of s.addedNodes) o.tagName === "LINK" && o.rel === "modulepreload" && n(o);
    }).observe(document, {
      childList: true,
      subtree: true
    });
    function t(i) {
      const s = {};
      return i.integrity && (s.integrity = i.integrity), i.referrerPolicy && (s.referrerPolicy = i.referrerPolicy), i.crossOrigin === "use-credentials" ? s.credentials = "include" : i.crossOrigin === "anonymous" ? s.credentials = "omit" : s.credentials = "same-origin", s;
    }
    function n(i) {
      if (i.ep) return;
      i.ep = true;
      const s = t(i);
      fetch(i.href, s);
    }
  })();
  let fo, uo, br, A, ho;
  fo = "modulepreload";
  uo = function(r) {
    return "/" + r;
  };
  br = {};
  A = function(e, t, n) {
    let i = Promise.resolve();
    if (t && t.length > 0) {
      let o = function(c) {
        return Promise.all(c.map((f) => Promise.resolve(f).then((d) => ({
          status: "fulfilled",
          value: d
        }), (d) => ({
          status: "rejected",
          reason: d
        }))));
      };
      document.getElementsByTagName("link");
      const l = document.querySelector("meta[property=csp-nonce]"), a = (l == null ? void 0 : l.nonce) || (l == null ? void 0 : l.getAttribute("nonce"));
      i = o(t.map((c) => {
        if (c = uo(c), c in br) return;
        br[c] = true;
        const f = c.endsWith(".css"), d = f ? '[rel="stylesheet"]' : "";
        if (document.querySelector(`link[href="${c}"]${d}`)) return;
        const u = document.createElement("link");
        if (u.rel = f ? "stylesheet" : fo, f || (u.as = "script"), u.crossOrigin = "", u.href = c, a && u.setAttribute("nonce", a), document.head.appendChild(u), f) return new Promise((m, p) => {
          u.addEventListener("load", m), u.addEventListener("error", () => p(new Error(`Unable to preload CSS for ${c}`)));
        });
      }));
    }
    function s(o) {
      const l = new Event("vite:preloadError", {
        cancelable: true
      });
      if (l.payload = o, window.dispatchEvent(l), !l.defaultPrevented) throw o;
    }
    return i.then((o) => {
      for (const l of o || []) l.status === "rejected" && s(l.reason);
      return e().catch(s);
    });
  };
  ho = "/assets/wasm_bg-BH8-0AJ-.wasm";
  po = async (r = {}, e) => {
    let t;
    if (e.startsWith("data:")) {
      const n = e.replace(/^data:.*?base64,/, "");
      let i;
      if (typeof Buffer == "function" && typeof Buffer.from == "function") i = Buffer.from(n, "base64");
      else if (typeof atob == "function") {
        const s = atob(n);
        i = new Uint8Array(s.length);
        for (let o = 0; o < s.length; o++) i[o] = s.charCodeAt(o);
      } else throw new Error("Cannot decode base64-encoded data URL");
      t = await WebAssembly.instantiate(i, r);
    } else {
      const n = await fetch(e), i = n.headers.get("Content-Type") || "";
      if ("instantiateStreaming" in WebAssembly && i.startsWith("application/wasm")) t = await WebAssembly.instantiateStreaming(n, r);
      else {
        const s = await n.arrayBuffer();
        t = await WebAssembly.instantiate(s, r);
      }
    }
    return t.instance.exports;
  };
  class le {
    static __wrap(e) {
      e = e >>> 0;
      const t = Object.create(le.prototype);
      return t.__wbg_ptr = e, hn.register(t, t.__wbg_ptr, t), t;
    }
    __destroy_into_raw() {
      const e = this.__wbg_ptr;
      return this.__wbg_ptr = 0, hn.unregister(this), e;
    }
    free() {
      const e = this.__destroy_into_raw();
      h.__wbg_document_free(e, 0);
    }
    static blueprintInstruction(e) {
      let t, n;
      try {
        const o = h.__wbindgen_add_to_stack_pointer(-16), l = D(e, h.__wbindgen_export, h.__wbindgen_export2), a = N;
        h.document_blueprintInstruction(o, l, a);
        var i = g().getInt32(o + 0, true), s = g().getInt32(o + 4, true);
        return t = i, n = s, j(i, s);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16), h.__wbindgen_export4(t, n, 1);
      }
    }
    get cardCount() {
      return h.document_cardCount(this.__wbg_ptr) >>> 0;
    }
    get cards() {
      try {
        const i = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_cards(i, this.__wbg_ptr);
        var e = g().getInt32(i + 0, true), t = g().getInt32(i + 4, true), n = g().getInt32(i + 8, true);
        if (n) throw k(t);
        return k(e);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    clone() {
      const e = h.document_clone(this.__wbg_ptr);
      return le.__wrap(e);
    }
    static currentSchemaVersion() {
      let e, t;
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_currentSchemaVersion(s);
        var n = g().getInt32(s + 0, true), i = g().getInt32(s + 4, true);
        return e = n, t = i, j(n, i);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16), h.__wbindgen_export4(e, t, 1);
      }
    }
    equals(e) {
      return Di(e, le), h.document_equals(this.__wbg_ptr, e.__wbg_ptr) !== 0;
    }
    static formatDiagnostic(e) {
      let t, n;
      try {
        const o = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_formatDiagnostic(o, C(e));
        var i = g().getInt32(o + 0, true), s = g().getInt32(o + 4, true);
        return t = i, n = s, j(i, s);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16), h.__wbindgen_export4(t, n, 1);
      }
    }
    static formatRules() {
      let e, t;
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_formatRules(s);
        var n = g().getInt32(s + 0, true), i = g().getInt32(s + 4, true);
        return e = n, t = i, j(n, i);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16), h.__wbindgen_export4(e, t, 1);
      }
    }
    static fromJson(e) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16), o = D(e, h.__wbindgen_export, h.__wbindgen_export2), l = N;
        h.document_fromJson(s, o, l);
        var t = g().getInt32(s + 0, true), n = g().getInt32(s + 4, true), i = g().getInt32(s + 8, true);
        if (i) throw k(n);
        return le.__wrap(t);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    static fromMarkdown(e) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16), o = D(e, h.__wbindgen_export, h.__wbindgen_export2), l = N;
        h.document_fromMarkdown(s, o, l);
        var t = g().getInt32(s + 0, true), n = g().getInt32(s + 4, true), i = g().getInt32(s + 8, true);
        if (i) throw k(n);
        return le.__wrap(t);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    insertCard(e, t) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_insertCard(s, this.__wbg_ptr, e, C(t));
        var n = g().getInt32(s + 0, true), i = g().getInt32(s + 4, true);
        if (i) throw k(n);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    get main() {
      try {
        const i = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_main(i, this.__wbg_ptr);
        var e = g().getInt32(i + 0, true), t = g().getInt32(i + 4, true), n = g().getInt32(i + 8, true);
        if (n) throw k(t);
        return k(e);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    static makeCard(e, t, n) {
      try {
        const c = h.__wbindgen_add_to_stack_pointer(-16), f = D(e, h.__wbindgen_export, h.__wbindgen_export2), d = N;
        var i = Re(n) ? 0 : D(n, h.__wbindgen_export, h.__wbindgen_export2), s = N;
        h.document_makeCard(c, f, d, Re(t) ? 0 : C(t), i, s);
        var o = g().getInt32(c + 0, true), l = g().getInt32(c + 4, true), a = g().getInt32(c + 8, true);
        if (a) throw k(l);
        return k(o);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    moveCard(e, t) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_moveCard(s, this.__wbg_ptr, e, t);
        var n = g().getInt32(s + 0, true), i = g().getInt32(s + 4, true);
        if (i) throw k(n);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    constructor(e) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16), o = D(e, h.__wbindgen_export, h.__wbindgen_export2), l = N;
        h.document_new(s, o, l);
        var t = g().getInt32(s + 0, true), n = g().getInt32(s + 4, true), i = g().getInt32(s + 8, true);
        if (i) throw k(n);
        return this.__wbg_ptr = t >>> 0, hn.register(this, this.__wbg_ptr, this), this;
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    pushCard(e) {
      try {
        const i = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_pushCard(i, this.__wbg_ptr, C(e));
        var t = g().getInt32(i + 0, true), n = g().getInt32(i + 4, true);
        if (n) throw k(t);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    get quillRef() {
      let e, t;
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_quillRef(s, this.__wbg_ptr);
        var n = g().getInt32(s + 0, true), i = g().getInt32(s + 4, true);
        return e = n, t = i, j(n, i);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16), h.__wbindgen_export4(e, t, 1);
      }
    }
    static quillRefHint() {
      let e, t;
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_quillRefHint(s);
        var n = g().getInt32(s + 0, true), i = g().getInt32(s + 4, true);
        return e = n, t = i, j(n, i);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16), h.__wbindgen_export4(e, t, 1);
      }
    }
    removeCard(e) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_removeCard(s, this.__wbg_ptr, e);
        var t = g().getInt32(s + 0, true), n = g().getInt32(s + 4, true), i = g().getInt32(s + 8, true);
        if (i) throw k(n);
        return k(t);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    removeCardExt(e) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_removeCardExt(s, this.__wbg_ptr, e);
        var t = g().getInt32(s + 0, true), n = g().getInt32(s + 4, true), i = g().getInt32(s + 8, true);
        if (i) throw k(n);
        return k(t);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    removeCardExtNamespace(e, t) {
      try {
        const o = h.__wbindgen_add_to_stack_pointer(-16), l = D(t, h.__wbindgen_export, h.__wbindgen_export2), a = N;
        h.document_removeCardExtNamespace(o, this.__wbg_ptr, e, l, a);
        var n = g().getInt32(o + 0, true), i = g().getInt32(o + 4, true), s = g().getInt32(o + 8, true);
        if (s) throw k(i);
        return k(n);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    removeCardField(e, t) {
      try {
        const o = h.__wbindgen_add_to_stack_pointer(-16), l = D(t, h.__wbindgen_export, h.__wbindgen_export2), a = N;
        h.document_removeCardField(o, this.__wbg_ptr, e, l, a);
        var n = g().getInt32(o + 0, true), i = g().getInt32(o + 4, true), s = g().getInt32(o + 8, true);
        if (s) throw k(i);
        return k(n);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    removeExt() {
      try {
        const i = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_removeExt(i, this.__wbg_ptr);
        var e = g().getInt32(i + 0, true), t = g().getInt32(i + 4, true), n = g().getInt32(i + 8, true);
        if (n) throw k(t);
        return k(e);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    removeExtNamespace(e) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16), o = D(e, h.__wbindgen_export, h.__wbindgen_export2), l = N;
        h.document_removeExtNamespace(s, this.__wbg_ptr, o, l);
        var t = g().getInt32(s + 0, true), n = g().getInt32(s + 4, true), i = g().getInt32(s + 8, true);
        if (i) throw k(n);
        return k(t);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    removeField(e) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16), o = D(e, h.__wbindgen_export, h.__wbindgen_export2), l = N;
        h.document_removeField(s, this.__wbg_ptr, o, l);
        var t = g().getInt32(s + 0, true), n = g().getInt32(s + 4, true), i = g().getInt32(s + 8, true);
        if (i) throw k(n);
        return k(t);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    removeSeedNamespace(e) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16), o = D(e, h.__wbindgen_export, h.__wbindgen_export2), l = N;
        h.document_removeSeedNamespace(s, this.__wbg_ptr, o, l);
        var t = g().getInt32(s + 0, true), n = g().getInt32(s + 4, true), i = g().getInt32(s + 8, true);
        if (i) throw k(n);
        return k(t);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    replaceBody(e) {
      const t = D(e, h.__wbindgen_export, h.__wbindgen_export2), n = N;
      h.document_replaceBody(this.__wbg_ptr, t, n);
    }
    static schemaVersionOf(e) {
      try {
        const i = h.__wbindgen_add_to_stack_pointer(-16), s = D(e, h.__wbindgen_export, h.__wbindgen_export2), o = N;
        h.document_schemaVersionOf(i, s, o);
        var t = g().getInt32(i + 0, true), n = g().getInt32(i + 4, true);
        let l;
        return t !== 0 && (l = j(t, n).slice(), h.__wbindgen_export4(t, n * 1, 1)), l;
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setCardExt(e, t) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_setCardExt(s, this.__wbg_ptr, e, C(t));
        var n = g().getInt32(s + 0, true), i = g().getInt32(s + 4, true);
        if (i) throw k(n);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setCardExtNamespace(e, t, n) {
      try {
        const o = h.__wbindgen_add_to_stack_pointer(-16), l = D(t, h.__wbindgen_export, h.__wbindgen_export2), a = N;
        h.document_setCardExtNamespace(o, this.__wbg_ptr, e, l, a, C(n));
        var i = g().getInt32(o + 0, true), s = g().getInt32(o + 4, true);
        if (s) throw k(i);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setCardKind(e, t) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16), o = D(t, h.__wbindgen_export, h.__wbindgen_export2), l = N;
        h.document_setCardKind(s, this.__wbg_ptr, e, o, l);
        var n = g().getInt32(s + 0, true), i = g().getInt32(s + 4, true);
        if (i) throw k(n);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setExt(e) {
      try {
        const i = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_setExt(i, this.__wbg_ptr, C(e));
        var t = g().getInt32(i + 0, true), n = g().getInt32(i + 4, true);
        if (n) throw k(t);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setExtNamespace(e, t) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16), o = D(e, h.__wbindgen_export, h.__wbindgen_export2), l = N;
        h.document_setExtNamespace(s, this.__wbg_ptr, o, l, C(t));
        var n = g().getInt32(s + 0, true), i = g().getInt32(s + 4, true);
        if (i) throw k(n);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setField(e, t) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16), o = D(e, h.__wbindgen_export, h.__wbindgen_export2), l = N;
        h.document_setField(s, this.__wbg_ptr, o, l, C(t));
        var n = g().getInt32(s + 0, true), i = g().getInt32(s + 4, true);
        if (i) throw k(n);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setFields(e) {
      try {
        const i = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_setFields(i, this.__wbg_ptr, C(e));
        var t = g().getInt32(i + 0, true), n = g().getInt32(i + 4, true);
        if (n) throw k(t);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setFill(e, t) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16), o = D(e, h.__wbindgen_export, h.__wbindgen_export2), l = N;
        h.document_setFill(s, this.__wbg_ptr, o, l, C(t));
        var n = g().getInt32(s + 0, true), i = g().getInt32(s + 4, true);
        if (i) throw k(n);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setQuillRef(e) {
      try {
        const i = h.__wbindgen_add_to_stack_pointer(-16), s = D(e, h.__wbindgen_export, h.__wbindgen_export2), o = N;
        h.document_setQuillRef(i, this.__wbg_ptr, s, o);
        var t = g().getInt32(i + 0, true), n = g().getInt32(i + 4, true);
        if (n) throw k(t);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    setSeedNamespace(e, t) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16), o = D(e, h.__wbindgen_export, h.__wbindgen_export2), l = N;
        h.document_setSeedNamespace(s, this.__wbg_ptr, o, l, C(t));
        var n = g().getInt32(s + 0, true), i = g().getInt32(s + 4, true);
        if (i) throw k(n);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    toJson() {
      let e, t;
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_toJson(s, this.__wbg_ptr);
        var n = g().getInt32(s + 0, true), i = g().getInt32(s + 4, true);
        return e = n, t = i, j(n, i);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16), h.__wbindgen_export4(e, t, 1);
      }
    }
    toMarkdown() {
      let e, t;
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_toMarkdown(s, this.__wbg_ptr);
        var n = g().getInt32(s + 0, true), i = g().getInt32(s + 4, true);
        return e = n, t = i, j(n, i);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16), h.__wbindgen_export4(e, t, 1);
      }
    }
    static tryFromJson(e) {
      const t = D(e, h.__wbindgen_export, h.__wbindgen_export2), n = N, i = h.document_tryFromJson(t, n);
      return i === 0 ? void 0 : le.__wrap(i);
    }
    updateCardBody(e, t) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16), o = D(t, h.__wbindgen_export, h.__wbindgen_export2), l = N;
        h.document_updateCardBody(s, this.__wbg_ptr, e, o, l);
        var n = g().getInt32(s + 0, true), i = g().getInt32(s + 4, true);
        if (i) throw k(n);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    updateCardField(e, t, n) {
      try {
        const o = h.__wbindgen_add_to_stack_pointer(-16), l = D(t, h.__wbindgen_export, h.__wbindgen_export2), a = N;
        h.document_updateCardField(o, this.__wbg_ptr, e, l, a, C(n));
        var i = g().getInt32(o + 0, true), s = g().getInt32(o + 4, true);
        if (s) throw k(i);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    updateCardFields(e, t) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_updateCardFields(s, this.__wbg_ptr, e, C(t));
        var n = g().getInt32(s + 0, true), i = g().getInt32(s + 4, true);
        if (i) throw k(n);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    get warnings() {
      try {
        const i = h.__wbindgen_add_to_stack_pointer(-16);
        h.document_warnings(i, this.__wbg_ptr);
        var e = g().getInt32(i + 0, true), t = g().getInt32(i + 4, true), n = g().getInt32(i + 8, true);
        if (n) throw k(t);
        return k(e);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
  }
  Symbol.dispose && (le.prototype[Symbol.dispose] = le.prototype.free);
  class rt {
    static __wrap(e) {
      e = e >>> 0;
      const t = Object.create(rt.prototype);
      return t.__wbg_ptr = e, wr.register(t, t.__wbg_ptr, t), t;
    }
    __destroy_into_raw() {
      const e = this.__wbg_ptr;
      return this.__wbg_ptr = 0, wr.unregister(this), e;
    }
    free() {
      const e = this.__destroy_into_raw();
      h.__wbg_quill_free(e, 0);
    }
    get backendId() {
      let e, t;
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16);
        h.quill_backendId(s, this.__wbg_ptr);
        var n = g().getInt32(s + 0, true), i = g().getInt32(s + 4, true);
        return e = n, t = i, j(n, i);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16), h.__wbindgen_export4(e, t, 1);
      }
    }
    get blueprint() {
      let e, t;
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16);
        h.quill_blueprint(s, this.__wbg_ptr);
        var n = g().getInt32(s + 0, true), i = g().getInt32(s + 4, true);
        return e = n, t = i, j(n, i);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16), h.__wbindgen_export4(e, t, 1);
      }
    }
    static fromTree(e) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16);
        h.quill_fromTree(s, C(e));
        var t = g().getInt32(s + 0, true), n = g().getInt32(s + 4, true), i = g().getInt32(s + 8, true);
        if (i) throw k(n);
        return rt.__wrap(t);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    get metadata() {
      try {
        const i = h.__wbindgen_add_to_stack_pointer(-16);
        h.quill_metadata(i, this.__wbg_ptr);
        var e = g().getInt32(i + 0, true), t = g().getInt32(i + 4, true), n = g().getInt32(i + 8, true);
        if (n) throw k(t);
        return k(e);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    get schema() {
      try {
        const i = h.__wbindgen_add_to_stack_pointer(-16);
        h.quill_schema(i, this.__wbg_ptr);
        var e = g().getInt32(i + 0, true), t = g().getInt32(i + 4, true), n = g().getInt32(i + 8, true);
        if (n) throw k(t);
        return k(e);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    seedCard(e, t) {
      try {
        const o = h.__wbindgen_add_to_stack_pointer(-16), l = D(e, h.__wbindgen_export, h.__wbindgen_export2), a = N;
        h.quill_seedCard(o, this.__wbg_ptr, l, a, C(t));
        var n = g().getInt32(o + 0, true), i = g().getInt32(o + 4, true), s = g().getInt32(o + 8, true);
        if (s) throw k(i);
        return k(n);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    seedDocument() {
      const e = h.quill_seedDocument(this.__wbg_ptr);
      return le.__wrap(e);
    }
    seedMain() {
      try {
        const i = h.__wbindgen_add_to_stack_pointer(-16);
        h.quill_seedMain(i, this.__wbg_ptr);
        var e = g().getInt32(i + 0, true), t = g().getInt32(i + 4, true), n = g().getInt32(i + 8, true);
        if (n) throw k(t);
        return k(e);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
    toTree() {
      const e = h.quill_toTree(this.__wbg_ptr);
      return k(e);
    }
    validate(e) {
      try {
        const s = h.__wbindgen_add_to_stack_pointer(-16);
        Di(e, le), h.quill_validate(s, this.__wbg_ptr, e.__wbg_ptr);
        var t = g().getInt32(s + 0, true), n = g().getInt32(s + 4, true), i = g().getInt32(s + 8, true);
        if (i) throw k(n);
        return k(t);
      } finally {
        h.__wbindgen_add_to_stack_pointer(16);
      }
    }
  }
  Symbol.dispose && (rt.prototype[Symbol.dispose] = rt.prototype.free);
  function mo() {
    h.init();
  }
  function go(r, e) {
    const t = Error(j(r, e));
    return C(t);
  }
  function _o(r, e) {
    const t = String(x(e)), n = D(t, h.__wbindgen_export, h.__wbindgen_export2), i = N;
    g().setInt32(r + 4, i, true), g().setInt32(r + 0, n, true);
  }
  function yo(r, e) {
    const t = String(x(e)), n = D(t, h.__wbindgen_export, h.__wbindgen_export2), i = N;
    g().setInt32(r + 4, i, true), g().setInt32(r + 0, n, true);
  }
  function bo(r, e) {
    const t = x(e), n = typeof t == "bigint" ? t : void 0;
    g().setBigInt64(r + 8, Re(n) ? BigInt(0) : n, true), g().setInt32(r + 0, !Re(n), true);
  }
  function wo(r) {
    const e = x(r), t = typeof e == "boolean" ? e : void 0;
    return Re(t) ? 16777215 : t ? 1 : 0;
  }
  function ko(r, e) {
    const t = zn(x(e)), n = D(t, h.__wbindgen_export, h.__wbindgen_export2), i = N;
    g().setInt32(r + 4, i, true), g().setInt32(r + 0, n, true);
  }
  function xo(r, e) {
    return x(r) in x(e);
  }
  function So(r) {
    return typeof x(r) == "bigint";
  }
  function Co(r) {
    return typeof x(r) == "function";
  }
  function Oo(r) {
    return x(r) === null;
  }
  function Mo(r) {
    const e = x(r);
    return typeof e == "object" && e !== null;
  }
  function No(r) {
    return typeof x(r) == "string";
  }
  function Eo(r) {
    return x(r) === void 0;
  }
  function To(r, e) {
    return x(r) === x(e);
  }
  function Do(r, e) {
    return x(r) == x(e);
  }
  function Io(r, e) {
    const t = x(e), n = typeof t == "number" ? t : void 0;
    g().setFloat64(r + 8, Re(n) ? 0 : n, true), g().setInt32(r + 0, !Re(n), true);
  }
  function Ao(r, e) {
    const t = x(e), n = typeof t == "string" ? t : void 0;
    var i = Re(n) ? 0 : D(n, h.__wbindgen_export, h.__wbindgen_export2), s = N;
    g().setInt32(r + 4, s, true), g().setInt32(r + 0, i, true);
  }
  function vo(r, e) {
    throw new Error(j(r, e));
  }
  function Ro() {
    return It(function(r, e) {
      const t = x(r).call(x(e));
      return C(t);
    }, arguments);
  }
  function Po(r) {
    return x(r).done;
  }
  function Bo(r) {
    const e = x(r).entries();
    return C(e);
  }
  function zo(r) {
    const e = Object.entries(x(r));
    return C(e);
  }
  function Fo(r, e) {
    let t, n;
    try {
      t = r, n = e, console.error(j(r, e));
    } finally {
      h.__wbindgen_export4(t, n, 1);
    }
  }
  function Vo(r) {
    const e = Array.from(x(r));
    return C(e);
  }
  function Lo() {
    return It(function(r, e) {
      globalThis.crypto.getRandomValues(tr(r, e));
    }, arguments);
  }
  function qo() {
    return It(function(r, e) {
      const t = Reflect.get(x(r), x(e));
      return C(t);
    }, arguments);
  }
  function Wo(r, e) {
    const t = x(r)[e >>> 0];
    return C(t);
  }
  function Jo(r, e) {
    const t = x(r)[e >>> 0];
    return C(t);
  }
  function $o(r, e) {
    const t = x(r)[x(e)];
    return C(t);
  }
  function jo(r, e) {
    const t = x(r)[x(e)];
    return C(t);
  }
  function Ko(r) {
    let e;
    try {
      e = x(r) instanceof ArrayBuffer;
    } catch {
      e = false;
    }
    return e;
  }
  function Ho(r) {
    let e;
    try {
      e = x(r) instanceof Map;
    } catch {
      e = false;
    }
    return e;
  }
  function Uo(r) {
    let e;
    try {
      e = x(r) instanceof Object;
    } catch {
      e = false;
    }
    return e;
  }
  function Go(r) {
    let e;
    try {
      e = x(r) instanceof Uint8Array;
    } catch {
      e = false;
    }
    return e;
  }
  function Yo(r) {
    return Array.isArray(x(r));
  }
  function Qo(r) {
    return Number.isSafeInteger(x(r));
  }
  function Xo() {
    return C(Symbol.iterator);
  }
  function Zo(r) {
    const e = Object.keys(x(r));
    return C(e);
  }
  function el(r) {
    return x(r).length;
  }
  function tl(r) {
    return x(r).length;
  }
  function nl(r) {
    const e = new Uint8Array(x(r));
    return C(e);
  }
  function rl() {
    const r = new Error();
    return C(r);
  }
  function il() {
    return C(/* @__PURE__ */ new Map());
  }
  function sl(r, e) {
    const t = new Error(j(r, e));
    return C(t);
  }
  function ol() {
    const r = new Array();
    return C(r);
  }
  function ll() {
    const r = new Object();
    return C(r);
  }
  function al(r, e) {
    const t = new Uint8Array(tr(r, e));
    return C(t);
  }
  function cl() {
    return It(function(r) {
      const e = x(r).next();
      return C(e);
    }, arguments);
  }
  function fl(r) {
    const e = x(r).next;
    return C(e);
  }
  function dl(r, e, t) {
    Uint8Array.prototype.set.call(tr(r, e), x(t));
  }
  function ul() {
    return It(function(r, e, t) {
      return Reflect.set(x(r), x(e), x(t));
    }, arguments);
  }
  function hl(r, e, t) {
    x(r)[e >>> 0] = k(t);
  }
  function pl(r, e, t) {
    x(r)[k(e)] = k(t);
  }
  function ml(r, e, t) {
    const n = x(r).set(x(e), x(t));
    return C(n);
  }
  function gl(r, e) {
    const t = x(e).stack, n = D(t, h.__wbindgen_export, h.__wbindgen_export2), i = N;
    g().setInt32(r + 4, i, true), g().setInt32(r + 0, n, true);
  }
  function _l(r) {
    const e = x(r).value;
    return C(e);
  }
  function yl(r) {
    return C(r);
  }
  function bl(r) {
    return C(r);
  }
  function wl(r, e) {
    const t = j(r, e);
    return C(t);
  }
  function kl(r) {
    const e = BigInt.asUintN(64, r);
    return C(e);
  }
  function xl(r) {
    const e = x(r);
    return C(e);
  }
  function Sl(r) {
    k(r);
  }
  const hn = typeof FinalizationRegistry > "u" ? {
    register: () => {
    },
    unregister: () => {
    }
  } : new FinalizationRegistry((r) => h.__wbg_document_free(r >>> 0, 1)), wr = typeof FinalizationRegistry > "u" ? {
    register: () => {
    },
    unregister: () => {
    }
  } : new FinalizationRegistry((r) => h.__wbg_quill_free(r >>> 0, 1));
  function C(r) {
    mt === we.length && we.push(we.length + 1);
    const e = mt;
    return mt = we[e], we[e] = r, e;
  }
  function Di(r, e) {
    if (!(r instanceof e)) throw new Error(`expected instance of ${e.name}`);
  }
  function zn(r) {
    const e = typeof r;
    if (e == "number" || e == "boolean" || r == null) return `${r}`;
    if (e == "string") return `"${r}"`;
    if (e == "symbol") {
      const i = r.description;
      return i == null ? "Symbol" : `Symbol(${i})`;
    }
    if (e == "function") {
      const i = r.name;
      return typeof i == "string" && i.length > 0 ? `Function(${i})` : "Function";
    }
    if (Array.isArray(r)) {
      const i = r.length;
      let s = "[";
      i > 0 && (s += zn(r[0]));
      for (let o = 1; o < i; o++) s += ", " + zn(r[o]);
      return s += "]", s;
    }
    const t = /\[object ([^\]]+)\]/.exec(toString.call(r));
    let n;
    if (t && t.length > 1) n = t[1];
    else return toString.call(r);
    if (n == "Object") try {
      return "Object(" + JSON.stringify(r) + ")";
    } catch {
      return "Object";
    }
    return r instanceof Error ? `${r.name}: ${r.message}
${r.stack}` : n;
  }
  function Cl(r) {
    r < 1028 || (we[r] = mt, mt = r);
  }
  function tr(r, e) {
    return r = r >>> 0, pt().subarray(r / 1, r / 1 + e);
  }
  let Qe = null;
  function g() {
    return (Qe === null || Qe.buffer.detached === true || Qe.buffer.detached === void 0 && Qe.buffer !== h.memory.buffer) && (Qe = new DataView(h.memory.buffer)), Qe;
  }
  function j(r, e) {
    return r = r >>> 0, Ml(r, e);
  }
  let Ft = null;
  function pt() {
    return (Ft === null || Ft.byteLength === 0) && (Ft = new Uint8Array(h.memory.buffer)), Ft;
  }
  function x(r) {
    return we[r];
  }
  function It(r, e) {
    try {
      return r.apply(this, e);
    } catch (t) {
      h.__wbindgen_export3(C(t));
    }
  }
  let we = new Array(1024).fill(void 0);
  we.push(void 0, null, true, false);
  let mt = we.length;
  function Re(r) {
    return r == null;
  }
  function D(r, e, t) {
    if (t === void 0) {
      const l = gt.encode(r), a = e(l.length, 1) >>> 0;
      return pt().subarray(a, a + l.length).set(l), N = l.length, a;
    }
    let n = r.length, i = e(n, 1) >>> 0;
    const s = pt();
    let o = 0;
    for (; o < n; o++) {
      const l = r.charCodeAt(o);
      if (l > 127) break;
      s[i + o] = l;
    }
    if (o !== n) {
      o !== 0 && (r = r.slice(o)), i = t(i, n, n = o + r.length * 3, 1) >>> 0;
      const l = pt().subarray(i + o, i + n), a = gt.encodeInto(r, l);
      o += a.written, i = t(i, n, o, 1) >>> 0;
    }
    return N = o, i;
  }
  function k(r) {
    const e = x(r);
    return Cl(r), e;
  }
  let $t = new TextDecoder("utf-8", {
    ignoreBOM: true,
    fatal: true
  });
  $t.decode();
  const Ol = 2146435072;
  let pn = 0;
  function Ml(r, e) {
    return pn += e, pn >= Ol && ($t = new TextDecoder("utf-8", {
      ignoreBOM: true,
      fatal: true
    }), $t.decode(), pn = e), $t.decode(pt().subarray(r, r + e));
  }
  const gt = new TextEncoder();
  "encodeInto" in gt || (gt.encodeInto = function(r, e) {
    const t = gt.encode(r);
    return e.set(t), {
      read: r.length,
      written: t.length
    };
  });
  let N = 0, h;
  function Nl(r) {
    h = r;
  }
  URL = globalThis.URL;
  const El = await po({
    "./wasm_bg.js": {
      __wbindgen_object_clone_ref: xl,
      __wbindgen_object_drop_ref: Sl,
      __wbg_get_unchecked_17f53dad852b9588: Jo,
      __wbg_set_3bf1de9fab0cd644: hl,
      __wbg_length_3d4ecd04bd8d22f1: el,
      __wbg_set_fde2cec06c23692b: ml,
      __wbg_entries_2bf997cf82353e47: Bo,
      __wbg_next_0340c4ae324393c3: cl,
      __wbg_instanceof_Object_7c99480a1cdfb911: Uo,
      __wbg_instanceof_Map_1b76fd4635be43eb: Ho,
      __wbg_done_9158f7cc8751ba32: Po,
      __wbg_value_ee3a06f4579184fa: _l,
      __wbg_keys_2fd1bfdda7e278ca: Zo,
      __wbg_new_227d7c05414eb861: rl,
      __wbg_stack_3b0d974bbf31e44f: gl,
      __wbg_error_a6fa202b58aa1cd3: Fo,
      __wbg_get_with_ref_key_6412cf3094599694: $o,
      __wbg_set_6be42768c690e380: pl,
      __wbg_get_8360291721e2339f: Wo,
      __wbg_String_8564e559799eccda: _o,
      __wbg_get_with_ref_key_f64427178466f623: jo,
      __wbg_String_b51de6b05a10845b: yo,
      __wbg_getRandomValues_3f44b700395062e5: Lo,
      __wbg_new_from_slice_b5ea43e23f6008c0: al,
      __wbg_new_0c7403db6e782f19: nl,
      __wbg_length_9f1775224cf1d815: tl,
      __wbg_prototypesetcall_a6b02eb00b0f4ce2: dl,
      __wbg_call_14b169f759b26747: Ro,
      __wbg_instanceof_Uint8Array_152ba1f289edcf3f: Go,
      __wbg_instanceof_ArrayBuffer_7c8433c6ed14ffe3: Ko,
      __wbg_new_34d45cc8e36aaead: il,
      __wbg_new_682678e2f47e32bc: ol,
      __wbg_from_0dbf29f09e7fb200: Vo,
      __wbg_isArray_c3109d14ffc06469: Yo,
      __wbg_new_5e360d2ff7b9e1c3: sl,
      __wbg_isSafeInteger_4fc213d1989d6d2a: Qo,
      __wbg_new_aa8d0fa9762c29bd: ll,
      __wbg_entries_e0b73aa8571ddb56: zo,
      __wbg_iterator_013bc09ec998c2a7: Xo,
      __wbg_get_1affdbdd5573b16a: qo,
      __wbg_set_022bee52d0b05b19: ul,
      __wbg_next_7646edaa39458ef7: fl,
      __wbg___wbindgen_in_a5d8b22e52b24dd1: xo,
      __wbg___wbindgen_throw_6b64449b9b9ed33c: vo,
      __wbg___wbindgen_is_null_52ff4ec04186736f: Oo,
      __wbg___wbindgen_jsval_eq_d3465d8a07697228: To,
      __wbg_Error_960c155d3d49e4c2: go,
      __wbg___wbindgen_is_bigint_ec25c7f91b4d9e93: So,
      __wbg___wbindgen_is_object_63322ec0cd6ea4ef: Mo,
      __wbg___wbindgen_is_string_6df3bf7ef1164ed3: No,
      __wbg___wbindgen_number_get_c7f42aed0525c451: Io,
      __wbg___wbindgen_string_get_7ed5322991caaec5: Ao,
      __wbg___wbindgen_boolean_get_6ea149f0a8dcc5ff: wo,
      __wbg___wbindgen_is_function_3baa9db1a987f47d: Co,
      __wbg___wbindgen_is_undefined_29a43b4d42920abd: Eo,
      __wbg___wbindgen_jsval_loose_eq_cac3565e89b4134c: Do,
      __wbg___wbindgen_bigint_get_as_i64_3d3aba5d616c6a51: bo,
      __wbg___wbindgen_debug_string_ab4b34d23d6778bd: ko,
      __wbindgen_cast_0000000000000001: yl,
      __wbindgen_cast_0000000000000002: bl,
      __wbindgen_cast_0000000000000003: wl,
      __wbindgen_cast_0000000000000004: kl
    }
  }, ho), { memory: Tl, __wbg_document_free: Dl, __wbg_quill_free: Il, document_blueprintInstruction: Al, document_cardCount: vl, document_cards: Rl, document_clone: Pl, document_currentSchemaVersion: Bl, document_equals: zl, document_formatDiagnostic: Fl, document_formatRules: Vl, document_fromJson: Ll, document_fromMarkdown: ql, document_insertCard: Wl, document_main: Jl, document_makeCard: $l, document_moveCard: jl, document_new: Kl, document_pushCard: Hl, document_quillRef: Ul, document_quillRefHint: Gl, document_removeCard: Yl, document_removeCardExt: Ql, document_removeCardExtNamespace: Xl, document_removeCardField: Zl, document_removeExt: ea, document_removeExtNamespace: ta, document_removeField: na, document_removeSeedNamespace: ra, document_replaceBody: ia, document_schemaVersionOf: sa, document_setCardExt: oa, document_setCardExtNamespace: la, document_setCardKind: aa, document_setExt: ca, document_setExtNamespace: fa, document_setField: da, document_setFields: ua, document_setFill: ha, document_setQuillRef: pa, document_setSeedNamespace: ma, document_toJson: ga, document_toMarkdown: _a, document_tryFromJson: ya, document_updateCardBody: ba, document_updateCardField: wa, document_updateCardFields: ka, document_warnings: xa, init: Sa, quill_backendId: Ca, quill_blueprint: Oa, quill_fromTree: Ma, quill_metadata: Na, quill_schema: Ea, quill_seedCard: Ta, quill_seedDocument: Da, quill_seedMain: Ia, quill_toTree: Aa, quill_validate: va, __wbindgen_export: Ra, __wbindgen_export2: Pa, __wbindgen_export3: Ba, __wbindgen_export4: za, __wbindgen_add_to_stack_pointer: Fa, __wbindgen_start: Ii } = El, Va = Object.freeze(Object.defineProperty({
    __proto__: null,
    __wbg_document_free: Dl,
    __wbg_quill_free: Il,
    __wbindgen_add_to_stack_pointer: Fa,
    __wbindgen_export: Ra,
    __wbindgen_export2: Pa,
    __wbindgen_export3: Ba,
    __wbindgen_export4: za,
    __wbindgen_start: Ii,
    document_blueprintInstruction: Al,
    document_cardCount: vl,
    document_cards: Rl,
    document_clone: Pl,
    document_currentSchemaVersion: Bl,
    document_equals: zl,
    document_formatDiagnostic: Fl,
    document_formatRules: Vl,
    document_fromJson: Ll,
    document_fromMarkdown: ql,
    document_insertCard: Wl,
    document_main: Jl,
    document_makeCard: $l,
    document_moveCard: jl,
    document_new: Kl,
    document_pushCard: Hl,
    document_quillRef: Ul,
    document_quillRefHint: Gl,
    document_removeCard: Yl,
    document_removeCardExt: Ql,
    document_removeCardExtNamespace: Xl,
    document_removeCardField: Zl,
    document_removeExt: ea,
    document_removeExtNamespace: ta,
    document_removeField: na,
    document_removeSeedNamespace: ra,
    document_replaceBody: ia,
    document_schemaVersionOf: sa,
    document_setCardExt: oa,
    document_setCardExtNamespace: la,
    document_setCardKind: aa,
    document_setExt: ca,
    document_setExtNamespace: fa,
    document_setField: da,
    document_setFields: ua,
    document_setFill: ha,
    document_setQuillRef: pa,
    document_setSeedNamespace: ma,
    document_toJson: ga,
    document_toMarkdown: _a,
    document_tryFromJson: ya,
    document_updateCardBody: ba,
    document_updateCardField: wa,
    document_updateCardFields: ka,
    document_warnings: xa,
    init: Sa,
    memory: Tl,
    quill_backendId: Ca,
    quill_blueprint: Oa,
    quill_fromTree: Ma,
    quill_metadata: Na,
    quill_schema: Ea,
    quill_seedCard: Ta,
    quill_seedDocument: Da,
    quill_seedMain: Ia,
    quill_toTree: Aa,
    quill_validate: va
  }, Symbol.toStringTag, {
    value: "Module"
  }));
  Nl(Va);
  Ii();
  const La = {
    typst: {
      load: () => A(() => import("./wasm-D6ckvW0u.js").then(async (m) => {
        await m.__tla;
        return m;
      }), []),
      formats: [
        "pdf",
        "svg",
        "png"
      ],
      canvas: true
    },
    pdfform: {
      load: () => A(() => import("./wasm-Coz7EMiK.js").then(async (m) => {
        await m.__tla;
        return m;
      }), []),
      formats: [
        "pdf",
        "svg",
        "png"
      ],
      canvas: true
    }
  };
  function qa(r, e) {
    if (!e || typeof e != "object") throw new Error(`Engine: backend '${r}' must be a descriptor { load, formats, canvas }.`);
    const { load: t, formats: n, canvas: i } = e;
    if (typeof t != "function") throw new Error(`Engine: backend '${r}' descriptor needs a callable 'load'.`);
    if (!Array.isArray(n)) throw new Error(`Engine: backend '${r}' descriptor needs a 'formats' array.`);
    if (typeof i != "boolean") throw new Error(`Engine: backend '${r}' descriptor needs a boolean 'canvas'.`);
    return {
      load: t,
      formats: n,
      canvas: i
    };
  }
  class Wa {
    constructor(e) {
      __privateAdd(this, _Wa_instances);
      __privateAdd(this, _e2, /* @__PURE__ */ new Map());
      __privateAdd(this, _t2, /* @__PURE__ */ new Map());
      __privateAdd(this, _n2);
      __privateAdd(this, _r2, /* @__PURE__ */ new Map());
      const t = {
        ...La,
        ...(e == null ? void 0 : e.backends) ?? {}
      }, n = {};
      for (const [i, s] of Object.entries(t)) n[i] = qa(i, s);
      __privateSet(this, _n2, n);
    }
    async render(e, t, n) {
      return __privateMethod(this, _Wa_instances, s_fn).call(this, e.backendId, e, t, ({ engine: i, quill: s, doc: o }) => i.render(s, o, n ?? void 0));
    }
    async open(e, t) {
      return __privateMethod(this, _Wa_instances, s_fn).call(this, e.backendId, e, t, ({ mod: n, engine: i, quill: s, doc: o }) => new Ja(i.open(s, o), n));
    }
    async supportedFormats(e) {
      return __privateMethod(this, _Wa_instances, i_fn).call(this, e.backendId).formats.slice();
    }
    async supportsCanvas(e) {
      return __privateMethod(this, _Wa_instances, i_fn).call(this, e.backendId).canvas;
    }
  }
  _e2 = new WeakMap();
  _t2 = new WeakMap();
  _n2 = new WeakMap();
  _r2 = new WeakMap();
  _Wa_instances = new WeakSet();
  i_fn = function(e) {
    const t = __privateGet(this, _n2)[e];
    if (!t) throw new Error(`Engine: no backend registered for '${e}'. Known backends: ${Object.keys(__privateGet(this, _n2)).join(", ") || "(none)"}.`);
    return t;
  };
  o_fn = async function(e) {
    const t = __privateMethod(this, _Wa_instances, i_fn).call(this, e);
    let n = __privateGet(this, _e2).get(e);
    n || (n = Promise.resolve().then(t.load).catch((o) => {
      throw __privateGet(this, _e2).delete(e), o;
    }), __privateGet(this, _e2).set(e, n));
    const i = await n;
    let s = __privateGet(this, _t2).get(e);
    return s || (s = new i.Quillmark(), __privateGet(this, _t2).set(e, s)), {
      mod: i,
      engine: s
    };
  };
  l_fn = function(e, t, n, i) {
    let s = __privateGet(this, _r2).get(t);
    s || (s = /* @__PURE__ */ new WeakMap(), __privateGet(this, _r2).set(t, s));
    let o = s.get(n);
    return o || (o = e.Quill.fromTree(i), s.set(n, o)), o;
  };
  s_fn = async function(e, t, n, i) {
    var _a2;
    const s = n.toJson(), o = ((_a2 = __privateGet(this, _r2).get(e)) == null ? void 0 : _a2.has(t)) ? null : t.toTree(), { mod: l, engine: a } = await __privateMethod(this, _Wa_instances, o_fn).call(this, e), c = __privateMethod(this, _Wa_instances, l_fn).call(this, l, e, t, o);
    let f = null;
    try {
      return f = l.Document.fromJson(s), i({
        mod: l,
        engine: a,
        quill: c,
        doc: f
      });
    } finally {
      f == null ? void 0 : f.free();
    }
  };
  class Ja {
    constructor(e, t) {
      __privateAdd(this, _e3);
      __privateAdd(this, _t3);
      __privateSet(this, _e3, e), __privateSet(this, _t3, t);
    }
    apply(e) {
      let t = null;
      try {
        return t = __privateGet(this, _t3).Document.fromJson(e.toJson()), __privateGet(this, _e3).apply(t);
      } finally {
        t == null ? void 0 : t.free();
      }
    }
    get pageCount() {
      return __privateGet(this, _e3).pageCount;
    }
    get backendId() {
      return __privateGet(this, _e3).backendId;
    }
    get supportsCanvas() {
      return __privateGet(this, _e3).supportsCanvas;
    }
    get warnings() {
      return __privateGet(this, _e3).warnings;
    }
    render(e) {
      return __privateGet(this, _e3).render(e ?? void 0);
    }
    regions() {
      return __privateGet(this, _e3).regions();
    }
    fieldAt(e, t, n) {
      return __privateGet(this, _e3).fieldAt(e, t, n);
    }
    positionAt(e, t, n) {
      return __privateGet(this, _e3).positionAt(e, t, n);
    }
    locate(e, t) {
      return __privateGet(this, _e3).locate(e, t);
    }
    pageSize(e) {
      return __privateGet(this, _e3).pageSize(e);
    }
    paint(e, t, n) {
      return __privateGet(this, _e3).paint(e, t, n);
    }
    free() {
      __privateGet(this, _e3).free();
    }
  }
  _e3 = new WeakMap();
  _t3 = new WeakMap();
  function q(r) {
    this.content = r;
  }
  q.prototype = {
    constructor: q,
    find: function(r) {
      for (var e = 0; e < this.content.length; e += 2) if (this.content[e] === r) return e;
      return -1;
    },
    get: function(r) {
      var e = this.find(r);
      return e == -1 ? void 0 : this.content[e + 1];
    },
    update: function(r, e, t) {
      var n = t && t != r ? this.remove(t) : this, i = n.find(r), s = n.content.slice();
      return i == -1 ? s.push(t || r, e) : (s[i + 1] = e, t && (s[i] = t)), new q(s);
    },
    remove: function(r) {
      var e = this.find(r);
      if (e == -1) return this;
      var t = this.content.slice();
      return t.splice(e, 2), new q(t);
    },
    addToStart: function(r, e) {
      return new q([
        r,
        e
      ].concat(this.remove(r).content));
    },
    addToEnd: function(r, e) {
      var t = this.remove(r).content.slice();
      return t.push(r, e), new q(t);
    },
    addBefore: function(r, e, t) {
      var n = this.remove(e), i = n.content.slice(), s = n.find(r);
      return i.splice(s == -1 ? i.length : s, 0, e, t), new q(i);
    },
    forEach: function(r) {
      for (var e = 0; e < this.content.length; e += 2) r(this.content[e], this.content[e + 1]);
    },
    prepend: function(r) {
      return r = q.from(r), r.size ? new q(r.content.concat(this.subtract(r).content)) : this;
    },
    append: function(r) {
      return r = q.from(r), r.size ? new q(this.subtract(r).content.concat(r.content)) : this;
    },
    subtract: function(r) {
      var e = this;
      r = q.from(r);
      for (var t = 0; t < r.content.length; t += 2) e = e.remove(r.content[t]);
      return e;
    },
    toObject: function() {
      var r = {};
      return this.forEach(function(e, t) {
        r[e] = t;
      }), r;
    },
    get size() {
      return this.content.length >> 1;
    }
  };
  q.from = function(r) {
    if (r instanceof q) return r;
    var e = [];
    if (r) for (var t in r) e.push(t, r[t]);
    return new q(e);
  };
  function Ai(r, e, t) {
    for (let n = 0; ; n++) {
      if (n == r.childCount || n == e.childCount) return r.childCount == e.childCount ? null : t;
      let i = r.child(n), s = e.child(n);
      if (i == s) {
        t += i.nodeSize;
        continue;
      }
      if (!i.sameMarkup(s)) return t;
      if (i.isText && i.text != s.text) {
        let o = i.text, l = s.text, a = 0;
        for (; o[a] == l[a]; a++) t++;
        return a && a < o.length && a < l.length && Pi(o.charCodeAt(a - 1)) && Ri(o.charCodeAt(a)) && t--, t;
      }
      if (i.content.size || s.content.size) {
        let o = Ai(i.content, s.content, t + 1);
        if (o != null) return o;
      }
      t += i.nodeSize;
    }
  }
  function vi(r, e, t, n) {
    for (let i = r.childCount, s = e.childCount; ; ) {
      if (i == 0 || s == 0) return i == s ? null : {
        a: t,
        b: n
      };
      let o = r.child(--i), l = e.child(--s), a = o.nodeSize;
      if (o == l) {
        t -= a, n -= a;
        continue;
      }
      if (!o.sameMarkup(l)) return {
        a: t,
        b: n
      };
      if (o.isText && o.text != l.text) {
        let c = o.text, f = l.text, d = c.length, u = f.length;
        for (; d > 0 && u > 0 && c[d - 1] == f[u - 1]; ) d--, u--, t--, n--;
        return d && u && d < c.length && Pi(c.charCodeAt(d - 1)) && Ri(c.charCodeAt(d)) && (t++, n++), {
          a: t,
          b: n
        };
      }
      if (o.content.size || l.content.size) {
        let c = vi(o.content, l.content, t - 1, n - 1);
        if (c) return c;
      }
      t -= a, n -= a;
    }
  }
  function Ri(r) {
    return r >= 56320 && r < 57344;
  }
  function Pi(r) {
    return r >= 55296 && r < 56320;
  }
  class b {
    constructor(e, t) {
      if (this.content = e, this.size = t || 0, t == null) for (let n = 0; n < e.length; n++) this.size += e[n].nodeSize;
    }
    nodesBetween(e, t, n, i = 0, s) {
      for (let o = 0, l = 0; l < t; o++) {
        let a = this.content[o], c = l + a.nodeSize;
        if (c > e && n(a, i + l, s || null, o) !== false && a.content.size) {
          let f = l + 1;
          a.nodesBetween(Math.max(0, e - f), Math.min(a.content.size, t - f), n, i + f);
        }
        l = c;
      }
    }
    descendants(e) {
      this.nodesBetween(0, this.size, e);
    }
    textBetween(e, t, n, i) {
      let s = "", o = true;
      return this.nodesBetween(e, t, (l, a) => {
        let c = l.isText ? l.text.slice(Math.max(e, a) - a, t - a) : l.isLeaf ? i ? typeof i == "function" ? i(l) : i : l.type.spec.leafText ? l.type.spec.leafText(l) : "" : "";
        l.isBlock && (l.isLeaf && c || l.isTextblock) && n && (o ? o = false : s += n), s += c;
      }, 0), s;
    }
    append(e) {
      if (!e.size) return this;
      if (!this.size) return e;
      let t = this.lastChild, n = e.firstChild, i = this.content.slice(), s = 0;
      for (t.isText && t.sameMarkup(n) && (i[i.length - 1] = t.withText(t.text + n.text), s = 1); s < e.content.length; s++) i.push(e.content[s]);
      return new b(i, this.size + e.size);
    }
    cut(e, t = this.size) {
      if (e == 0 && t == this.size) return this;
      let n = [], i = 0;
      if (t > e) for (let s = 0, o = 0; o < t; s++) {
        let l = this.content[s], a = o + l.nodeSize;
        a > e && ((o < e || a > t) && (l.isText ? l = l.cut(Math.max(0, e - o), Math.min(l.text.length, t - o)) : l = l.cut(Math.max(0, e - o - 1), Math.min(l.content.size, t - o - 1))), n.push(l), i += l.nodeSize), o = a;
      }
      return new b(n, i);
    }
    cutByIndex(e, t) {
      return e == t ? b.empty : e == 0 && t == this.content.length ? this : new b(this.content.slice(e, t));
    }
    replaceChild(e, t) {
      let n = this.content[e];
      if (n == t) return this;
      let i = this.content.slice(), s = this.size + t.nodeSize - n.nodeSize;
      return i[e] = t, new b(i, s);
    }
    addToStart(e) {
      return new b([
        e
      ].concat(this.content), this.size + e.nodeSize);
    }
    addToEnd(e) {
      return new b(this.content.concat(e), this.size + e.nodeSize);
    }
    eq(e) {
      if (this.content.length != e.content.length) return false;
      for (let t = 0; t < this.content.length; t++) if (!this.content[t].eq(e.content[t])) return false;
      return true;
    }
    get firstChild() {
      return this.content.length ? this.content[0] : null;
    }
    get lastChild() {
      return this.content.length ? this.content[this.content.length - 1] : null;
    }
    get childCount() {
      return this.content.length;
    }
    child(e) {
      let t = this.content[e];
      if (!t) throw new RangeError("Index " + e + " out of range for " + this);
      return t;
    }
    maybeChild(e) {
      return this.content[e] || null;
    }
    forEach(e) {
      for (let t = 0, n = 0; t < this.content.length; t++) {
        let i = this.content[t];
        e(i, n, t), n += i.nodeSize;
      }
    }
    findDiffStart(e, t = 0) {
      return Ai(this, e, t);
    }
    findDiffEnd(e, t = this.size, n = e.size) {
      return vi(this, e, t, n);
    }
    findIndex(e) {
      if (e == 0) return Vt(0, e);
      if (e == this.size) return Vt(this.content.length, e);
      if (e > this.size || e < 0) throw new RangeError(`Position ${e} outside of fragment (${this})`);
      for (let t = 0, n = 0; ; t++) {
        let i = this.child(t), s = n + i.nodeSize;
        if (s >= e) return s == e ? Vt(t + 1, s) : Vt(t, n);
        n = s;
      }
    }
    toString() {
      return "<" + this.toStringInner() + ">";
    }
    toStringInner() {
      return this.content.join(", ");
    }
    toJSON() {
      return this.content.length ? this.content.map((e) => e.toJSON()) : null;
    }
    static fromJSON(e, t) {
      if (!t) return b.empty;
      if (!Array.isArray(t)) throw new RangeError("Invalid input for Fragment.fromJSON");
      return b.fromArray(t.map(e.nodeFromJSON));
    }
    static fromArray(e) {
      if (!e.length) return b.empty;
      let t, n = 0;
      for (let i = 0; i < e.length; i++) {
        let s = e[i];
        n += s.nodeSize, i && s.isText && e[i - 1].sameMarkup(s) ? (t || (t = e.slice(0, i)), t[t.length - 1] = s.withText(t[t.length - 1].text + s.text)) : t && t.push(s);
      }
      return new b(t || e, n);
    }
    static from(e) {
      if (!e) return b.empty;
      if (e instanceof b) return e;
      if (Array.isArray(e)) return this.fromArray(e);
      if (e.attrs) return new b([
        e
      ], e.nodeSize);
      throw new RangeError("Can not convert " + e + " to a Fragment" + (e.nodesBetween ? " (looks like multiple versions of prosemirror-model were loaded)" : ""));
    }
  }
  b.empty = new b([], 0);
  const mn = {
    index: 0,
    offset: 0
  };
  function Vt(r, e) {
    return mn.index = r, mn.offset = e, mn;
  }
  function Gt(r, e) {
    if (r === e) return true;
    if (!(r && typeof r == "object") || !(e && typeof e == "object")) return false;
    let t = Array.isArray(r);
    if (Array.isArray(e) != t) return false;
    if (t) {
      if (r.length != e.length) return false;
      for (let n = 0; n < r.length; n++) if (!Gt(r[n], e[n])) return false;
    } else {
      for (let n in r) if (!(n in e) || !Gt(r[n], e[n])) return false;
      for (let n in e) if (!(n in r)) return false;
    }
    return true;
  }
  class T {
    constructor(e, t) {
      this.type = e, this.attrs = t;
    }
    addToSet(e) {
      let t, n = false;
      for (let i = 0; i < e.length; i++) {
        let s = e[i];
        if (this.eq(s)) return e;
        if (this.type.excludes(s.type)) t || (t = e.slice(0, i));
        else {
          if (s.type.excludes(this.type)) return e;
          !n && s.type.rank > this.type.rank && (t || (t = e.slice(0, i)), t.push(this), n = true), t && t.push(s);
        }
      }
      return t || (t = e.slice()), n || t.push(this), t;
    }
    removeFromSet(e) {
      for (let t = 0; t < e.length; t++) if (this.eq(e[t])) return e.slice(0, t).concat(e.slice(t + 1));
      return e;
    }
    isInSet(e) {
      for (let t = 0; t < e.length; t++) if (this.eq(e[t])) return true;
      return false;
    }
    eq(e) {
      return this == e || this.type == e.type && Gt(this.attrs, e.attrs);
    }
    toJSON() {
      let e = {
        type: this.type.name
      };
      for (let t in this.attrs) {
        e.attrs = this.attrs;
        break;
      }
      return e;
    }
    static fromJSON(e, t) {
      if (!t) throw new RangeError("Invalid input for Mark.fromJSON");
      let n = e.marks[t.type];
      if (!n) throw new RangeError(`There is no mark type ${t.type} in this schema`);
      let i = n.create(t.attrs);
      return n.checkAttrs(i.attrs), i;
    }
    static sameSet(e, t) {
      if (e == t) return true;
      if (e.length != t.length) return false;
      for (let n = 0; n < e.length; n++) if (!e[n].eq(t[n])) return false;
      return true;
    }
    static setFrom(e) {
      if (!e || Array.isArray(e) && e.length == 0) return T.none;
      if (e instanceof T) return [
        e
      ];
      let t = e.slice();
      return t.sort((n, i) => n.type.rank - i.type.rank), t;
    }
  }
  T.none = [];
  class xt extends Error {
  }
  class w {
    constructor(e, t, n) {
      this.content = e, this.openStart = t, this.openEnd = n;
    }
    get size() {
      return this.content.size - this.openStart - this.openEnd;
    }
    insertAt(e, t) {
      let n = zi(this.content, e + this.openStart, t, this.openStart + 1, this.openEnd + 1);
      return n && new w(n, this.openStart, this.openEnd);
    }
    removeBetween(e, t) {
      return new w(Bi(this.content, e + this.openStart, t + this.openStart), this.openStart, this.openEnd);
    }
    eq(e) {
      return this.content.eq(e.content) && this.openStart == e.openStart && this.openEnd == e.openEnd;
    }
    toString() {
      return this.content + "(" + this.openStart + "," + this.openEnd + ")";
    }
    toJSON() {
      if (!this.content.size) return null;
      let e = {
        content: this.content.toJSON()
      };
      return this.openStart > 0 && (e.openStart = this.openStart), this.openEnd > 0 && (e.openEnd = this.openEnd), e;
    }
    static fromJSON(e, t) {
      if (!t) return w.empty;
      let n = t.openStart || 0, i = t.openEnd || 0;
      if (typeof n != "number" || typeof i != "number") throw new RangeError("Invalid input for Slice.fromJSON");
      return new w(b.fromJSON(e, t.content), n, i);
    }
    static maxOpen(e, t = true) {
      let n = 0, i = 0;
      for (let s = e.firstChild; s && !s.isLeaf && (t || !s.type.spec.isolating); s = s.firstChild) n++;
      for (let s = e.lastChild; s && !s.isLeaf && (t || !s.type.spec.isolating); s = s.lastChild) i++;
      return new w(e, n, i);
    }
  }
  w.empty = new w(b.empty, 0, 0);
  function Bi(r, e, t) {
    let { index: n, offset: i } = r.findIndex(e), s = r.maybeChild(n), { index: o, offset: l } = r.findIndex(t);
    if (i == e || s.isText) {
      if (l != t && !r.child(o).isText) throw new RangeError("Removing non-flat range");
      return r.cut(0, e).append(r.cut(t));
    }
    if (n != o) throw new RangeError("Removing non-flat range");
    return r.replaceChild(n, s.copy(Bi(s.content, e - i - 1, t - i - 1)));
  }
  function zi(r, e, t, n, i, s) {
    let { index: o, offset: l } = r.findIndex(e), a = r.maybeChild(o);
    if (l == e || a.isText) return s && n <= 0 && i <= 0 && !s.canReplace(o, o, t) ? null : r.cut(0, e).append(t).append(r.cut(e));
    let c = zi(a.content, e - l - 1, t, o == 0 ? n - 1 : 0, o == r.childCount - 1 ? i - 1 : 0, a);
    return c && r.replaceChild(o, a.copy(c));
  }
  function $a(r, e, t) {
    if (t.openStart > r.depth) throw new xt("Inserted content deeper than insertion position");
    if (r.depth - t.openStart != e.depth - t.openEnd) throw new xt("Inconsistent open depths");
    return Fi(r, e, t, 0);
  }
  function Fi(r, e, t, n) {
    let i = r.index(n), s = r.node(n);
    if (i == e.index(n) && n < r.depth - t.openStart) {
      let o = Fi(r, e, t, n + 1);
      return s.copy(s.content.replaceChild(i, o));
    } else if (t.content.size) if (!t.openStart && !t.openEnd && r.depth == n && e.depth == n) {
      let o = r.parent, l = o.content;
      return $e(o, l.cut(0, r.parentOffset).append(t.content).append(l.cut(e.parentOffset)));
    } else {
      let { start: o, end: l } = ja(t, r);
      return $e(s, Li(r, o, l, e, n));
    }
    else return $e(s, Yt(r, e, n));
  }
  function Vi(r, e) {
    if (!e.type.compatibleContent(r.type)) throw new xt("Cannot join " + e.type.name + " onto " + r.type.name);
  }
  function Fn(r, e, t) {
    let n = r.node(t);
    return Vi(n, e.node(t)), n;
  }
  function Je(r, e) {
    let t = e.length - 1;
    t >= 0 && r.isText && r.sameMarkup(e[t]) ? e[t] = r.withText(e[t].text + r.text) : e.push(r);
  }
  function _t(r, e, t, n) {
    let i = (e || r).node(t), s = 0, o = e ? e.index(t) : i.childCount;
    r && (s = r.index(t), r.depth > t ? s++ : r.textOffset && (Je(r.nodeAfter, n), s++));
    for (let l = s; l < o; l++) Je(i.child(l), n);
    e && e.depth == t && e.textOffset && Je(e.nodeBefore, n);
  }
  function $e(r, e) {
    if (!r.type.validContent(e)) throw new xt("Invalid content for node " + r.type.name);
    return r.copy(e);
  }
  function Li(r, e, t, n, i) {
    let s = r.depth > i && Fn(r, e, i + 1), o = n.depth > i && Fn(t, n, i + 1), l = [];
    return _t(null, r, i, l), s && o && e.index(i) == t.index(i) ? (Vi(s, o), Je($e(s, Li(r, e, t, n, i + 1)), l)) : (s && Je($e(s, Yt(r, e, i + 1)), l), _t(e, t, i, l), o && Je($e(o, Yt(t, n, i + 1)), l)), _t(n, null, i, l), new b(l);
  }
  function Yt(r, e, t) {
    let n = [];
    if (_t(null, r, t, n), r.depth > t) {
      let i = Fn(r, e, t + 1);
      Je($e(i, Yt(r, e, t + 1)), n);
    }
    return _t(e, null, t, n), new b(n);
  }
  function ja(r, e) {
    let t = e.depth - r.openStart, i = e.node(t).copy(r.content);
    for (let s = t - 1; s >= 0; s--) i = e.node(s).copy(b.from(i));
    return {
      start: i.resolveNoCache(r.openStart + t),
      end: i.resolveNoCache(i.content.size - r.openEnd - t)
    };
  }
  class St {
    constructor(e, t, n) {
      this.pos = e, this.path = t, this.parentOffset = n, this.depth = t.length / 3 - 1;
    }
    resolveDepth(e) {
      return e == null ? this.depth : e < 0 ? this.depth + e : e;
    }
    get parent() {
      return this.node(this.depth);
    }
    get doc() {
      return this.node(0);
    }
    node(e) {
      return this.path[this.resolveDepth(e) * 3];
    }
    index(e) {
      return this.path[this.resolveDepth(e) * 3 + 1];
    }
    indexAfter(e) {
      return e = this.resolveDepth(e), this.index(e) + (e == this.depth && !this.textOffset ? 0 : 1);
    }
    start(e) {
      return e = this.resolveDepth(e), e == 0 ? 0 : this.path[e * 3 - 1] + 1;
    }
    end(e) {
      return e = this.resolveDepth(e), this.start(e) + this.node(e).content.size;
    }
    before(e) {
      if (e = this.resolveDepth(e), !e) throw new RangeError("There is no position before the top-level node");
      return e == this.depth + 1 ? this.pos : this.path[e * 3 - 1];
    }
    after(e) {
      if (e = this.resolveDepth(e), !e) throw new RangeError("There is no position after the top-level node");
      return e == this.depth + 1 ? this.pos : this.path[e * 3 - 1] + this.path[e * 3].nodeSize;
    }
    get textOffset() {
      return this.pos - this.path[this.path.length - 1];
    }
    get nodeAfter() {
      let e = this.parent, t = this.index(this.depth);
      if (t == e.childCount) return null;
      let n = this.pos - this.path[this.path.length - 1], i = e.child(t);
      return n ? e.child(t).cut(n) : i;
    }
    get nodeBefore() {
      let e = this.index(this.depth), t = this.pos - this.path[this.path.length - 1];
      return t ? this.parent.child(e).cut(0, t) : e == 0 ? null : this.parent.child(e - 1);
    }
    posAtIndex(e, t) {
      t = this.resolveDepth(t);
      let n = this.path[t * 3], i = t == 0 ? 0 : this.path[t * 3 - 1] + 1;
      for (let s = 0; s < e; s++) i += n.child(s).nodeSize;
      return i;
    }
    marks() {
      let e = this.parent, t = this.index();
      if (e.content.size == 0) return T.none;
      if (this.textOffset) return e.child(t).marks;
      let n = e.maybeChild(t - 1), i = e.maybeChild(t);
      if (!n) {
        let l = n;
        n = i, i = l;
      }
      let s = n.marks;
      for (var o = 0; o < s.length; o++) s[o].type.spec.inclusive === false && (!i || !s[o].isInSet(i.marks)) && (s = s[o--].removeFromSet(s));
      return s;
    }
    marksAcross(e) {
      let t = this.parent.maybeChild(this.index());
      if (!t || !t.isInline) return null;
      let n = t.marks, i = e.parent.maybeChild(e.index());
      for (var s = 0; s < n.length; s++) n[s].type.spec.inclusive === false && (!i || !n[s].isInSet(i.marks)) && (n = n[s--].removeFromSet(n));
      return n;
    }
    sharedDepth(e) {
      for (let t = this.depth; t > 0; t--) if (this.start(t) <= e && this.end(t) >= e) return t;
      return 0;
    }
    blockRange(e = this, t) {
      if (e.pos < this.pos) return e.blockRange(this);
      for (let n = this.depth - (this.parent.inlineContent || this.pos == e.pos ? 1 : 0); n >= 0; n--) if (e.pos <= this.end(n) && (!t || t(this.node(n)))) return new Ua(this, e, n);
      return null;
    }
    sameParent(e) {
      return this.pos - this.parentOffset == e.pos - e.parentOffset;
    }
    max(e) {
      return e.pos > this.pos ? e : this;
    }
    min(e) {
      return e.pos < this.pos ? e : this;
    }
    toString() {
      let e = "";
      for (let t = 1; t <= this.depth; t++) e += (e ? "/" : "") + this.node(t).type.name + "_" + this.index(t - 1);
      return e + ":" + this.parentOffset;
    }
    static resolve(e, t) {
      if (!(t >= 0 && t <= e.content.size)) throw new RangeError("Position " + t + " out of range");
      let n = [], i = 0, s = t;
      for (let o = e; ; ) {
        let { index: l, offset: a } = o.content.findIndex(s), c = s - a;
        if (n.push(o, l, i + a), !c || (o = o.child(l), o.isText)) break;
        s = c - 1, i += a + 1;
      }
      return new St(t, n, s);
    }
    static resolveCached(e, t) {
      let n = kr.get(e);
      if (n) for (let s = 0; s < n.elts.length; s++) {
        let o = n.elts[s];
        if (o.pos == t) return o;
      }
      else kr.set(e, n = new Ka());
      let i = n.elts[n.i] = St.resolve(e, t);
      return n.i = (n.i + 1) % Ha, i;
    }
  }
  class Ka {
    constructor() {
      this.elts = [], this.i = 0;
    }
  }
  const Ha = 12, kr = /* @__PURE__ */ new WeakMap();
  class Ua {
    constructor(e, t, n) {
      this.$from = e, this.$to = t, this.depth = n;
    }
    get start() {
      return this.$from.before(this.depth + 1);
    }
    get end() {
      return this.$to.after(this.depth + 1);
    }
    get parent() {
      return this.$from.node(this.depth);
    }
    get startIndex() {
      return this.$from.index(this.depth);
    }
    get endIndex() {
      return this.$to.indexAfter(this.depth);
    }
  }
  const Ga = /* @__PURE__ */ Object.create(null);
  class me {
    constructor(e, t, n, i = T.none) {
      this.type = e, this.attrs = t, this.marks = i, this.content = n || b.empty;
    }
    get children() {
      return this.content.content;
    }
    get nodeSize() {
      return this.isLeaf ? 1 : 2 + this.content.size;
    }
    get childCount() {
      return this.content.childCount;
    }
    child(e) {
      return this.content.child(e);
    }
    maybeChild(e) {
      return this.content.maybeChild(e);
    }
    forEach(e) {
      this.content.forEach(e);
    }
    nodesBetween(e, t, n, i = 0) {
      this.content.nodesBetween(e, t, n, i, this);
    }
    descendants(e) {
      this.nodesBetween(0, this.content.size, e);
    }
    get textContent() {
      return this.isLeaf && this.type.spec.leafText ? this.type.spec.leafText(this) : this.textBetween(0, this.content.size, "");
    }
    textBetween(e, t, n, i) {
      return this.content.textBetween(e, t, n, i);
    }
    get firstChild() {
      return this.content.firstChild;
    }
    get lastChild() {
      return this.content.lastChild;
    }
    eq(e) {
      return this == e || this.sameMarkup(e) && this.content.eq(e.content);
    }
    sameMarkup(e) {
      return this.hasMarkup(e.type, e.attrs, e.marks);
    }
    hasMarkup(e, t, n) {
      return this.type == e && Gt(this.attrs, t || e.defaultAttrs || Ga) && T.sameSet(this.marks, n || T.none);
    }
    copy(e = null) {
      return e == this.content ? this : new me(this.type, this.attrs, e, this.marks);
    }
    mark(e) {
      return e == this.marks ? this : new me(this.type, this.attrs, this.content, e);
    }
    cut(e, t = this.content.size) {
      return e == 0 && t == this.content.size ? this : this.copy(this.content.cut(e, t));
    }
    slice(e, t = this.content.size, n = false) {
      if (e == t) return w.empty;
      let i = this.resolve(e), s = this.resolve(t), o = n ? 0 : i.sharedDepth(t), l = i.start(o), c = i.node(o).content.cut(i.pos - l, s.pos - l);
      return new w(c, i.depth - o, s.depth - o);
    }
    replace(e, t, n) {
      return $a(this.resolve(e), this.resolve(t), n);
    }
    nodeAt(e) {
      for (let t = this; ; ) {
        let { index: n, offset: i } = t.content.findIndex(e);
        if (t = t.maybeChild(n), !t) return null;
        if (i == e || t.isText) return t;
        e -= i + 1;
      }
    }
    childAfter(e) {
      let { index: t, offset: n } = this.content.findIndex(e);
      return {
        node: this.content.maybeChild(t),
        index: t,
        offset: n
      };
    }
    childBefore(e) {
      if (e == 0) return {
        node: null,
        index: 0,
        offset: 0
      };
      let { index: t, offset: n } = this.content.findIndex(e);
      if (n < e) return {
        node: this.content.child(t),
        index: t,
        offset: n
      };
      let i = this.content.child(t - 1);
      return {
        node: i,
        index: t - 1,
        offset: n - i.nodeSize
      };
    }
    resolve(e) {
      return St.resolveCached(this, e);
    }
    resolveNoCache(e) {
      return St.resolve(this, e);
    }
    rangeHasMark(e, t, n) {
      let i = false;
      return t > e && this.nodesBetween(e, t, (s) => (n.isInSet(s.marks) && (i = true), !i)), i;
    }
    get isBlock() {
      return this.type.isBlock;
    }
    get isTextblock() {
      return this.type.isTextblock;
    }
    get inlineContent() {
      return this.type.inlineContent;
    }
    get isInline() {
      return this.type.isInline;
    }
    get isText() {
      return this.type.isText;
    }
    get isLeaf() {
      return this.type.isLeaf;
    }
    get isAtom() {
      return this.type.isAtom;
    }
    toString() {
      if (this.type.spec.toDebugString) return this.type.spec.toDebugString(this);
      let e = this.type.name;
      return this.content.size && (e += "(" + this.content.toStringInner() + ")"), qi(this.marks, e);
    }
    contentMatchAt(e) {
      let t = this.type.contentMatch.matchFragment(this.content, 0, e);
      if (!t) throw new Error("Called contentMatchAt on a node with invalid content");
      return t;
    }
    canReplace(e, t, n = b.empty, i = 0, s = n.childCount) {
      let o = this.contentMatchAt(e).matchFragment(n, i, s), l = o && o.matchFragment(this.content, t);
      if (!l || !l.validEnd) return false;
      for (let a = i; a < s; a++) if (!this.type.allowsMarks(n.child(a).marks)) return false;
      return true;
    }
    canReplaceWith(e, t, n, i) {
      if (i && !this.type.allowsMarks(i)) return false;
      let s = this.contentMatchAt(e).matchType(n), o = s && s.matchFragment(this.content, t);
      return o ? o.validEnd : false;
    }
    canAppend(e) {
      return e.content.size ? this.canReplace(this.childCount, this.childCount, e.content) : this.type.compatibleContent(e.type);
    }
    check() {
      this.type.checkContent(this.content), this.type.checkAttrs(this.attrs);
      let e = T.none;
      for (let t = 0; t < this.marks.length; t++) {
        let n = this.marks[t];
        n.type.checkAttrs(n.attrs), e = n.addToSet(e);
      }
      if (!T.sameSet(e, this.marks)) throw new RangeError(`Invalid collection of marks for node ${this.type.name}: ${this.marks.map((t) => t.type.name)}`);
      this.content.forEach((t) => t.check());
    }
    toJSON() {
      let e = {
        type: this.type.name
      };
      for (let t in this.attrs) {
        e.attrs = this.attrs;
        break;
      }
      return this.content.size && (e.content = this.content.toJSON()), this.marks.length && (e.marks = this.marks.map((t) => t.toJSON())), e;
    }
    static fromJSON(e, t) {
      if (!t) throw new RangeError("Invalid input for Node.fromJSON");
      let n;
      if (t.marks) {
        if (!Array.isArray(t.marks)) throw new RangeError("Invalid mark data for Node.fromJSON");
        n = t.marks.map(e.markFromJSON);
      }
      if (t.type == "text") {
        if (typeof t.text != "string") throw new RangeError("Invalid text node in JSON");
        return e.text(t.text, n);
      }
      let i = b.fromJSON(e, t.content), s = e.nodeType(t.type).create(t.attrs, i, n);
      return s.type.checkAttrs(s.attrs), s;
    }
  }
  me.prototype.text = void 0;
  class Qt extends me {
    constructor(e, t, n, i) {
      if (super(e, t, null, i), !n) throw new RangeError("Empty text nodes are not allowed");
      this.text = n;
    }
    toString() {
      return this.type.spec.toDebugString ? this.type.spec.toDebugString(this) : qi(this.marks, JSON.stringify(this.text));
    }
    get textContent() {
      return this.text;
    }
    textBetween(e, t) {
      return this.text.slice(e, t);
    }
    get nodeSize() {
      return this.text.length;
    }
    mark(e) {
      return e == this.marks ? this : new Qt(this.type, this.attrs, this.text, e);
    }
    withText(e) {
      return e == this.text ? this : new Qt(this.type, this.attrs, e, this.marks);
    }
    cut(e = 0, t = this.text.length) {
      return e == 0 && t == this.text.length ? this : this.withText(this.text.slice(e, t));
    }
    eq(e) {
      return this.sameMarkup(e) && this.text == e.text;
    }
    toJSON() {
      let e = super.toJSON();
      return e.text = this.text, e;
    }
  }
  function qi(r, e) {
    for (let t = r.length - 1; t >= 0; t--) e = r[t].type.name + "(" + e + ")";
    return e;
  }
  class Ue {
    constructor(e) {
      this.validEnd = e, this.next = [], this.wrapCache = [];
    }
    static parse(e, t) {
      let n = new Ya(e, t);
      if (n.next == null) return Ue.empty;
      let i = Wi(n);
      n.next && n.err("Unexpected trailing text");
      let s = rc(nc(i));
      return ic(s, n), s;
    }
    matchType(e) {
      for (let t = 0; t < this.next.length; t++) if (this.next[t].type == e) return this.next[t].next;
      return null;
    }
    matchFragment(e, t = 0, n = e.childCount) {
      let i = this;
      for (let s = t; i && s < n; s++) i = i.matchType(e.child(s).type);
      return i;
    }
    get inlineContent() {
      return this.next.length != 0 && this.next[0].type.isInline;
    }
    get defaultType() {
      for (let e = 0; e < this.next.length; e++) {
        let { type: t } = this.next[e];
        if (!(t.isText || t.hasRequiredAttrs())) return t;
      }
      return null;
    }
    compatible(e) {
      for (let t = 0; t < this.next.length; t++) for (let n = 0; n < e.next.length; n++) if (this.next[t].type == e.next[n].type) return true;
      return false;
    }
    fillBefore(e, t = false, n = 0) {
      let i = [
        this
      ];
      function s(o, l) {
        let a = o.matchFragment(e, n);
        if (a && (!t || a.validEnd)) return b.from(l.map((c) => c.createAndFill()));
        for (let c = 0; c < o.next.length; c++) {
          let { type: f, next: d } = o.next[c];
          if (!(f.isText || f.hasRequiredAttrs()) && i.indexOf(d) == -1) {
            i.push(d);
            let u = s(d, l.concat(f));
            if (u) return u;
          }
        }
        return null;
      }
      return s(this, []);
    }
    findWrapping(e) {
      for (let n = 0; n < this.wrapCache.length; n += 2) if (this.wrapCache[n] == e) return this.wrapCache[n + 1];
      let t = this.computeWrapping(e);
      return this.wrapCache.push(e, t), t;
    }
    computeWrapping(e) {
      let t = /* @__PURE__ */ Object.create(null), n = [
        {
          match: this,
          type: null,
          via: null
        }
      ];
      for (; n.length; ) {
        let i = n.shift(), s = i.match;
        if (s.matchType(e)) {
          let o = [];
          for (let l = i; l.type; l = l.via) o.push(l.type);
          return o.reverse();
        }
        for (let o = 0; o < s.next.length; o++) {
          let { type: l, next: a } = s.next[o];
          !l.isLeaf && !l.hasRequiredAttrs() && !(l.name in t) && (!i.type || a.validEnd) && (n.push({
            match: l.contentMatch,
            type: l,
            via: i
          }), t[l.name] = true);
        }
      }
      return null;
    }
    get edgeCount() {
      return this.next.length;
    }
    edge(e) {
      if (e >= this.next.length) throw new RangeError(`There's no ${e}th edge in this content match`);
      return this.next[e];
    }
    toString() {
      let e = [];
      function t(n) {
        e.push(n);
        for (let i = 0; i < n.next.length; i++) e.indexOf(n.next[i].next) == -1 && t(n.next[i].next);
      }
      return t(this), e.map((n, i) => {
        let s = i + (n.validEnd ? "*" : " ") + " ";
        for (let o = 0; o < n.next.length; o++) s += (o ? ", " : "") + n.next[o].type.name + "->" + e.indexOf(n.next[o].next);
        return s;
      }).join(`
`);
    }
  }
  Ue.empty = new Ue(true);
  class Ya {
    constructor(e, t) {
      this.string = e, this.nodeTypes = t, this.inline = null, this.pos = 0, this.tokens = e.split(/\s*(?=\b|\W|$)/), this.tokens[this.tokens.length - 1] == "" && this.tokens.pop(), this.tokens[0] == "" && this.tokens.shift();
    }
    get next() {
      return this.tokens[this.pos];
    }
    eat(e) {
      return this.next == e && (this.pos++ || true);
    }
    err(e) {
      throw new SyntaxError(e + " (in content expression '" + this.string + "')");
    }
  }
  function Wi(r) {
    let e = [];
    do
      e.push(Qa(r));
    while (r.eat("|"));
    return e.length == 1 ? e[0] : {
      type: "choice",
      exprs: e
    };
  }
  function Qa(r) {
    let e = [];
    do
      e.push(Xa(r));
    while (r.next && r.next != ")" && r.next != "|");
    return e.length == 1 ? e[0] : {
      type: "seq",
      exprs: e
    };
  }
  function Xa(r) {
    let e = tc(r);
    for (; ; ) if (r.eat("+")) e = {
      type: "plus",
      expr: e
    };
    else if (r.eat("*")) e = {
      type: "star",
      expr: e
    };
    else if (r.eat("?")) e = {
      type: "opt",
      expr: e
    };
    else if (r.eat("{")) e = Za(r, e);
    else break;
    return e;
  }
  function xr(r) {
    /\D/.test(r.next) && r.err("Expected number, got '" + r.next + "'");
    let e = Number(r.next);
    return r.pos++, e;
  }
  function Za(r, e) {
    let t = xr(r), n = t;
    return r.eat(",") && (r.next != "}" ? n = xr(r) : n = -1), r.eat("}") || r.err("Unclosed braced range"), {
      type: "range",
      min: t,
      max: n,
      expr: e
    };
  }
  function ec(r, e) {
    let t = r.nodeTypes, n = t[e];
    if (n) return [
      n
    ];
    let i = [];
    for (let s in t) {
      let o = t[s];
      o.isInGroup(e) && i.push(o);
    }
    return i.length == 0 && r.err("No node type or group '" + e + "' found"), i;
  }
  function tc(r) {
    if (r.eat("(")) {
      let e = Wi(r);
      return r.eat(")") || r.err("Missing closing paren"), e;
    } else if (/\W/.test(r.next)) r.err("Unexpected token '" + r.next + "'");
    else {
      let e = ec(r, r.next).map((t) => (r.inline == null ? r.inline = t.isInline : r.inline != t.isInline && r.err("Mixing inline and block content"), {
        type: "name",
        value: t
      }));
      return r.pos++, e.length == 1 ? e[0] : {
        type: "choice",
        exprs: e
      };
    }
  }
  function nc(r) {
    let e = [
      []
    ];
    return i(s(r, 0), t()), e;
    function t() {
      return e.push([]) - 1;
    }
    function n(o, l, a) {
      let c = {
        term: a,
        to: l
      };
      return e[o].push(c), c;
    }
    function i(o, l) {
      o.forEach((a) => a.to = l);
    }
    function s(o, l) {
      if (o.type == "choice") return o.exprs.reduce((a, c) => a.concat(s(c, l)), []);
      if (o.type == "seq") for (let a = 0; ; a++) {
        let c = s(o.exprs[a], l);
        if (a == o.exprs.length - 1) return c;
        i(c, l = t());
      }
      else if (o.type == "star") {
        let a = t();
        return n(l, a), i(s(o.expr, a), a), [
          n(a)
        ];
      } else if (o.type == "plus") {
        let a = t();
        return i(s(o.expr, l), a), i(s(o.expr, a), a), [
          n(a)
        ];
      } else {
        if (o.type == "opt") return [
          n(l)
        ].concat(s(o.expr, l));
        if (o.type == "range") {
          let a = l;
          for (let c = 0; c < o.min; c++) {
            let f = t();
            i(s(o.expr, a), f), a = f;
          }
          if (o.max == -1) i(s(o.expr, a), a);
          else for (let c = o.min; c < o.max; c++) {
            let f = t();
            n(a, f), i(s(o.expr, a), f), a = f;
          }
          return [
            n(a)
          ];
        } else {
          if (o.type == "name") return [
            n(l, void 0, o.value)
          ];
          throw new Error("Unknown expr type");
        }
      }
    }
  }
  function Ji(r, e) {
    return e - r;
  }
  function Sr(r, e) {
    let t = [];
    return n(e), t.sort(Ji);
    function n(i) {
      let s = r[i];
      if (s.length == 1 && !s[0].term) return n(s[0].to);
      t.push(i);
      for (let o = 0; o < s.length; o++) {
        let { term: l, to: a } = s[o];
        !l && t.indexOf(a) == -1 && n(a);
      }
    }
  }
  function rc(r) {
    let e = /* @__PURE__ */ Object.create(null);
    return t(Sr(r, 0));
    function t(n) {
      let i = [];
      n.forEach((o) => {
        r[o].forEach(({ term: l, to: a }) => {
          if (!l) return;
          let c;
          for (let f = 0; f < i.length; f++) i[f][0] == l && (c = i[f][1]);
          Sr(r, a).forEach((f) => {
            c || i.push([
              l,
              c = []
            ]), c.indexOf(f) == -1 && c.push(f);
          });
        });
      });
      let s = e[n.join(",")] = new Ue(n.indexOf(r.length - 1) > -1);
      for (let o = 0; o < i.length; o++) {
        let l = i[o][1].sort(Ji);
        s.next.push({
          type: i[o][0],
          next: e[l.join(",")] || t(l)
        });
      }
      return s;
    }
  }
  function ic(r, e) {
    for (let t = 0, n = [
      r
    ]; t < n.length; t++) {
      let i = n[t], s = !i.validEnd, o = [];
      for (let l = 0; l < i.next.length; l++) {
        let { type: a, next: c } = i.next[l];
        o.push(a.name), s && !(a.isText || a.hasRequiredAttrs()) && (s = false), n.indexOf(c) == -1 && n.push(c);
      }
      s && e.err("Only non-generatable nodes (" + o.join(", ") + ") in a required position (see https://prosemirror.net/docs/guide/#generatable)");
    }
  }
  function $i(r) {
    let e = /* @__PURE__ */ Object.create(null);
    for (let t in r) {
      let n = r[t];
      if (!n.hasDefault) return null;
      e[t] = n.default;
    }
    return e;
  }
  function ji(r, e) {
    let t = /* @__PURE__ */ Object.create(null);
    for (let n in r) {
      let i = e && e[n];
      if (i === void 0) {
        let s = r[n];
        if (s.hasDefault) i = s.default;
        else throw new RangeError("No value supplied for attribute " + n);
      }
      t[n] = i;
    }
    return t;
  }
  function Ki(r, e, t, n) {
    for (let i in e) if (!(i in r)) throw new RangeError(`Unsupported attribute ${i} for ${t} of type ${n}`);
    for (let i in r) r[i].validate && r[i].validate(e[i]);
  }
  function Hi(r, e) {
    let t = /* @__PURE__ */ Object.create(null);
    if (e) for (let n in e) t[n] = new oc(r, n, e[n]);
    return t;
  }
  let Cr = class Ui {
    constructor(e, t, n) {
      this.name = e, this.schema = t, this.spec = n, this.markSet = null, this.groups = n.group ? n.group.split(" ") : [], this.attrs = Hi(e, n.attrs), this.defaultAttrs = $i(this.attrs), this.contentMatch = null, this.inlineContent = null, this.isBlock = !(n.inline || e == "text"), this.isText = e == "text";
    }
    get isInline() {
      return !this.isBlock;
    }
    get isTextblock() {
      return this.isBlock && this.inlineContent;
    }
    get isLeaf() {
      return this.contentMatch == Ue.empty;
    }
    get isAtom() {
      return this.isLeaf || !!this.spec.atom;
    }
    isInGroup(e) {
      return this.groups.indexOf(e) > -1;
    }
    get whitespace() {
      return this.spec.whitespace || (this.spec.code ? "pre" : "normal");
    }
    hasRequiredAttrs() {
      for (let e in this.attrs) if (this.attrs[e].isRequired) return true;
      return false;
    }
    compatibleContent(e) {
      return this == e || this.contentMatch.compatible(e.contentMatch);
    }
    computeAttrs(e) {
      return !e && this.defaultAttrs ? this.defaultAttrs : ji(this.attrs, e);
    }
    create(e = null, t, n) {
      if (this.isText) throw new Error("NodeType.create can't construct text nodes");
      return new me(this, this.computeAttrs(e), b.from(t), T.setFrom(n));
    }
    createChecked(e = null, t, n) {
      return t = b.from(t), this.checkContent(t), new me(this, this.computeAttrs(e), t, T.setFrom(n));
    }
    createAndFill(e = null, t, n) {
      if (e = this.computeAttrs(e), t = b.from(t), t.size) {
        let o = this.contentMatch.fillBefore(t);
        if (!o) return null;
        t = o.append(t);
      }
      let i = this.contentMatch.matchFragment(t), s = i && i.fillBefore(b.empty, true);
      return s ? new me(this, e, t.append(s), T.setFrom(n)) : null;
    }
    validContent(e) {
      let t = this.contentMatch.matchFragment(e);
      if (!t || !t.validEnd) return false;
      for (let n = 0; n < e.childCount; n++) if (!this.allowsMarks(e.child(n).marks)) return false;
      return true;
    }
    checkContent(e) {
      if (!this.validContent(e)) throw new RangeError(`Invalid content for node ${this.name}: ${e.toString().slice(0, 50)}`);
    }
    checkAttrs(e) {
      Ki(this.attrs, e, "node", this.name);
    }
    allowsMarkType(e) {
      return this.markSet == null || this.markSet.indexOf(e) > -1;
    }
    allowsMarks(e) {
      if (this.markSet == null) return true;
      for (let t = 0; t < e.length; t++) if (!this.allowsMarkType(e[t].type)) return false;
      return true;
    }
    allowedMarks(e) {
      if (this.markSet == null) return e;
      let t;
      for (let n = 0; n < e.length; n++) this.allowsMarkType(e[n].type) ? t && t.push(e[n]) : t || (t = e.slice(0, n));
      return t ? t.length ? t : T.none : e;
    }
    static compile(e, t) {
      let n = /* @__PURE__ */ Object.create(null);
      e.forEach((s, o) => n[s] = new Ui(s, t, o));
      let i = t.spec.topNode || "doc";
      if (!n[i]) throw new RangeError("Schema is missing its top node type ('" + i + "')");
      if (!n.text) throw new RangeError("Every schema needs a 'text' type");
      for (let s in n.text.attrs) throw new RangeError("The text node type should not have attributes");
      return n;
    }
  };
  function sc(r, e, t) {
    let n = t.split("|");
    return (i) => {
      let s = i === null ? "null" : typeof i;
      if (n.indexOf(s) < 0) throw new RangeError(`Expected value of type ${n} for attribute ${e} on type ${r}, got ${s}`);
    };
  }
  class oc {
    constructor(e, t, n) {
      this.hasDefault = Object.prototype.hasOwnProperty.call(n, "default"), this.default = n.default, this.validate = typeof n.validate == "string" ? sc(e, t, n.validate) : n.validate;
    }
    get isRequired() {
      return !this.hasDefault;
    }
  }
  class an {
    constructor(e, t, n, i) {
      this.name = e, this.rank = t, this.schema = n, this.spec = i, this.attrs = Hi(e, i.attrs), this.excluded = null;
      let s = $i(this.attrs);
      this.instance = s ? new T(this, s) : null;
    }
    create(e = null) {
      return !e && this.instance ? this.instance : new T(this, ji(this.attrs, e));
    }
    static compile(e, t) {
      let n = /* @__PURE__ */ Object.create(null), i = 0;
      return e.forEach((s, o) => n[s] = new an(s, i++, t, o)), n;
    }
    removeFromSet(e) {
      for (var t = 0; t < e.length; t++) e[t].type == this && (e = e.slice(0, t).concat(e.slice(t + 1)), t--);
      return e;
    }
    isInSet(e) {
      for (let t = 0; t < e.length; t++) if (e[t].type == this) return e[t];
    }
    checkAttrs(e) {
      Ki(this.attrs, e, "mark", this.name);
    }
    excludes(e) {
      return this.excluded.indexOf(e) > -1;
    }
  }
  class Gi {
    constructor(e) {
      this.linebreakReplacement = null, this.cached = /* @__PURE__ */ Object.create(null);
      let t = this.spec = {};
      for (let i in e) t[i] = e[i];
      t.nodes = q.from(e.nodes), t.marks = q.from(e.marks || {}), this.nodes = Cr.compile(this.spec.nodes, this), this.marks = an.compile(this.spec.marks, this);
      let n = /* @__PURE__ */ Object.create(null);
      for (let i in this.nodes) {
        if (i in this.marks) throw new RangeError(i + " can not be both a node and a mark");
        let s = this.nodes[i], o = s.spec.content || "", l = s.spec.marks;
        if (s.contentMatch = n[o] || (n[o] = Ue.parse(o, this.nodes)), s.inlineContent = s.contentMatch.inlineContent, s.spec.linebreakReplacement) {
          if (this.linebreakReplacement) throw new RangeError("Multiple linebreak nodes defined");
          if (!s.isInline || !s.isLeaf) throw new RangeError("Linebreak replacement nodes must be inline leaf nodes");
          this.linebreakReplacement = s;
        }
        s.markSet = l == "_" ? null : l ? Or(this, l.split(" ")) : l == "" || !s.inlineContent ? [] : null;
      }
      for (let i in this.marks) {
        let s = this.marks[i], o = s.spec.excludes;
        s.excluded = o == null ? [
          s
        ] : o == "" ? [] : Or(this, o.split(" "));
      }
      this.nodeFromJSON = (i) => me.fromJSON(this, i), this.markFromJSON = (i) => T.fromJSON(this, i), this.topNodeType = this.nodes[this.spec.topNode || "doc"], this.cached.wrappings = /* @__PURE__ */ Object.create(null);
    }
    node(e, t = null, n, i) {
      if (typeof e == "string") e = this.nodeType(e);
      else if (e instanceof Cr) {
        if (e.schema != this) throw new RangeError("Node type from different schema used (" + e.name + ")");
      } else throw new RangeError("Invalid node type: " + e);
      return e.createChecked(t, n, i);
    }
    text(e, t) {
      let n = this.nodes.text;
      return new Qt(n, n.defaultAttrs, e, T.setFrom(t));
    }
    mark(e, t) {
      return typeof e == "string" && (e = this.marks[e]), e.create(t);
    }
    nodeType(e) {
      let t = this.nodes[e];
      if (!t) throw new RangeError("Unknown node type: " + e);
      return t;
    }
  }
  function Or(r, e) {
    let t = [];
    for (let n = 0; n < e.length; n++) {
      let i = e[n], s = r.marks[i], o = s;
      if (s) t.push(s);
      else for (let l in r.marks) {
        let a = r.marks[l];
        (i == "_" || a.spec.group && a.spec.group.split(" ").indexOf(i) > -1) && t.push(o = a);
      }
      if (!o) throw new SyntaxError("Unknown mark type: '" + e[n] + "'");
    }
    return t;
  }
  function lc(r) {
    return r.tag != null;
  }
  function ac(r) {
    return r.style != null;
  }
  class Ct {
    constructor(e, t) {
      this.schema = e, this.rules = t, this.tags = [], this.styles = [];
      let n = this.matchedStyles = [];
      t.forEach((i) => {
        if (lc(i)) this.tags.push(i);
        else if (ac(i)) {
          let s = /[^=]*/.exec(i.style)[0];
          n.indexOf(s) < 0 && n.push(s), this.styles.push(i);
        }
      }), this.normalizeLists = !this.tags.some((i) => {
        if (!/^(ul|ol)\b/.test(i.tag) || !i.node) return false;
        let s = e.nodes[i.node];
        return s.contentMatch.matchType(s);
      });
    }
    parse(e, t = {}) {
      let n = new Nr(this, t, false);
      return n.addAll(e, T.none, t.from, t.to), n.finish();
    }
    parseSlice(e, t = {}) {
      let n = new Nr(this, t, true);
      return n.addAll(e, T.none, t.from, t.to), w.maxOpen(n.finish());
    }
    matchTag(e, t, n) {
      for (let i = n ? this.tags.indexOf(n) + 1 : 0; i < this.tags.length; i++) {
        let s = this.tags[i];
        if (dc(e, s.tag) && (s.namespace === void 0 || e.namespaceURI == s.namespace) && (!s.context || t.matchesContext(s.context))) {
          if (s.getAttrs) {
            let o = s.getAttrs(e);
            if (o === false) continue;
            s.attrs = o || void 0;
          }
          return s;
        }
      }
    }
    matchStyle(e, t, n, i) {
      for (let s = i ? this.styles.indexOf(i) + 1 : 0; s < this.styles.length; s++) {
        let o = this.styles[s], l = o.style;
        if (!(l.indexOf(e) != 0 || o.context && !n.matchesContext(o.context) || l.length > e.length && (l.charCodeAt(e.length) != 61 || l.slice(e.length + 1) != t))) {
          if (o.getAttrs) {
            let a = o.getAttrs(t);
            if (a === false) continue;
            o.attrs = a || void 0;
          }
          return o;
        }
      }
    }
    static schemaRules(e) {
      let t = [];
      function n(i) {
        let s = i.priority == null ? 50 : i.priority, o = 0;
        for (; o < t.length; o++) {
          let l = t[o];
          if ((l.priority == null ? 50 : l.priority) < s) break;
        }
        t.splice(o, 0, i);
      }
      for (let i in e.marks) {
        let s = e.marks[i].spec.parseDOM;
        s && s.forEach((o) => {
          n(o = Er(o)), o.mark || o.ignore || o.clearMark || (o.mark = i);
        });
      }
      for (let i in e.nodes) {
        let s = e.nodes[i].spec.parseDOM;
        s && s.forEach((o) => {
          n(o = Er(o)), o.node || o.ignore || o.mark || (o.node = i);
        });
      }
      return t;
    }
    static fromSchema(e) {
      return e.cached.domParser || (e.cached.domParser = new Ct(e, Ct.schemaRules(e)));
    }
  }
  const Yi = {
    address: true,
    article: true,
    aside: true,
    blockquote: true,
    canvas: true,
    dd: true,
    div: true,
    dl: true,
    fieldset: true,
    figcaption: true,
    figure: true,
    footer: true,
    form: true,
    h1: true,
    h2: true,
    h3: true,
    h4: true,
    h5: true,
    h6: true,
    header: true,
    hgroup: true,
    hr: true,
    li: true,
    noscript: true,
    ol: true,
    output: true,
    p: true,
    pre: true,
    section: true,
    table: true,
    tfoot: true,
    ul: true
  }, cc = {
    head: true,
    noscript: true,
    object: true,
    script: true,
    style: true,
    title: true
  }, Qi = {
    ol: true,
    ul: true
  }, Ot = 1, Vn = 2, yt = 4;
  function Mr(r, e, t) {
    return e != null ? (e ? Ot : 0) | (e === "full" ? Vn : 0) : r && r.whitespace == "pre" ? Ot | Vn : t & ~yt;
  }
  class Lt {
    constructor(e, t, n, i, s, o) {
      this.type = e, this.attrs = t, this.marks = n, this.solid = i, this.options = o, this.content = [], this.activeMarks = T.none, this.match = s || (o & yt ? null : e.contentMatch);
    }
    findWrapping(e) {
      if (!this.match) {
        if (!this.type) return [];
        let t = this.type.contentMatch.fillBefore(b.from(e));
        if (t) this.match = this.type.contentMatch.matchFragment(t);
        else {
          let n = this.type.contentMatch, i;
          return (i = n.findWrapping(e.type)) ? (this.match = n, i) : null;
        }
      }
      return this.match.findWrapping(e.type);
    }
    finish(e) {
      if (!(this.options & Ot)) {
        let n = this.content[this.content.length - 1], i;
        if (n && n.isText && (i = /[ \t\r\n\u000c]+$/.exec(n.text))) {
          let s = n;
          n.text.length == i[0].length ? this.content.pop() : this.content[this.content.length - 1] = s.withText(s.text.slice(0, s.text.length - i[0].length));
        }
      }
      let t = b.from(this.content);
      return !e && this.match && (t = t.append(this.match.fillBefore(b.empty, true))), this.type ? this.type.create(this.attrs, t, this.marks) : t;
    }
    inlineContext(e) {
      return this.type ? this.type.inlineContent : this.content.length ? this.content[0].isInline : e.parentNode && !Yi.hasOwnProperty(e.parentNode.nodeName.toLowerCase());
    }
  }
  class Nr {
    constructor(e, t, n) {
      this.parser = e, this.options = t, this.isOpen = n, this.open = 0, this.localPreserveWS = false;
      let i = t.topNode, s, o = Mr(null, t.preserveWhitespace, 0) | (n ? yt : 0);
      i ? s = new Lt(i.type, i.attrs, T.none, true, t.topMatch || i.type.contentMatch, o) : n ? s = new Lt(null, null, T.none, true, null, o) : s = new Lt(e.schema.topNodeType, null, T.none, true, null, o), this.nodes = [
        s
      ], this.find = t.findPositions, this.needsBlock = false;
    }
    get top() {
      return this.nodes[this.open];
    }
    addDOM(e, t) {
      e.nodeType == 3 ? this.addTextNode(e, t) : e.nodeType == 1 && this.addElement(e, t);
    }
    addTextNode(e, t) {
      let n = e.nodeValue, i = this.top, s = i.options & Vn ? "full" : this.localPreserveWS || (i.options & Ot) > 0, { schema: o } = this.parser;
      if (s === "full" || i.inlineContext(e) || /[^ \t\r\n\u000c]/.test(n)) {
        if (s) if (s === "full") n = n.replace(/\r\n?/g, `
`);
        else if (o.linebreakReplacement && /[\r\n]/.test(n) && this.top.findWrapping(o.linebreakReplacement.create())) {
          let l = n.split(/\r?\n|\r/);
          for (let a = 0; a < l.length; a++) a && this.insertNode(o.linebreakReplacement.create(), t, true), l[a] && this.insertNode(o.text(l[a]), t, !/\S/.test(l[a]));
          n = "";
        } else n = n.replace(/\r?\n|\r/g, " ");
        else if (n = n.replace(/[ \t\r\n\u000c]+/g, " "), /^[ \t\r\n\u000c]/.test(n) && this.open == this.nodes.length - 1) {
          let l = i.content[i.content.length - 1], a = e.previousSibling;
          (!l || a && a.nodeName == "BR" || l.isText && /[ \t\r\n\u000c]$/.test(l.text)) && (n = n.slice(1));
        }
        n && this.insertNode(o.text(n), t, !/\S/.test(n)), this.findInText(e);
      } else this.findInside(e);
    }
    addElement(e, t, n) {
      let i = this.localPreserveWS, s = this.top;
      (e.tagName == "PRE" || /pre/.test(e.style && e.style.whiteSpace)) && (this.localPreserveWS = true);
      let o = e.nodeName.toLowerCase(), l;
      Qi.hasOwnProperty(o) && this.parser.normalizeLists && fc(e);
      let a = this.options.ruleFromNode && this.options.ruleFromNode(e) || (l = this.parser.matchTag(e, this, n));
      e: if (a ? a.ignore : cc.hasOwnProperty(o)) this.findInside(e), this.ignoreFallback(e, t);
      else if (!a || a.skip || a.closeParent) {
        a && a.closeParent ? this.open = Math.max(0, this.open - 1) : a && a.skip.nodeType && (e = a.skip);
        let c, f = this.needsBlock;
        if (Yi.hasOwnProperty(o)) s.content.length && s.content[0].isInline && this.open && (this.open--, s = this.top), c = true, s.type || (this.needsBlock = true);
        else if (!e.firstChild) {
          this.leafFallback(e, t);
          break e;
        }
        let d = a && a.skip ? t : this.readStyles(e, t);
        d && this.addAll(e, d), c && this.sync(s), this.needsBlock = f;
      } else {
        let c = this.readStyles(e, t);
        c && this.addElementByRule(e, a, c, a.consuming === false ? l : void 0);
      }
      this.localPreserveWS = i;
    }
    leafFallback(e, t) {
      e.nodeName == "BR" && this.top.type && this.top.type.inlineContent && this.addTextNode(e.ownerDocument.createTextNode(`
`), t);
    }
    ignoreFallback(e, t) {
      e.nodeName == "BR" && (!this.top.type || !this.top.type.inlineContent) && this.findPlace(this.parser.schema.text("-"), t, true);
    }
    readStyles(e, t) {
      let n = e.style;
      if (n && n.length) for (let i = 0; i < this.parser.matchedStyles.length; i++) {
        let s = this.parser.matchedStyles[i], o = n.getPropertyValue(s);
        if (o) for (let l = void 0; ; ) {
          let a = this.parser.matchStyle(s, o, this, l);
          if (!a) break;
          if (a.ignore) return null;
          if (a.clearMark ? t = t.filter((c) => !a.clearMark(c)) : t = t.concat(this.parser.schema.marks[a.mark].create(a.attrs)), a.consuming === false) l = a;
          else break;
        }
      }
      return t;
    }
    addElementByRule(e, t, n, i) {
      let s, o;
      if (t.node) if (o = this.parser.schema.nodes[t.node], o.isLeaf) this.insertNode(o.create(t.attrs), n, e.nodeName == "BR") || this.leafFallback(e, n);
      else {
        let a = this.enter(o, t.attrs || null, n, t.preserveWhitespace);
        a && (s = true, n = a);
      }
      else {
        let a = this.parser.schema.marks[t.mark];
        n = n.concat(a.create(t.attrs));
      }
      let l = this.top;
      if (o && o.isLeaf) this.findInside(e);
      else if (i) this.addElement(e, n, i);
      else if (t.getContent) this.findInside(e), t.getContent(e, this.parser.schema).forEach((a) => this.insertNode(a, n, false));
      else {
        let a = e;
        typeof t.contentElement == "string" ? a = e.querySelector(t.contentElement) : typeof t.contentElement == "function" ? a = t.contentElement(e) : t.contentElement && (a = t.contentElement), this.findAround(e, a, true), this.addAll(a, n), this.findAround(e, a, false);
      }
      s && this.sync(l) && this.open--;
    }
    addAll(e, t, n, i) {
      let s = n || 0;
      for (let o = n ? e.childNodes[n] : e.firstChild, l = i == null ? null : e.childNodes[i]; o != l; o = o.nextSibling, ++s) this.findAtPoint(e, s), this.addDOM(o, t);
      this.findAtPoint(e, s);
    }
    findPlace(e, t, n) {
      let i, s;
      for (let o = this.open, l = 0; o >= 0; o--) {
        let a = this.nodes[o], c = a.findWrapping(e);
        if (c && (!i || i.length > c.length + l) && (i = c, s = a, !c.length)) break;
        if (a.solid) {
          if (n) break;
          l += 2;
        }
      }
      if (!i) return null;
      this.sync(s);
      for (let o = 0; o < i.length; o++) t = this.enterInner(i[o], null, t, false);
      return t;
    }
    insertNode(e, t, n) {
      if (e.isInline && this.needsBlock && !this.top.type) {
        let s = this.textblockFromContext();
        s && (t = this.enterInner(s, null, t));
      }
      let i = this.findPlace(e, t, n);
      if (i) {
        this.closeExtra();
        let s = this.top;
        s.match && (s.match = s.match.matchType(e.type));
        let o = T.none;
        for (let l of i.concat(e.marks)) (s.type ? s.type.allowsMarkType(l.type) : Tr(l.type, e.type)) && (o = l.addToSet(o));
        return s.content.push(e.mark(o)), true;
      }
      return false;
    }
    enter(e, t, n, i) {
      let s = this.findPlace(e.create(t), n, false);
      return s && (s = this.enterInner(e, t, n, true, i)), s;
    }
    enterInner(e, t, n, i = false, s) {
      this.closeExtra();
      let o = this.top;
      o.match = o.match && o.match.matchType(e);
      let l = Mr(e, s, o.options);
      o.options & yt && o.content.length == 0 && (l |= yt);
      let a = T.none;
      return n = n.filter((c) => (o.type ? o.type.allowsMarkType(c.type) : Tr(c.type, e)) ? (a = c.addToSet(a), false) : true), this.nodes.push(new Lt(e, t, a, i, null, l)), this.open++, n;
    }
    closeExtra(e = false) {
      let t = this.nodes.length - 1;
      if (t > this.open) {
        for (; t > this.open; t--) this.nodes[t - 1].content.push(this.nodes[t].finish(e));
        this.nodes.length = this.open + 1;
      }
    }
    finish() {
      return this.open = 0, this.closeExtra(this.isOpen), this.nodes[0].finish(!!(this.isOpen || this.options.topOpen));
    }
    sync(e) {
      for (let t = this.open; t >= 0; t--) {
        if (this.nodes[t] == e) return this.open = t, true;
        this.localPreserveWS && (this.nodes[t].options |= Ot);
      }
      return false;
    }
    get currentPos() {
      this.closeExtra();
      let e = 0;
      for (let t = this.open; t >= 0; t--) {
        let n = this.nodes[t].content;
        for (let i = n.length - 1; i >= 0; i--) e += n[i].nodeSize;
        t && e++;
      }
      return e;
    }
    findAtPoint(e, t) {
      if (this.find) for (let n = 0; n < this.find.length; n++) this.find[n].node == e && this.find[n].offset == t && (this.find[n].pos = this.currentPos);
    }
    findInside(e) {
      if (this.find) for (let t = 0; t < this.find.length; t++) this.find[t].pos == null && e.nodeType == 1 && e.contains(this.find[t].node) && (this.find[t].pos = this.currentPos);
    }
    findAround(e, t, n) {
      if (e != t && this.find) for (let i = 0; i < this.find.length; i++) this.find[i].pos == null && e.nodeType == 1 && e.contains(this.find[i].node) && t.compareDocumentPosition(this.find[i].node) & (n ? 2 : 4) && (this.find[i].pos = this.currentPos);
    }
    findInText(e) {
      if (this.find) for (let t = 0; t < this.find.length; t++) this.find[t].node == e && (this.find[t].pos = this.currentPos - (e.nodeValue.length - this.find[t].offset));
    }
    matchesContext(e) {
      if (e.indexOf("|") > -1) return e.split(/\s*\|\s*/).some(this.matchesContext, this);
      let t = e.split("/"), n = this.options.context, i = !this.isOpen && (!n || n.parent.type == this.nodes[0].type), s = -(n ? n.depth + 1 : 0) + (i ? 0 : 1), o = (l, a) => {
        for (; l >= 0; l--) {
          let c = t[l];
          if (c == "") {
            if (l == t.length - 1 || l == 0) continue;
            for (; a >= s; a--) if (o(l - 1, a)) return true;
            return false;
          } else {
            let f = a > 0 || a == 0 && i ? this.nodes[a].type : n && a >= s ? n.node(a - s).type : null;
            if (!f || f.name != c && !f.isInGroup(c)) return false;
            a--;
          }
        }
        return true;
      };
      return o(t.length - 1, this.open);
    }
    textblockFromContext() {
      let e = this.options.context;
      if (e) for (let t = e.depth; t >= 0; t--) {
        let n = e.node(t).contentMatchAt(e.indexAfter(t)).defaultType;
        if (n && n.isTextblock && n.defaultAttrs) return n;
      }
      for (let t in this.parser.schema.nodes) {
        let n = this.parser.schema.nodes[t];
        if (n.isTextblock && n.defaultAttrs) return n;
      }
    }
  }
  function fc(r) {
    for (let e = r.firstChild, t = null; e; e = e.nextSibling) {
      let n = e.nodeType == 1 ? e.nodeName.toLowerCase() : null;
      n && Qi.hasOwnProperty(n) && t ? (t.appendChild(e), e = t) : n == "li" ? t = e : n && (t = null);
    }
  }
  function dc(r, e) {
    return (r.matches || r.msMatchesSelector || r.webkitMatchesSelector || r.mozMatchesSelector).call(r, e);
  }
  function Er(r) {
    let e = {};
    for (let t in r) e[t] = r[t];
    return e;
  }
  function Tr(r, e) {
    let t = e.schema.nodes;
    for (let n in t) {
      let i = t[n];
      if (!i.allowsMarkType(r)) continue;
      let s = [], o = (l) => {
        s.push(l);
        for (let a = 0; a < l.edgeCount; a++) {
          let { type: c, next: f } = l.edge(a);
          if (c == e || s.indexOf(f) < 0 && o(f)) return true;
        }
      };
      if (o(i.contentMatch)) return true;
    }
  }
  class at {
    constructor(e, t) {
      this.nodes = e, this.marks = t;
    }
    serializeFragment(e, t = {}, n) {
      n || (n = qt(t).createDocumentFragment());
      let i = n, s = [];
      return e.forEach((o) => {
        if (s.length || o.marks.length) {
          let l = 0, a = 0;
          for (; l < s.length && a < o.marks.length; ) {
            let c = o.marks[a];
            if (!this.marks[c.type.name]) {
              a++;
              continue;
            }
            if (!c.eq(s[l][0]) || c.type.spec.spanning === false) break;
            l++, a++;
          }
          for (; l < s.length; ) i = s.pop()[1];
          for (; a < o.marks.length; ) {
            let c = o.marks[a++], f = this.serializeMark(c, o.isInline, t);
            f && (s.push([
              c,
              i
            ]), i.appendChild(f.dom), i = f.contentDOM || f.dom);
          }
        }
        i.appendChild(this.serializeNodeInner(o, t));
      }), n;
    }
    serializeNodeInner(e, t) {
      if (e.isText) return qt(t).createTextNode(e.text);
      let { dom: n, contentDOM: i } = jt(qt(t), this.nodes[e.type.name](e), null, e.attrs);
      if (i) {
        if (e.isLeaf) throw new RangeError("Content hole not allowed in a leaf node spec");
        this.serializeFragment(e.content, t, i);
      }
      return n;
    }
    serializeNode(e, t = {}) {
      let n = this.serializeNodeInner(e, t);
      for (let i = e.marks.length - 1; i >= 0; i--) {
        let s = this.serializeMark(e.marks[i], e.isInline, t);
        s && ((s.contentDOM || s.dom).appendChild(n), n = s.dom);
      }
      return n;
    }
    serializeMark(e, t, n = {}) {
      let i = this.marks[e.type.name];
      return i && jt(qt(n), i(e, t), null, e.attrs);
    }
    static renderSpec(e, t, n = null, i) {
      return typeof t == "string" ? {
        dom: e.createTextNode(t)
      } : jt(e, t, n, i);
    }
    static fromSchema(e) {
      return e.cached.domSerializer || (e.cached.domSerializer = new at(this.nodesFromSchema(e), this.marksFromSchema(e)));
    }
    static nodesFromSchema(e) {
      let t = Dr(e.nodes);
      return t.text || (t.text = (n) => n.text), t;
    }
    static marksFromSchema(e) {
      return Dr(e.marks);
    }
  }
  function Dr(r) {
    let e = {};
    for (let t in r) {
      let n = r[t].spec.toDOM;
      n && (e[t] = n);
    }
    return e;
  }
  function qt(r) {
    return r.document || window.document;
  }
  const Ir = /* @__PURE__ */ new WeakMap();
  function uc(r) {
    let e = Ir.get(r);
    return e === void 0 && Ir.set(r, e = hc(r)), e;
  }
  function hc(r) {
    let e = null;
    function t(n) {
      if (n && typeof n == "object") if (Array.isArray(n)) if (typeof n[0] == "string") e || (e = []), e.push(n);
      else for (let i = 0; i < n.length; i++) t(n[i]);
      else for (let i in n) t(n[i]);
    }
    return t(r), e;
  }
  function jt(r, e, t, n) {
    if (e.nodeType == 1) return {
      dom: e
    };
    if (e.dom && e.dom.nodeType == 1) return e;
    let i = e[0], s;
    if (typeof i != "string") throw new RangeError("Invalid array passed to renderSpec");
    if (n && (s = uc(n)) && s.indexOf(e) > -1) throw new RangeError("Using an array from an attribute object as a DOM spec. This may be an attempted cross site scripting attack.");
    let o = i.indexOf(" ");
    o > 0 && (t = i.slice(0, o), i = i.slice(o + 1));
    let l, a = t ? r.createElementNS(t, i) : r.createElement(i), c = e[1], f = 1;
    if (c && typeof c == "object" && c.nodeType == null && !Array.isArray(c)) {
      f = 2;
      for (let d in c) if (c[d] != null) {
        let u = d.indexOf(" ");
        u > 0 ? a.setAttributeNS(d.slice(0, u), d.slice(u + 1), c[d]) : d == "style" && a.style ? a.style.cssText = c[d] : a.setAttribute(d, c[d]);
      }
    }
    for (let d = f; d < e.length; d++) {
      let u = e[d];
      if (u === 0) {
        if (d < e.length - 1 || d > f) throw new RangeError("Content hole must be the only child of its parent node");
        return {
          dom: a,
          contentDOM: a
        };
      } else if (typeof u == "string") a.appendChild(r.createTextNode(u));
      else {
        let { dom: m, contentDOM: p } = jt(r, u, t, n);
        if (a.appendChild(m), p) {
          if (l) throw new RangeError("Multiple content holes");
          l = p;
        }
      }
    }
    return {
      dom: a,
      contentDOM: l
    };
  }
  const Xi = 65535, Zi = Math.pow(2, 16);
  function pc(r, e) {
    return r + e * Zi;
  }
  function Ar(r) {
    return r & Xi;
  }
  function mc(r) {
    return (r - (r & Xi)) / Zi;
  }
  const es = 1, ts = 2, Kt = 4, ns = 8;
  class Ln {
    constructor(e, t, n) {
      this.pos = e, this.delInfo = t, this.recover = n;
    }
    get deleted() {
      return (this.delInfo & ns) > 0;
    }
    get deletedBefore() {
      return (this.delInfo & (es | Kt)) > 0;
    }
    get deletedAfter() {
      return (this.delInfo & (ts | Kt)) > 0;
    }
    get deletedAcross() {
      return (this.delInfo & Kt) > 0;
    }
  }
  class ee {
    constructor(e, t = false) {
      if (this.ranges = e, this.inverted = t, !e.length && ee.empty) return ee.empty;
    }
    recover(e) {
      let t = 0, n = Ar(e);
      if (!this.inverted) for (let i = 0; i < n; i++) t += this.ranges[i * 3 + 2] - this.ranges[i * 3 + 1];
      return this.ranges[n * 3] + t + mc(e);
    }
    mapResult(e, t = 1) {
      return this._map(e, t, false);
    }
    map(e, t = 1) {
      return this._map(e, t, true);
    }
    _map(e, t, n) {
      let i = 0, s = this.inverted ? 2 : 1, o = this.inverted ? 1 : 2;
      for (let l = 0; l < this.ranges.length; l += 3) {
        let a = this.ranges[l] - (this.inverted ? i : 0);
        if (a > e) break;
        let c = this.ranges[l + s], f = this.ranges[l + o], d = a + c;
        if (e <= d) {
          let u = c ? e == a ? -1 : e == d ? 1 : t : t, m = a + i + (u < 0 ? 0 : f);
          if (n) return m;
          let p = e == (t < 0 ? a : d) ? null : pc(l / 3, e - a), _ = e == a ? ts : e == d ? es : Kt;
          return (t < 0 ? e != a : e != d) && (_ |= ns), new Ln(m, _, p);
        }
        i += f - c;
      }
      return n ? e + i : new Ln(e + i, 0, null);
    }
    touches(e, t) {
      let n = 0, i = Ar(t), s = this.inverted ? 2 : 1, o = this.inverted ? 1 : 2;
      for (let l = 0; l < this.ranges.length; l += 3) {
        let a = this.ranges[l] - (this.inverted ? n : 0);
        if (a > e) break;
        let c = this.ranges[l + s], f = a + c;
        if (e <= f && l == i * 3) return true;
        n += this.ranges[l + o] - c;
      }
      return false;
    }
    forEach(e) {
      let t = this.inverted ? 2 : 1, n = this.inverted ? 1 : 2;
      for (let i = 0, s = 0; i < this.ranges.length; i += 3) {
        let o = this.ranges[i], l = o - (this.inverted ? s : 0), a = o + (this.inverted ? 0 : s), c = this.ranges[i + t], f = this.ranges[i + n];
        e(l, l + c, a, a + f), s += f - c;
      }
    }
    invert() {
      return new ee(this.ranges, !this.inverted);
    }
    toString() {
      return (this.inverted ? "-" : "") + JSON.stringify(this.ranges);
    }
    static offset(e) {
      return e == 0 ? ee.empty : new ee(e < 0 ? [
        0,
        -e,
        0
      ] : [
        0,
        0,
        e
      ]);
    }
  }
  ee.empty = new ee([]);
  class Mt {
    constructor(e, t, n = 0, i = e ? e.length : 0) {
      this.mirror = t, this.from = n, this.to = i, this._maps = e || [], this.ownData = !(e || t);
    }
    get maps() {
      return this._maps;
    }
    slice(e = 0, t = this.maps.length) {
      return new Mt(this._maps, this.mirror, e, t);
    }
    appendMap(e, t) {
      this.ownData || (this._maps = this._maps.slice(), this.mirror = this.mirror && this.mirror.slice(), this.ownData = true), this.to = this._maps.push(e), t != null && this.setMirror(this._maps.length - 1, t);
    }
    appendMapping(e) {
      for (let t = 0, n = this._maps.length; t < e._maps.length; t++) {
        let i = e.getMirror(t);
        this.appendMap(e._maps[t], i != null && i < t ? n + i : void 0);
      }
    }
    getMirror(e) {
      if (this.mirror) {
        for (let t = 0; t < this.mirror.length; t++) if (this.mirror[t] == e) return this.mirror[t + (t % 2 ? -1 : 1)];
      }
    }
    setMirror(e, t) {
      this.mirror || (this.mirror = []), this.mirror.push(e, t);
    }
    appendMappingInverted(e) {
      for (let t = e.maps.length - 1, n = this._maps.length + e._maps.length; t >= 0; t--) {
        let i = e.getMirror(t);
        this.appendMap(e._maps[t].invert(), i != null && i > t ? n - i - 1 : void 0);
      }
    }
    invert() {
      let e = new Mt();
      return e.appendMappingInverted(this), e;
    }
    map(e, t = 1) {
      if (this.mirror) return this._map(e, t, true);
      for (let n = this.from; n < this.to; n++) e = this._maps[n].map(e, t);
      return e;
    }
    mapResult(e, t = 1) {
      return this._map(e, t, false);
    }
    _map(e, t, n) {
      let i = 0;
      for (let s = this.from; s < this.to; s++) {
        let o = this._maps[s], l = o.mapResult(e, t);
        if (l.recover != null) {
          let a = this.getMirror(s);
          if (a != null && a > s && a < this.to) {
            s = a, e = this._maps[a].recover(l.recover);
            continue;
          }
        }
        i |= l.delInfo, e = l.pos;
      }
      return n ? e : new Ln(e, i, null);
    }
  }
  const gn = /* @__PURE__ */ Object.create(null);
  class H {
    getMap() {
      return ee.empty;
    }
    merge(e) {
      return null;
    }
    static fromJSON(e, t) {
      if (!t || !t.stepType) throw new RangeError("Invalid input for Step.fromJSON");
      let n = gn[t.stepType];
      if (!n) throw new RangeError(`No step type ${t.stepType} defined`);
      return n.fromJSON(e, t);
    }
    static jsonID(e, t) {
      if (e in gn) throw new RangeError("Duplicate use of step JSON ID " + e);
      return gn[e] = t, t.prototype.jsonID = e, t;
    }
  }
  class v {
    constructor(e, t) {
      this.doc = e, this.failed = t;
    }
    static ok(e) {
      return new v(e, null);
    }
    static fail(e) {
      return new v(null, e);
    }
    static fromReplace(e, t, n, i) {
      try {
        return v.ok(e.replace(t, n, i));
      } catch (s) {
        if (s instanceof xt) return v.fail(s.message);
        throw s;
      }
    }
  }
  function nr(r, e, t) {
    let n = [];
    for (let i = 0; i < r.childCount; i++) {
      let s = r.child(i);
      s.content.size && (s = s.copy(nr(s.content, e, s))), s.isInline && (s = e(s, t, i)), n.push(s);
    }
    return b.fromArray(n);
  }
  class Ee extends H {
    constructor(e, t, n) {
      super(), this.from = e, this.to = t, this.mark = n;
    }
    apply(e) {
      let t = e.slice(this.from, this.to), n = e.resolve(this.from), i = n.node(n.sharedDepth(this.to)), s = new w(nr(t.content, (o, l) => !o.isAtom || !l.type.allowsMarkType(this.mark.type) ? o : o.mark(this.mark.addToSet(o.marks)), i), t.openStart, t.openEnd);
      return v.fromReplace(e, this.from, this.to, s);
    }
    invert() {
      return new pe(this.from, this.to, this.mark);
    }
    map(e) {
      let t = e.mapResult(this.from, 1), n = e.mapResult(this.to, -1);
      return t.deleted && n.deleted || t.pos >= n.pos ? null : new Ee(t.pos, n.pos, this.mark);
    }
    merge(e) {
      return e instanceof Ee && e.mark.eq(this.mark) && this.from <= e.to && this.to >= e.from ? new Ee(Math.min(this.from, e.from), Math.max(this.to, e.to), this.mark) : null;
    }
    toJSON() {
      return {
        stepType: "addMark",
        mark: this.mark.toJSON(),
        from: this.from,
        to: this.to
      };
    }
    static fromJSON(e, t) {
      if (typeof t.from != "number" || typeof t.to != "number") throw new RangeError("Invalid input for AddMarkStep.fromJSON");
      return new Ee(t.from, t.to, e.markFromJSON(t.mark));
    }
  }
  H.jsonID("addMark", Ee);
  class pe extends H {
    constructor(e, t, n) {
      super(), this.from = e, this.to = t, this.mark = n;
    }
    apply(e) {
      let t = e.slice(this.from, this.to), n = new w(nr(t.content, (i) => i.mark(this.mark.removeFromSet(i.marks)), e), t.openStart, t.openEnd);
      return v.fromReplace(e, this.from, this.to, n);
    }
    invert() {
      return new Ee(this.from, this.to, this.mark);
    }
    map(e) {
      let t = e.mapResult(this.from, 1), n = e.mapResult(this.to, -1);
      return t.deleted && n.deleted || t.pos >= n.pos ? null : new pe(t.pos, n.pos, this.mark);
    }
    merge(e) {
      return e instanceof pe && e.mark.eq(this.mark) && this.from <= e.to && this.to >= e.from ? new pe(Math.min(this.from, e.from), Math.max(this.to, e.to), this.mark) : null;
    }
    toJSON() {
      return {
        stepType: "removeMark",
        mark: this.mark.toJSON(),
        from: this.from,
        to: this.to
      };
    }
    static fromJSON(e, t) {
      if (typeof t.from != "number" || typeof t.to != "number") throw new RangeError("Invalid input for RemoveMarkStep.fromJSON");
      return new pe(t.from, t.to, e.markFromJSON(t.mark));
    }
  }
  H.jsonID("removeMark", pe);
  class Te extends H {
    constructor(e, t) {
      super(), this.pos = e, this.mark = t;
    }
    apply(e) {
      let t = e.nodeAt(this.pos);
      if (!t) return v.fail("No node at mark step's position");
      let n = t.type.create(t.attrs, null, this.mark.addToSet(t.marks));
      return v.fromReplace(e, this.pos, this.pos + 1, new w(b.from(n), 0, t.isLeaf ? 0 : 1));
    }
    invert(e) {
      let t = e.nodeAt(this.pos);
      if (t) {
        let n = this.mark.addToSet(t.marks);
        if (n.length == t.marks.length) {
          for (let i = 0; i < t.marks.length; i++) if (!t.marks[i].isInSet(n)) return new Te(this.pos, t.marks[i]);
          return new Te(this.pos, this.mark);
        }
      }
      return new Ge(this.pos, this.mark);
    }
    map(e) {
      let t = e.mapResult(this.pos, 1);
      return t.deletedAfter ? null : new Te(t.pos, this.mark);
    }
    toJSON() {
      return {
        stepType: "addNodeMark",
        pos: this.pos,
        mark: this.mark.toJSON()
      };
    }
    static fromJSON(e, t) {
      if (typeof t.pos != "number") throw new RangeError("Invalid input for AddNodeMarkStep.fromJSON");
      return new Te(t.pos, e.markFromJSON(t.mark));
    }
  }
  H.jsonID("addNodeMark", Te);
  class Ge extends H {
    constructor(e, t) {
      super(), this.pos = e, this.mark = t;
    }
    apply(e) {
      let t = e.nodeAt(this.pos);
      if (!t) return v.fail("No node at mark step's position");
      let n = t.type.create(t.attrs, null, this.mark.removeFromSet(t.marks));
      return v.fromReplace(e, this.pos, this.pos + 1, new w(b.from(n), 0, t.isLeaf ? 0 : 1));
    }
    invert(e) {
      let t = e.nodeAt(this.pos);
      return !t || !this.mark.isInSet(t.marks) ? this : new Te(this.pos, this.mark);
    }
    map(e) {
      let t = e.mapResult(this.pos, 1);
      return t.deletedAfter ? null : new Ge(t.pos, this.mark);
    }
    toJSON() {
      return {
        stepType: "removeNodeMark",
        pos: this.pos,
        mark: this.mark.toJSON()
      };
    }
    static fromJSON(e, t) {
      if (typeof t.pos != "number") throw new RangeError("Invalid input for RemoveNodeMarkStep.fromJSON");
      return new Ge(t.pos, e.markFromJSON(t.mark));
    }
  }
  H.jsonID("removeNodeMark", Ge);
  class B extends H {
    constructor(e, t, n, i = false) {
      super(), this.from = e, this.to = t, this.slice = n, this.structure = i;
    }
    apply(e) {
      return this.structure && qn(e, this.from, this.to) ? v.fail("Structure replace would overwrite content") : v.fromReplace(e, this.from, this.to, this.slice);
    }
    getMap() {
      return new ee([
        this.from,
        this.to - this.from,
        this.slice.size
      ]);
    }
    invert(e) {
      return new B(this.from, this.from + this.slice.size, e.slice(this.from, this.to));
    }
    map(e) {
      let t = e.mapResult(this.to, -1), n = this.from == this.to && B.MAP_BIAS < 0 ? t : e.mapResult(this.from, 1);
      return n.deletedAcross && t.deletedAcross ? null : new B(n.pos, Math.max(n.pos, t.pos), this.slice, this.structure);
    }
    merge(e) {
      if (!(e instanceof B) || e.structure || this.structure) return null;
      if (this.from + this.slice.size == e.from && !this.slice.openEnd && !e.slice.openStart) {
        let t = this.slice.size + e.slice.size == 0 ? w.empty : new w(this.slice.content.append(e.slice.content), this.slice.openStart, e.slice.openEnd);
        return new B(this.from, this.to + (e.to - e.from), t, this.structure);
      } else if (e.to == this.from && !this.slice.openStart && !e.slice.openEnd) {
        let t = this.slice.size + e.slice.size == 0 ? w.empty : new w(e.slice.content.append(this.slice.content), e.slice.openStart, this.slice.openEnd);
        return new B(e.from, this.to, t, this.structure);
      } else return null;
    }
    toJSON() {
      let e = {
        stepType: "replace",
        from: this.from,
        to: this.to
      };
      return this.slice.size && (e.slice = this.slice.toJSON()), this.structure && (e.structure = true), e;
    }
    static fromJSON(e, t) {
      if (typeof t.from != "number" || typeof t.to != "number") throw new RangeError("Invalid input for ReplaceStep.fromJSON");
      return new B(t.from, t.to, w.fromJSON(e, t.slice), !!t.structure);
    }
  }
  B.MAP_BIAS = 1;
  H.jsonID("replace", B);
  class te extends H {
    constructor(e, t, n, i, s, o, l = false) {
      super(), this.from = e, this.to = t, this.gapFrom = n, this.gapTo = i, this.slice = s, this.insert = o, this.structure = l;
    }
    apply(e) {
      if (this.structure && (qn(e, this.from, this.gapFrom) || qn(e, this.gapTo, this.to))) return v.fail("Structure gap-replace would overwrite content");
      let t = e.slice(this.gapFrom, this.gapTo);
      if (t.openStart || t.openEnd) return v.fail("Gap is not a flat range");
      let n = this.slice.insertAt(this.insert, t.content);
      return n ? v.fromReplace(e, this.from, this.to, n) : v.fail("Content does not fit in gap");
    }
    getMap() {
      return new ee([
        this.from,
        this.gapFrom - this.from,
        this.insert,
        this.gapTo,
        this.to - this.gapTo,
        this.slice.size - this.insert
      ]);
    }
    invert(e) {
      let t = this.gapTo - this.gapFrom;
      return new te(this.from, this.from + this.slice.size + t, this.from + this.insert, this.from + this.insert + t, e.slice(this.from, this.to).removeBetween(this.gapFrom - this.from, this.gapTo - this.from), this.gapFrom - this.from, this.structure);
    }
    map(e) {
      let t = e.mapResult(this.from, 1), n = e.mapResult(this.to, -1), i = this.from == this.gapFrom ? t.pos : e.map(this.gapFrom, -1), s = this.to == this.gapTo ? n.pos : e.map(this.gapTo, 1);
      return t.deletedAcross && n.deletedAcross || i < t.pos || s > n.pos ? null : new te(t.pos, n.pos, i, s, this.slice, this.insert, this.structure);
    }
    toJSON() {
      let e = {
        stepType: "replaceAround",
        from: this.from,
        to: this.to,
        gapFrom: this.gapFrom,
        gapTo: this.gapTo,
        insert: this.insert
      };
      return this.slice.size && (e.slice = this.slice.toJSON()), this.structure && (e.structure = true), e;
    }
    static fromJSON(e, t) {
      if (typeof t.from != "number" || typeof t.to != "number" || typeof t.gapFrom != "number" || typeof t.gapTo != "number" || typeof t.insert != "number") throw new RangeError("Invalid input for ReplaceAroundStep.fromJSON");
      return new te(t.from, t.to, t.gapFrom, t.gapTo, w.fromJSON(e, t.slice), t.insert, !!t.structure);
    }
  }
  H.jsonID("replaceAround", te);
  function qn(r, e, t) {
    let n = r.resolve(e), i = t - e, s = n.depth;
    for (; i > 0 && s > 0 && n.indexAfter(s) == n.node(s).childCount; ) s--, i--;
    if (i > 0) {
      let o = n.node(s).maybeChild(n.indexAfter(s));
      for (; i > 0; ) {
        if (!o || o.isLeaf) return true;
        o = o.firstChild, i--;
      }
    }
    return false;
  }
  function gc(r, e, t, n) {
    let i = [], s = [], o, l;
    r.doc.nodesBetween(e, t, (a, c, f) => {
      if (!a.isInline) return;
      let d = a.marks;
      if (!n.isInSet(d) && f.type.allowsMarkType(n.type)) {
        let u = Math.max(c, e), m = Math.min(c + a.nodeSize, t), p = n.addToSet(d);
        for (let _ = 0; _ < d.length; _++) d[_].isInSet(p) || (o && o.to == u && o.mark.eq(d[_]) ? o.to = m : i.push(o = new pe(u, m, d[_])));
        l && l.to == u ? l.to = m : s.push(l = new Ee(u, m, n));
      }
    }), i.forEach((a) => r.step(a)), s.forEach((a) => r.step(a));
  }
  function _c(r, e, t, n) {
    let i = [], s = 0;
    r.doc.nodesBetween(e, t, (o, l) => {
      if (!o.isInline) return;
      s++;
      let a = null;
      if (n instanceof an) {
        let c = o.marks, f;
        for (; f = n.isInSet(c); ) (a || (a = [])).push(f), c = f.removeFromSet(c);
      } else n ? n.isInSet(o.marks) && (a = [
        n
      ]) : a = o.marks;
      if (a && a.length) {
        let c = Math.min(l + o.nodeSize, t);
        for (let f = 0; f < a.length; f++) {
          let d = a[f], u;
          for (let m = 0; m < i.length; m++) {
            let p = i[m];
            p.step == s - 1 && d.eq(i[m].style) && (u = p);
          }
          u ? (u.to = c, u.step = s) : i.push({
            style: d,
            from: Math.max(l, e),
            to: c,
            step: s
          });
        }
      }
    }), i.forEach((o) => r.step(new pe(o.from, o.to, o.style)));
  }
  function rr(r, e, t, n = t.contentMatch, i = true) {
    let s = r.doc.nodeAt(e), o = [], l = e + 1;
    for (let a = 0; a < s.childCount; a++) {
      let c = s.child(a), f = l + c.nodeSize, d = n.matchType(c.type);
      if (!d) o.push(new B(l, f, w.empty));
      else {
        n = d;
        for (let u = 0; u < c.marks.length; u++) t.allowsMarkType(c.marks[u].type) || r.step(new pe(l, f, c.marks[u]));
        if (i && c.isText && t.whitespace != "pre") {
          let u, m = /\r?\n|\r/g, p;
          for (; u = m.exec(c.text); ) p || (p = new w(b.from(t.schema.text(" ", t.allowedMarks(c.marks))), 0, 0)), o.push(new B(l + u.index, l + u.index + u[0].length, p));
        }
      }
      l = f;
    }
    if (!n.validEnd) {
      let a = n.fillBefore(b.empty, true);
      r.replace(l, l, new w(a, 0, 0));
    }
    for (let a = o.length - 1; a >= 0; a--) r.step(o[a]);
  }
  function yc(r, e, t) {
    return (e == 0 || r.canReplace(e, r.childCount)) && (t == r.childCount || r.canReplace(0, t));
  }
  function ir(r) {
    let t = r.parent.content.cutByIndex(r.startIndex, r.endIndex);
    for (let n = r.depth, i = 0, s = 0; ; --n) {
      let o = r.$from.node(n), l = r.$from.index(n) + i, a = r.$to.indexAfter(n) - s;
      if (n < r.depth && o.canReplace(l, a, t)) return n;
      if (n == 0 || o.type.spec.isolating || !yc(o, l, a)) break;
      l && (i = 1), a < o.childCount && (s = 1);
    }
    return null;
  }
  function bc(r, e, t) {
    let { $from: n, $to: i, depth: s } = e, o = n.before(s + 1), l = i.after(s + 1), a = o, c = l, f = b.empty, d = 0;
    for (let p = s, _ = false; p > t; p--) _ || n.index(p) > 0 ? (_ = true, f = b.from(n.node(p).copy(f)), d++) : a--;
    let u = b.empty, m = 0;
    for (let p = s, _ = false; p > t; p--) _ || i.after(p + 1) < i.end(p) ? (_ = true, u = b.from(i.node(p).copy(u)), m++) : c++;
    r.step(new te(a, c, o, l, new w(f.append(u), d, m), f.size - d, true));
  }
  function wc(r, e, t) {
    let n = b.empty;
    for (let o = t.length - 1; o >= 0; o--) {
      if (n.size) {
        let l = t[o].type.contentMatch.matchFragment(n);
        if (!l || !l.validEnd) throw new RangeError("Wrapper type given to Transform.wrap does not form valid content of its parent wrapper");
      }
      n = b.from(t[o].type.create(t[o].attrs, n));
    }
    let i = e.start, s = e.end;
    r.step(new te(i, s, i, s, new w(n, 0, 0), t.length, true));
  }
  function kc(r, e, t, n, i) {
    if (!n.isTextblock) throw new RangeError("Type given to setBlockType should be a textblock");
    let s = r.steps.length;
    r.doc.nodesBetween(e, t, (o, l) => {
      let a = typeof i == "function" ? i(o) : i;
      if (o.isTextblock && !o.hasMarkup(n, a) && xc(r.doc, r.mapping.slice(s).map(l), n)) {
        let c = null;
        if (n.schema.linebreakReplacement) {
          let m = n.whitespace == "pre", p = !!n.contentMatch.matchType(n.schema.linebreakReplacement);
          m && !p ? c = false : !m && p && (c = true);
        }
        c === false && is(r, o, l, s), rr(r, r.mapping.slice(s).map(l, 1), n, void 0, c === null);
        let f = r.mapping.slice(s), d = f.map(l, 1), u = f.map(l + o.nodeSize, 1);
        return r.step(new te(d, u, d + 1, u - 1, new w(b.from(n.create(a, null, o.marks)), 0, 0), 1, true)), c === true && rs(r, o, l, s), false;
      }
    });
  }
  function rs(r, e, t, n) {
    e.forEach((i, s) => {
      if (i.isText) {
        let o, l = /\r?\n|\r/g;
        for (; o = l.exec(i.text); ) {
          let a = r.mapping.slice(n).map(t + 1 + s + o.index);
          r.replaceWith(a, a + 1, e.type.schema.linebreakReplacement.create());
        }
      }
    });
  }
  function is(r, e, t, n) {
    e.forEach((i, s) => {
      if (i.type == i.type.schema.linebreakReplacement) {
        let o = r.mapping.slice(n).map(t + 1 + s);
        r.replaceWith(o, o + 1, e.type.schema.text(`
`));
      }
    });
  }
  function xc(r, e, t) {
    let n = r.resolve(e), i = n.index();
    return n.parent.canReplaceWith(i, i + 1, t);
  }
  function Sc(r, e, t, n, i) {
    let s = r.doc.nodeAt(e);
    if (!s) throw new RangeError("No node at given position");
    t || (t = s.type);
    let o = t.create(n, null, i || s.marks);
    if (s.isLeaf) return r.replaceWith(e, e + s.nodeSize, o);
    if (!t.validContent(s.content)) throw new RangeError("Invalid content for node type " + t.name);
    r.step(new te(e, e + s.nodeSize, e + 1, e + s.nodeSize - 1, new w(b.from(o), 0, 0), 1, true));
  }
  function Ht(r, e, t = 1, n) {
    let i = r.resolve(e), s = i.depth - t, o = n && n[n.length - 1] || i.parent;
    if (s < 0 || i.parent.type.spec.isolating || !i.parent.canReplace(i.index(), i.parent.childCount) || !o.type.validContent(i.parent.content.cutByIndex(i.index(), i.parent.childCount))) return false;
    for (let c = i.depth - 1, f = t - 2; c > s; c--, f--) {
      let d = i.node(c), u = i.index(c);
      if (d.type.spec.isolating) return false;
      let m = d.content.cutByIndex(u, d.childCount), p = n && n[f + 1];
      p && (m = m.replaceChild(0, p.type.create(p.attrs)));
      let _ = n && n[f] || d;
      if (!d.canReplace(u + 1, d.childCount) || !_.type.validContent(m)) return false;
    }
    let l = i.indexAfter(s), a = n && n[0];
    return i.node(s).canReplaceWith(l, l, a ? a.type : i.node(s + 1).type);
  }
  function Cc(r, e, t = 1, n) {
    let i = r.doc.resolve(e), s = b.empty, o = b.empty;
    for (let l = i.depth, a = i.depth - t, c = t - 1; l > a; l--, c--) {
      s = b.from(i.node(l).copy(s));
      let f = n && n[c];
      o = b.from(f ? f.type.create(f.attrs, o) : i.node(l).copy(o));
    }
    r.step(new B(e, e, new w(s.append(o), t, t), true));
  }
  function ss(r, e) {
    let t = r.resolve(e), n = t.index();
    return Mc(t.nodeBefore, t.nodeAfter) && t.parent.canReplace(n, n + 1);
  }
  function Oc(r, e) {
    e.content.size || r.type.compatibleContent(e.type);
    let t = r.contentMatchAt(r.childCount), { linebreakReplacement: n } = r.type.schema;
    for (let i = 0; i < e.childCount; i++) {
      let s = e.child(i), o = s.type == n ? r.type.schema.nodes.text : s.type;
      if (t = t.matchType(o), !t || !r.type.allowsMarks(s.marks)) return false;
    }
    return t.validEnd;
  }
  function Mc(r, e) {
    return !!(r && e && !r.isLeaf && Oc(r, e));
  }
  function Nc(r, e, t) {
    let n = null, { linebreakReplacement: i } = r.doc.type.schema, s = r.doc.resolve(e - t), o = s.node().type;
    if (i && o.inlineContent) {
      let f = o.whitespace == "pre", d = !!o.contentMatch.matchType(i);
      f && !d ? n = false : !f && d && (n = true);
    }
    let l = r.steps.length;
    if (n === false) {
      let f = r.doc.resolve(e + t);
      is(r, f.node(), f.before(), l);
    }
    o.inlineContent && rr(r, e + t - 1, o, s.node().contentMatchAt(s.index()), n == null);
    let a = r.mapping.slice(l), c = a.map(e - t);
    if (r.step(new B(c, a.map(e + t, -1), w.empty, true)), n === true) {
      let f = r.doc.resolve(c);
      rs(r, f.node(), f.before(), r.steps.length);
    }
    return r;
  }
  function Ec(r, e, t) {
    let n = r.resolve(e);
    if (n.parent.canReplaceWith(n.index(), n.index(), t)) return e;
    if (n.parentOffset == 0) for (let i = n.depth - 1; i >= 0; i--) {
      let s = n.index(i);
      if (n.node(i).canReplaceWith(s, s, t)) return n.before(i + 1);
      if (s > 0) return null;
    }
    if (n.parentOffset == n.parent.content.size) for (let i = n.depth - 1; i >= 0; i--) {
      let s = n.indexAfter(i);
      if (n.node(i).canReplaceWith(s, s, t)) return n.after(i + 1);
      if (s < n.node(i).childCount) return null;
    }
    return null;
  }
  function Tc(r, e, t) {
    let n = r.resolve(e);
    if (!t.content.size) return e;
    let i = t.content;
    for (let s = 0; s < t.openStart; s++) i = i.firstChild.content;
    for (let s = 1; s <= (t.openStart == 0 && t.size ? 2 : 1); s++) for (let o = n.depth; o >= 0; o--) {
      let l = o == n.depth ? 0 : n.pos <= (n.start(o + 1) + n.end(o + 1)) / 2 ? -1 : 1, a = n.index(o) + (l > 0 ? 1 : 0), c = n.node(o), f = false;
      if (s == 1) f = c.canReplace(a, a, i);
      else {
        let d = c.contentMatchAt(a).findWrapping(i.firstChild.type);
        f = d && c.canReplaceWith(a, a, d[0]);
      }
      if (f) return l == 0 ? n.pos : l < 0 ? n.before(o + 1) : n.after(o + 1);
    }
    return null;
  }
  function sr(r, e, t = e, n = w.empty) {
    if (e == t && !n.size) return null;
    let i = r.resolve(e), s = r.resolve(t);
    return ls(i, s, n) ? new B(e, t, n) : new Dc(i, s, n).fit();
  }
  function ls(r, e, t) {
    return !t.openStart && !t.openEnd && r.start() == e.start() && r.parent.canReplace(r.index(), e.index(), t.content);
  }
  class Dc {
    constructor(e, t, n) {
      this.$from = e, this.$to = t, this.unplaced = n, this.frontier = [], this.placed = b.empty;
      for (let i = 0; i <= e.depth; i++) {
        let s = e.node(i);
        this.frontier.push({
          type: s.type,
          match: s.contentMatchAt(e.indexAfter(i))
        });
      }
      for (let i = e.depth; i > 0; i--) this.placed = b.from(e.node(i).copy(this.placed));
    }
    get depth() {
      return this.frontier.length - 1;
    }
    fit() {
      for (; this.unplaced.size; ) {
        let c = this.findFittable();
        c ? this.placeNodes(c) : this.openMore() || this.dropNode();
      }
      let e = this.mustMoveInline(), t = this.placed.size - this.depth - this.$from.depth, n = this.$from, i = this.close(e < 0 ? this.$to : n.doc.resolve(e));
      if (!i) return null;
      let s = this.placed, o = n.depth, l = i.depth;
      for (; o && l && s.childCount == 1; ) s = s.firstChild.content, o--, l--;
      let a = new w(s, o, l);
      return e > -1 ? new te(n.pos, e, this.$to.pos, this.$to.end(), a, t) : a.size || n.pos != this.$to.pos ? new B(n.pos, i.pos, a) : null;
    }
    findFittable() {
      let e = this.unplaced.openStart;
      for (let t = this.unplaced.content, n = 0, i = this.unplaced.openEnd; n < e; n++) {
        let s = t.firstChild;
        if (t.childCount > 1 && (i = 0), s.type.spec.isolating && i <= n) {
          e = n;
          break;
        }
        t = s.content;
      }
      for (let t = 1; t <= 2; t++) for (let n = t == 1 ? e : this.unplaced.openStart; n >= 0; n--) {
        let i, s = null;
        n ? (s = _n(this.unplaced.content, n - 1).firstChild, i = s.content) : i = this.unplaced.content;
        let o = i.firstChild;
        for (let l = this.depth; l >= 0; l--) {
          let { type: a, match: c } = this.frontier[l], f, d = null;
          if (t == 1 && (o ? c.matchType(o.type) || (d = c.fillBefore(b.from(o), false)) : s && a.compatibleContent(s.type))) return {
            sliceDepth: n,
            frontierDepth: l,
            parent: s,
            inject: d
          };
          if (t == 2 && o && (f = c.findWrapping(o.type))) return {
            sliceDepth: n,
            frontierDepth: l,
            parent: s,
            wrap: f
          };
          if (s && c.matchType(s.type)) break;
        }
      }
    }
    openMore() {
      let { content: e, openStart: t, openEnd: n } = this.unplaced, i = _n(e, t);
      return !i.childCount || i.firstChild.isLeaf ? false : (this.unplaced = new w(e, t + 1, Math.max(n, i.size + t >= e.size - n ? t + 1 : 0)), true);
    }
    dropNode() {
      let { content: e, openStart: t, openEnd: n } = this.unplaced, i = _n(e, t);
      if (i.childCount <= 1 && t > 0) {
        let s = e.size - t <= t + i.size;
        this.unplaced = new w(dt(e, t - 1, 1), t - 1, s ? t - 1 : n);
      } else this.unplaced = new w(dt(e, t, 1), t, n);
    }
    placeNodes({ sliceDepth: e, frontierDepth: t, parent: n, inject: i, wrap: s }) {
      for (; this.depth > t; ) this.closeFrontierNode();
      if (s) for (let _ = 0; _ < s.length; _++) this.openFrontierNode(s[_]);
      let o = this.unplaced, l = n ? n.content : o.content, a = o.openStart - e, c = 0, f = [], { match: d, type: u } = this.frontier[t];
      if (i) {
        for (let _ = 0; _ < i.childCount; _++) f.push(i.child(_));
        d = d.matchFragment(i);
      }
      let m = l.size + e - (o.content.size - o.openEnd);
      for (; c < l.childCount; ) {
        let _ = l.child(c), y = d.matchType(_.type);
        if (!y) break;
        c++, (c > 1 || a == 0 || _.content.size) && (d = y, f.push(as(_.mark(u.allowedMarks(_.marks)), c == 1 ? a : 0, c == l.childCount ? m : -1)));
      }
      let p = c == l.childCount;
      p || (m = -1), this.placed = ut(this.placed, t, b.from(f)), this.frontier[t].match = d, p && m < 0 && n && n.type == this.frontier[this.depth].type && this.frontier.length > 1 && this.closeFrontierNode();
      for (let _ = 0, y = l; _ < m; _++) {
        let S = y.lastChild;
        this.frontier.push({
          type: S.type,
          match: S.contentMatchAt(S.childCount)
        }), y = S.content;
      }
      this.unplaced = p ? e == 0 ? w.empty : new w(dt(o.content, e - 1, 1), e - 1, m < 0 ? o.openEnd : e - 1) : new w(dt(o.content, e, c), o.openStart, o.openEnd);
    }
    mustMoveInline() {
      if (!this.$to.parent.isTextblock) return -1;
      let e = this.frontier[this.depth], t;
      if (!e.type.isTextblock || !yn(this.$to, this.$to.depth, e.type, e.match, false) || this.$to.depth == this.depth && (t = this.findCloseLevel(this.$to)) && t.depth == this.depth) return -1;
      let { depth: n } = this.$to, i = this.$to.after(n);
      for (; n > 1 && i == this.$to.end(--n); ) ++i;
      return i;
    }
    findCloseLevel(e) {
      e: for (let t = Math.min(this.depth, e.depth); t >= 0; t--) {
        let { match: n, type: i } = this.frontier[t], s = t < e.depth && e.end(t + 1) == e.pos + (e.depth - (t + 1)), o = yn(e, t, i, n, s);
        if (o) {
          for (let l = t - 1; l >= 0; l--) {
            let { match: a, type: c } = this.frontier[l], f = yn(e, l, c, a, true);
            if (!f || f.childCount) continue e;
          }
          return {
            depth: t,
            fit: o,
            move: s ? e.doc.resolve(e.after(t + 1)) : e
          };
        }
      }
    }
    close(e) {
      let t = this.findCloseLevel(e);
      if (!t) return null;
      for (; this.depth > t.depth; ) this.closeFrontierNode();
      t.fit.childCount && (this.placed = ut(this.placed, t.depth, t.fit)), e = t.move;
      for (let n = t.depth + 1; n <= e.depth; n++) {
        let i = e.node(n), s = i.type.contentMatch.fillBefore(i.content, true, e.index(n));
        this.openFrontierNode(i.type, i.attrs, s);
      }
      return e;
    }
    openFrontierNode(e, t = null, n) {
      let i = this.frontier[this.depth];
      i.match = i.match.matchType(e), this.placed = ut(this.placed, this.depth, b.from(e.create(t, n))), this.frontier.push({
        type: e,
        match: e.contentMatch
      });
    }
    closeFrontierNode() {
      let t = this.frontier.pop().match.fillBefore(b.empty, true);
      t.childCount && (this.placed = ut(this.placed, this.frontier.length, t));
    }
  }
  function dt(r, e, t) {
    return e == 0 ? r.cutByIndex(t, r.childCount) : r.replaceChild(0, r.firstChild.copy(dt(r.firstChild.content, e - 1, t)));
  }
  function ut(r, e, t) {
    return e == 0 ? r.append(t) : r.replaceChild(r.childCount - 1, r.lastChild.copy(ut(r.lastChild.content, e - 1, t)));
  }
  function _n(r, e) {
    for (let t = 0; t < e; t++) r = r.firstChild.content;
    return r;
  }
  function as(r, e, t) {
    if (e <= 0) return r;
    let n = r.content;
    return e > 1 && (n = n.replaceChild(0, as(n.firstChild, e - 1, n.childCount == 1 ? t - 1 : 0))), e > 0 && (n = r.type.contentMatch.fillBefore(n).append(n), t <= 0 && (n = n.append(r.type.contentMatch.matchFragment(n).fillBefore(b.empty, true)))), r.copy(n);
  }
  function yn(r, e, t, n, i) {
    let s = r.node(e), o = i ? r.indexAfter(e) : r.index(e);
    if (o == s.childCount && !t.compatibleContent(s.type)) return null;
    let l = n.fillBefore(s.content, true, o);
    return l && !Ic(t, s.content, o) ? l : null;
  }
  function Ic(r, e, t) {
    for (let n = t; n < e.childCount; n++) if (!r.allowsMarks(e.child(n).marks)) return true;
    return false;
  }
  function Ac(r) {
    return r.spec.defining || r.spec.definingForContent;
  }
  function vc(r, e, t, n) {
    if (!n.size) return r.deleteRange(e, t);
    let i = r.doc.resolve(e), s = r.doc.resolve(t);
    if (ls(i, s, n)) return r.step(new B(e, t, n));
    let o = fs(i, s);
    o[o.length - 1] == 0 && o.pop();
    let l = -(i.depth + 1);
    o.unshift(l);
    for (let u = i.depth, m = i.pos - 1; u > 0; u--, m--) {
      let p = i.node(u).type.spec;
      if (p.defining || p.definingAsContext || p.isolating) break;
      o.indexOf(u) > -1 ? l = u : i.before(u) == m && o.splice(1, 0, -u);
    }
    let a = o.indexOf(l), c = [], f = n.openStart;
    for (let u = n.content, m = 0; ; m++) {
      let p = u.firstChild;
      if (c.push(p), m == n.openStart) break;
      u = p.content;
    }
    for (let u = f - 1; u >= 0; u--) {
      let m = c[u], p = Ac(m.type);
      if (p && !m.sameMarkup(i.node(Math.abs(l) - 1))) f = u;
      else if (p || !m.type.isTextblock) break;
    }
    for (let u = n.openStart; u >= 0; u--) {
      let m = (u + f + 1) % (n.openStart + 1), p = c[m];
      if (p) for (let _ = 0; _ < o.length; _++) {
        let y = o[(_ + a) % o.length], S = true;
        y < 0 && (S = false, y = -y);
        let L = i.node(y - 1), R = i.index(y - 1);
        if (L.canReplaceWith(R, R, p.type, p.marks)) return r.replace(i.before(y), S ? s.after(y) : t, new w(cs(n.content, 0, n.openStart, m), m, n.openEnd));
      }
    }
    let d = r.steps.length;
    for (let u = o.length - 1; u >= 0 && (r.replace(e, t, n), !(r.steps.length > d)); u--) {
      let m = o[u];
      m < 0 || (e = i.before(m), t = s.after(m));
    }
  }
  function cs(r, e, t, n, i) {
    if (e < t) {
      let s = r.firstChild;
      r = r.replaceChild(0, s.copy(cs(s.content, e + 1, t, n, s)));
    }
    if (e > n) {
      let s = i.contentMatchAt(0), o = s.fillBefore(r).append(r);
      r = o.append(s.matchFragment(o).fillBefore(b.empty, true));
    }
    return r;
  }
  function Rc(r, e, t, n) {
    if (!n.isInline && e == t && r.doc.resolve(e).parent.content.size) {
      let i = Ec(r.doc, e, n.type);
      i != null && (e = t = i);
    }
    r.replaceRange(e, t, new w(b.from(n), 0, 0));
  }
  function Pc(r, e, t) {
    let n = r.doc.resolve(e), i = r.doc.resolve(t);
    if (n.parent.isTextblock && i.parent.isTextblock && n.start() != i.start() && n.parentOffset == 0 && i.parentOffset == 0) {
      let o = n.sharedDepth(t), l = false;
      for (let a = n.depth; a > o; a--) n.node(a).type.spec.isolating && (l = true);
      for (let a = i.depth; a > o; a--) i.node(a).type.spec.isolating && (l = true);
      if (!l) {
        for (let a = n.depth; a > 0 && e == n.start(a); a--) e = n.before(a);
        for (let a = i.depth; a > 0 && t == i.start(a); a--) t = i.before(a);
        n = r.doc.resolve(e), i = r.doc.resolve(t);
      }
    }
    let s = fs(n, i);
    for (let o = 0; o < s.length; o++) {
      let l = s[o], a = o == s.length - 1;
      if (a && l == 0 || n.node(l).type.contentMatch.validEnd) return r.delete(n.start(l), i.end(l));
      if (l > 0 && (a || n.node(l - 1).canReplace(n.index(l - 1), i.indexAfter(l - 1)))) return r.delete(n.before(l), i.after(l));
    }
    for (let o = 1; o <= n.depth && o <= i.depth; o++) if (e - n.start(o) == n.depth - o && t > n.end(o) && i.end(o) - t != i.depth - o && n.start(o - 1) == i.start(o - 1) && n.node(o - 1).canReplace(n.index(o - 1), i.index(o - 1))) return r.delete(n.before(o), t);
    r.delete(e, t);
  }
  function fs(r, e) {
    let t = [], n = Math.min(r.depth, e.depth);
    for (let i = n; i >= 0; i--) {
      let s = r.start(i);
      if (s < r.pos - (r.depth - i) || e.end(i) > e.pos + (e.depth - i) || r.node(i).type.spec.isolating || e.node(i).type.spec.isolating) break;
      (s == e.start(i) || i == r.depth && i == e.depth && r.parent.inlineContent && e.parent.inlineContent && i && e.start(i - 1) == s - 1) && t.push(i);
    }
    return t;
  }
  class nt extends H {
    constructor(e, t, n) {
      super(), this.pos = e, this.attr = t, this.value = n;
    }
    apply(e) {
      let t = e.nodeAt(this.pos);
      if (!t) return v.fail("No node at attribute step's position");
      let n = /* @__PURE__ */ Object.create(null);
      for (let s in t.attrs) n[s] = t.attrs[s];
      n[this.attr] = this.value;
      let i = t.type.create(n, null, t.marks);
      return v.fromReplace(e, this.pos, this.pos + 1, new w(b.from(i), 0, t.isLeaf ? 0 : 1));
    }
    getMap() {
      return ee.empty;
    }
    invert(e) {
      return new nt(this.pos, this.attr, e.nodeAt(this.pos).attrs[this.attr]);
    }
    map(e) {
      let t = e.mapResult(this.pos, 1);
      return t.deletedAfter ? null : new nt(t.pos, this.attr, this.value);
    }
    toJSON() {
      return {
        stepType: "attr",
        pos: this.pos,
        attr: this.attr,
        value: this.value
      };
    }
    static fromJSON(e, t) {
      if (typeof t.pos != "number" || typeof t.attr != "string") throw new RangeError("Invalid input for AttrStep.fromJSON");
      return new nt(t.pos, t.attr, t.value);
    }
  }
  H.jsonID("attr", nt);
  class Nt extends H {
    constructor(e, t) {
      super(), this.attr = e, this.value = t;
    }
    apply(e) {
      let t = /* @__PURE__ */ Object.create(null);
      for (let i in e.attrs) t[i] = e.attrs[i];
      t[this.attr] = this.value;
      let n = e.type.create(t, e.content, e.marks);
      return v.ok(n);
    }
    getMap() {
      return ee.empty;
    }
    invert(e) {
      return new Nt(this.attr, e.attrs[this.attr]);
    }
    map(e) {
      return this;
    }
    toJSON() {
      return {
        stepType: "docAttr",
        attr: this.attr,
        value: this.value
      };
    }
    static fromJSON(e, t) {
      if (typeof t.attr != "string") throw new RangeError("Invalid input for DocAttrStep.fromJSON");
      return new Nt(t.attr, t.value);
    }
  }
  H.jsonID("docAttr", Nt);
  let it = class extends Error {
  };
  it = function r(e) {
    let t = Error.call(this, e);
    return t.__proto__ = r.prototype, t;
  };
  it.prototype = Object.create(Error.prototype);
  it.prototype.constructor = it;
  it.prototype.name = "TransformError";
  class Bc {
    constructor(e) {
      this.doc = e, this.steps = [], this.docs = [], this.mapping = new Mt();
    }
    get before() {
      return this.docs.length ? this.docs[0] : this.doc;
    }
    step(e) {
      let t = this.maybeStep(e);
      if (t.failed) throw new it(t.failed);
      return this;
    }
    maybeStep(e) {
      let t = e.apply(this.doc);
      return t.failed || this.addStep(e, t.doc), t;
    }
    get docChanged() {
      return this.steps.length > 0;
    }
    changedRange() {
      let e = 1e9, t = -1e9;
      for (let n = 0; n < this.mapping.maps.length; n++) {
        let i = this.mapping.maps[n];
        n && (e = i.map(e, 1), t = i.map(t, -1)), i.forEach((s, o, l, a) => {
          e = Math.min(e, l), t = Math.max(t, a);
        });
      }
      return e == 1e9 ? null : {
        from: e,
        to: t
      };
    }
    addStep(e, t) {
      this.docs.push(this.doc), this.steps.push(e), this.mapping.appendMap(e.getMap()), this.doc = t;
    }
    replace(e, t = e, n = w.empty) {
      let i = sr(this.doc, e, t, n);
      return i && this.step(i), this;
    }
    replaceWith(e, t, n) {
      return this.replace(e, t, new w(b.from(n), 0, 0));
    }
    delete(e, t) {
      return this.replace(e, t, w.empty);
    }
    insert(e, t) {
      return this.replaceWith(e, e, t);
    }
    replaceRange(e, t, n) {
      return vc(this, e, t, n), this;
    }
    replaceRangeWith(e, t, n) {
      return Rc(this, e, t, n), this;
    }
    deleteRange(e, t) {
      return Pc(this, e, t), this;
    }
    lift(e, t) {
      return bc(this, e, t), this;
    }
    join(e, t = 1) {
      return Nc(this, e, t), this;
    }
    wrap(e, t) {
      return wc(this, e, t), this;
    }
    setBlockType(e, t = e, n, i = null) {
      return kc(this, e, t, n, i), this;
    }
    setNodeMarkup(e, t, n = null, i) {
      return Sc(this, e, t, n, i), this;
    }
    setNodeAttribute(e, t, n) {
      return this.step(new nt(e, t, n)), this;
    }
    setDocAttribute(e, t) {
      return this.step(new Nt(e, t)), this;
    }
    addNodeMark(e, t) {
      return this.step(new Te(e, t)), this;
    }
    removeNodeMark(e, t) {
      let n = this.doc.nodeAt(e);
      if (!n) throw new RangeError("No node at position " + e);
      if (t instanceof T) t.isInSet(n.marks) && this.step(new Ge(e, t));
      else {
        let i = n.marks, s, o = [];
        for (; s = t.isInSet(i); ) o.push(new Ge(e, s)), i = s.removeFromSet(i);
        for (let l = o.length - 1; l >= 0; l--) this.step(o[l]);
      }
      return this;
    }
    split(e, t = 1, n) {
      return Cc(this, e, t, n), this;
    }
    addMark(e, t, n) {
      return gc(this, e, t, n), this;
    }
    removeMark(e, t, n) {
      return _c(this, e, t, n), this;
    }
    clearIncompatible(e, t, n) {
      return rr(this, e, t, n), this;
    }
  }
  const bn = /* @__PURE__ */ Object.create(null);
  class I {
    constructor(e, t, n) {
      this.$anchor = e, this.$head = t, this.ranges = n || [
        new zc(e.min(t), e.max(t))
      ];
    }
    get anchor() {
      return this.$anchor.pos;
    }
    get head() {
      return this.$head.pos;
    }
    get from() {
      return this.$from.pos;
    }
    get to() {
      return this.$to.pos;
    }
    get $from() {
      return this.ranges[0].$from;
    }
    get $to() {
      return this.ranges[0].$to;
    }
    get empty() {
      let e = this.ranges;
      for (let t = 0; t < e.length; t++) if (e[t].$from.pos != e[t].$to.pos) return false;
      return true;
    }
    content() {
      return this.$from.doc.slice(this.from, this.to, true);
    }
    replace(e, t = w.empty) {
      let n = t.content.lastChild, i = null;
      for (let l = 0; l < t.openEnd; l++) i = n, n = n.lastChild;
      let s = e.steps.length, o = this.ranges;
      for (let l = 0; l < o.length; l++) {
        let { $from: a, $to: c } = o[l], f = e.mapping.slice(s);
        e.replaceRange(f.map(a.pos), f.map(c.pos), l ? w.empty : t), l == 0 && Pr(e, s, (n ? n.isInline : i && i.isTextblock) ? -1 : 1);
      }
    }
    replaceWith(e, t) {
      let n = e.steps.length, i = this.ranges;
      for (let s = 0; s < i.length; s++) {
        let { $from: o, $to: l } = i[s], a = e.mapping.slice(n), c = a.map(o.pos), f = a.map(l.pos);
        s ? e.deleteRange(c, f) : (e.replaceRangeWith(c, f, t), Pr(e, n, t.isInline ? -1 : 1));
      }
    }
    static findFrom(e, t, n = false) {
      let i = e.parent.inlineContent ? new E(e) : et(e.node(0), e.parent, e.pos, e.index(), t, n);
      if (i) return i;
      for (let s = e.depth - 1; s >= 0; s--) {
        let o = t < 0 ? et(e.node(0), e.node(s), e.before(s + 1), e.index(s), t, n) : et(e.node(0), e.node(s), e.after(s + 1), e.index(s) + 1, t, n);
        if (o) return o;
      }
      return null;
    }
    static near(e, t = 1) {
      return this.findFrom(e, t) || this.findFrom(e, -t) || new ne(e.node(0));
    }
    static atStart(e) {
      return et(e, e, 0, 0, 1) || new ne(e);
    }
    static atEnd(e) {
      return et(e, e, e.content.size, e.childCount, -1) || new ne(e);
    }
    static fromJSON(e, t) {
      if (!t || !t.type) throw new RangeError("Invalid input for Selection.fromJSON");
      let n = bn[t.type];
      if (!n) throw new RangeError(`No selection type ${t.type} defined`);
      return n.fromJSON(e, t);
    }
    static jsonID(e, t) {
      if (e in bn) throw new RangeError("Duplicate use of selection JSON ID " + e);
      return bn[e] = t, t.prototype.jsonID = e, t;
    }
    getBookmark() {
      return E.between(this.$anchor, this.$head).getBookmark();
    }
  }
  I.prototype.visible = true;
  class zc {
    constructor(e, t) {
      this.$from = e, this.$to = t;
    }
  }
  let vr = false;
  function Rr(r) {
    !vr && !r.parent.inlineContent && (vr = true, console.warn("TextSelection endpoint not pointing into a node with inline content (" + r.parent.type.name + ")"));
  }
  class E extends I {
    constructor(e, t = e) {
      Rr(e), Rr(t), super(e, t);
    }
    get $cursor() {
      return this.$anchor.pos == this.$head.pos ? this.$head : null;
    }
    map(e, t) {
      let n = e.resolve(t.map(this.head));
      if (!n.parent.inlineContent) return I.near(n);
      let i = e.resolve(t.map(this.anchor));
      return new E(i.parent.inlineContent ? i : n, n);
    }
    replace(e, t = w.empty) {
      if (super.replace(e, t), t == w.empty) {
        let n = this.$from.marksAcross(this.$to);
        n && e.ensureMarks(n);
      }
    }
    eq(e) {
      return e instanceof E && e.anchor == this.anchor && e.head == this.head;
    }
    getBookmark() {
      return new cn(this.anchor, this.head);
    }
    toJSON() {
      return {
        type: "text",
        anchor: this.anchor,
        head: this.head
      };
    }
    static fromJSON(e, t) {
      if (typeof t.anchor != "number" || typeof t.head != "number") throw new RangeError("Invalid input for TextSelection.fromJSON");
      return new E(e.resolve(t.anchor), e.resolve(t.head));
    }
    static create(e, t, n = t) {
      let i = e.resolve(t);
      return new this(i, n == t ? i : e.resolve(n));
    }
    static between(e, t, n) {
      let i = e.pos - t.pos;
      if ((!n || i) && (n = i >= 0 ? 1 : -1), !t.parent.inlineContent) {
        let s = I.findFrom(t, n, true) || I.findFrom(t, -n, true);
        if (s) t = s.$head;
        else return I.near(t, n);
      }
      return e.parent.inlineContent || (i == 0 ? e = t : (e = (I.findFrom(e, -n, true) || I.findFrom(e, n, true)).$anchor, e.pos < t.pos != i < 0 && (e = t))), new E(e, t);
    }
  }
  I.jsonID("text", E);
  class cn {
    constructor(e, t) {
      this.anchor = e, this.head = t;
    }
    map(e) {
      return new cn(e.map(this.anchor), e.map(this.head));
    }
    resolve(e) {
      return E.between(e.resolve(this.anchor), e.resolve(this.head));
    }
  }
  class O extends I {
    constructor(e) {
      let t = e.nodeAfter, n = e.node(0).resolve(e.pos + t.nodeSize);
      super(e, n), this.node = t;
    }
    map(e, t) {
      let { deleted: n, pos: i } = t.mapResult(this.anchor), s = e.resolve(i);
      return n ? I.near(s) : new O(s);
    }
    content() {
      return new w(b.from(this.node), 0, 0);
    }
    eq(e) {
      return e instanceof O && e.anchor == this.anchor;
    }
    toJSON() {
      return {
        type: "node",
        anchor: this.anchor
      };
    }
    getBookmark() {
      return new or(this.anchor);
    }
    static fromJSON(e, t) {
      if (typeof t.anchor != "number") throw new RangeError("Invalid input for NodeSelection.fromJSON");
      return new O(e.resolve(t.anchor));
    }
    static create(e, t) {
      return new O(e.resolve(t));
    }
    static isSelectable(e) {
      return !e.isText && e.type.spec.selectable !== false;
    }
  }
  O.prototype.visible = false;
  I.jsonID("node", O);
  class or {
    constructor(e) {
      this.anchor = e;
    }
    map(e) {
      let { deleted: t, pos: n } = e.mapResult(this.anchor);
      return t ? new cn(n, n) : new or(n);
    }
    resolve(e) {
      let t = e.resolve(this.anchor), n = t.nodeAfter;
      return n && O.isSelectable(n) ? new O(t) : I.near(t);
    }
  }
  class ne extends I {
    constructor(e) {
      super(e.resolve(0), e.resolve(e.content.size));
    }
    replace(e, t = w.empty) {
      if (t == w.empty) {
        e.delete(0, e.doc.content.size);
        let n = I.atStart(e.doc);
        n.eq(e.selection) || e.setSelection(n);
      } else super.replace(e, t);
    }
    toJSON() {
      return {
        type: "all"
      };
    }
    static fromJSON(e) {
      return new ne(e);
    }
    map(e) {
      return new ne(e);
    }
    eq(e) {
      return e instanceof ne;
    }
    getBookmark() {
      return Fc;
    }
  }
  I.jsonID("all", ne);
  const Fc = {
    map() {
      return this;
    },
    resolve(r) {
      return new ne(r);
    }
  };
  function et(r, e, t, n, i, s = false) {
    if (e.inlineContent) return E.create(r, t);
    for (let o = n - (i > 0 ? 0 : 1); i > 0 ? o < e.childCount : o >= 0; o += i) {
      let l = e.child(o);
      if (l.isAtom) {
        if (!s && O.isSelectable(l)) return O.create(r, t - (i < 0 ? l.nodeSize : 0));
      } else {
        let a = et(r, l, t + i, i < 0 ? l.childCount : 0, i, s);
        if (a) return a;
      }
      t += l.nodeSize * i;
    }
    return null;
  }
  function Pr(r, e, t) {
    let n = r.steps.length - 1;
    if (n < e) return;
    let i = r.steps[n];
    if (!(i instanceof B || i instanceof te)) return;
    let s = r.mapping.maps[n], o;
    s.forEach((l, a, c, f) => {
      o == null && (o = f);
    }), r.setSelection(I.near(r.doc.resolve(o), t));
  }
  const Br = 1, Wt = 2, zr = 4;
  class Vc extends Bc {
    constructor(e) {
      super(e.doc), this.curSelectionFor = 0, this.updated = 0, this.meta = /* @__PURE__ */ Object.create(null), this.time = Date.now(), this.curSelection = e.selection, this.storedMarks = e.storedMarks;
    }
    get selection() {
      return this.curSelectionFor < this.steps.length && (this.curSelection = this.curSelection.map(this.doc, this.mapping.slice(this.curSelectionFor)), this.curSelectionFor = this.steps.length), this.curSelection;
    }
    setSelection(e) {
      if (e.$from.doc != this.doc) throw new RangeError("Selection passed to setSelection must point at the current document");
      return this.curSelection = e, this.curSelectionFor = this.steps.length, this.updated = (this.updated | Br) & ~Wt, this.storedMarks = null, this;
    }
    get selectionSet() {
      return (this.updated & Br) > 0;
    }
    setStoredMarks(e) {
      return this.storedMarks = e, this.updated |= Wt, this;
    }
    ensureMarks(e) {
      return T.sameSet(this.storedMarks || this.selection.$from.marks(), e) || this.setStoredMarks(e), this;
    }
    addStoredMark(e) {
      return this.ensureMarks(e.addToSet(this.storedMarks || this.selection.$head.marks()));
    }
    removeStoredMark(e) {
      return this.ensureMarks(e.removeFromSet(this.storedMarks || this.selection.$head.marks()));
    }
    get storedMarksSet() {
      return (this.updated & Wt) > 0;
    }
    addStep(e, t) {
      super.addStep(e, t), this.updated = this.updated & ~Wt, this.storedMarks = null;
    }
    setTime(e) {
      return this.time = e, this;
    }
    replaceSelection(e) {
      return this.selection.replace(this, e), this;
    }
    replaceSelectionWith(e, t = true) {
      let n = this.selection;
      return t && (e = e.mark(this.storedMarks || (n.empty ? n.$from.marks() : n.$from.marksAcross(n.$to) || T.none))), n.replaceWith(this, e), this;
    }
    deleteSelection() {
      return this.selection.replace(this), this;
    }
    insertText(e, t, n) {
      let i = this.doc.type.schema;
      if (t == null) return e ? this.replaceSelectionWith(i.text(e), true) : this.deleteSelection();
      {
        if (n == null && (n = t), !e) return this.deleteRange(t, n);
        let s = this.storedMarks;
        if (!s) {
          let o = this.doc.resolve(t);
          s = n == t ? o.marks() : o.marksAcross(this.doc.resolve(n));
        }
        return this.replaceRangeWith(t, n, i.text(e, s)), !this.selection.empty && this.selection.to == t + e.length && this.setSelection(I.near(this.selection.$to)), this;
      }
    }
    setMeta(e, t) {
      return this.meta[typeof e == "string" ? e : e.key] = t, this;
    }
    getMeta(e) {
      return this.meta[typeof e == "string" ? e : e.key];
    }
    get isGeneric() {
      for (let e in this.meta) return false;
      return true;
    }
    scrollIntoView() {
      return this.updated |= zr, this;
    }
    get scrolledIntoView() {
      return (this.updated & zr) > 0;
    }
  }
  function Fr(r, e) {
    return !e || !r ? r : r.bind(e);
  }
  class ht {
    constructor(e, t, n) {
      this.name = e, this.init = Fr(t.init, n), this.apply = Fr(t.apply, n);
    }
  }
  const Lc = [
    new ht("doc", {
      init(r) {
        return r.doc || r.schema.topNodeType.createAndFill();
      },
      apply(r) {
        return r.doc;
      }
    }),
    new ht("selection", {
      init(r, e) {
        return r.selection || I.atStart(e.doc);
      },
      apply(r) {
        return r.selection;
      }
    }),
    new ht("storedMarks", {
      init(r) {
        return r.storedMarks || null;
      },
      apply(r, e, t, n) {
        return n.selection.$cursor ? r.storedMarks : null;
      }
    }),
    new ht("scrollToSelection", {
      init() {
        return 0;
      },
      apply(r, e) {
        return r.scrolledIntoView ? e + 1 : e;
      }
    })
  ];
  class wn {
    constructor(e, t) {
      this.schema = e, this.plugins = [], this.pluginsByKey = /* @__PURE__ */ Object.create(null), this.fields = Lc.slice(), t && t.forEach((n) => {
        if (this.pluginsByKey[n.key]) throw new RangeError("Adding different instances of a keyed plugin (" + n.key + ")");
        this.plugins.push(n), this.pluginsByKey[n.key] = n, n.spec.state && this.fields.push(new ht(n.key, n.spec.state, n));
      });
    }
  }
  class Le {
    constructor(e) {
      this.config = e;
    }
    get schema() {
      return this.config.schema;
    }
    get plugins() {
      return this.config.plugins;
    }
    apply(e) {
      return this.applyTransaction(e).state;
    }
    filterTransaction(e, t = -1) {
      for (let n = 0; n < this.config.plugins.length; n++) if (n != t) {
        let i = this.config.plugins[n];
        if (i.spec.filterTransaction && !i.spec.filterTransaction.call(i, e, this)) return false;
      }
      return true;
    }
    applyTransaction(e) {
      if (!this.filterTransaction(e)) return {
        state: this,
        transactions: []
      };
      let t = [
        e
      ], n = this.applyInner(e), i = null;
      for (; ; ) {
        let s = false;
        for (let o = 0; o < this.config.plugins.length; o++) {
          let l = this.config.plugins[o];
          if (l.spec.appendTransaction) {
            let a = i ? i[o].n : 0, c = i ? i[o].state : this, f = a < t.length && l.spec.appendTransaction.call(l, a ? t.slice(a) : t, c, n);
            if (f && n.filterTransaction(f, o)) {
              if (f.setMeta("appendedTransaction", e), !i) {
                i = [];
                for (let d = 0; d < this.config.plugins.length; d++) i.push(d < o ? {
                  state: n,
                  n: t.length
                } : {
                  state: this,
                  n: 0
                });
              }
              t.push(f), n = n.applyInner(f), s = true;
            }
            i && (i[o] = {
              state: n,
              n: t.length
            });
          }
        }
        if (!s) return {
          state: n,
          transactions: t
        };
      }
    }
    applyInner(e) {
      if (!e.before.eq(this.doc)) throw new RangeError("Applying a mismatched transaction");
      let t = new Le(this.config), n = this.config.fields;
      for (let i = 0; i < n.length; i++) {
        let s = n[i];
        t[s.name] = s.apply(e, this[s.name], this, t);
      }
      return t;
    }
    get tr() {
      return new Vc(this);
    }
    static create(e) {
      let t = new wn(e.doc ? e.doc.type.schema : e.schema, e.plugins), n = new Le(t);
      for (let i = 0; i < t.fields.length; i++) n[t.fields[i].name] = t.fields[i].init(e, n);
      return n;
    }
    reconfigure(e) {
      let t = new wn(this.schema, e.plugins), n = t.fields, i = new Le(t);
      for (let s = 0; s < n.length; s++) {
        let o = n[s].name;
        i[o] = this.hasOwnProperty(o) ? this[o] : n[s].init(e, i);
      }
      return i;
    }
    toJSON(e) {
      let t = {
        doc: this.doc.toJSON(),
        selection: this.selection.toJSON()
      };
      if (this.storedMarks && (t.storedMarks = this.storedMarks.map((n) => n.toJSON())), e && typeof e == "object") for (let n in e) {
        if (n == "doc" || n == "selection") throw new RangeError("The JSON fields `doc` and `selection` are reserved");
        let i = e[n], s = i.spec.state;
        s && s.toJSON && (t[n] = s.toJSON.call(i, this[i.key]));
      }
      return t;
    }
    static fromJSON(e, t, n) {
      if (!t) throw new RangeError("Invalid input for EditorState.fromJSON");
      if (!e.schema) throw new RangeError("Required config field 'schema' missing");
      let i = new wn(e.schema, e.plugins), s = new Le(i);
      return i.fields.forEach((o) => {
        if (o.name == "doc") s.doc = me.fromJSON(e.schema, t.doc);
        else if (o.name == "selection") s.selection = I.fromJSON(s.doc, t.selection);
        else if (o.name == "storedMarks") t.storedMarks && (s.storedMarks = t.storedMarks.map(e.schema.markFromJSON));
        else {
          if (n) for (let l in n) {
            let a = n[l], c = a.spec.state;
            if (a.key == o.name && c && c.fromJSON && Object.prototype.hasOwnProperty.call(t, l)) {
              s[o.name] = c.fromJSON.call(a, e, t[l], s);
              return;
            }
          }
          s[o.name] = o.init(e, s);
        }
      }), s;
    }
  }
  function ds(r, e, t) {
    for (let n in r) {
      let i = r[n];
      i instanceof Function ? i = i.bind(e) : n == "handleDOMEvents" && (i = ds(i, e, {})), t[n] = i;
    }
    return t;
  }
  class us {
    constructor(e) {
      this.spec = e, this.props = {}, e.props && ds(e.props, this, this.props), this.key = e.key ? e.key.key : hs("plugin");
    }
    getState(e) {
      return e[this.key];
    }
  }
  const kn = /* @__PURE__ */ Object.create(null);
  function hs(r) {
    return r in kn ? r + "$" + ++kn[r] : (kn[r] = 0, r + "$");
  }
  class ps {
    constructor(e = "key") {
      this.key = hs(e);
    }
    get(e) {
      return e.config.pluginsByKey[this.key];
    }
    getState(e) {
      return e[this.key];
    }
  }
  const xn = {
    strong: 0,
    emph: 1,
    underline: 2,
    strike: 3,
    code: 4,
    link: 5,
    anchor: 6
  };
  function qc(r) {
    let e = 0;
    for (const t of r) e++;
    return e;
  }
  function Xt(r) {
    return r === "emph" ? "em" : r;
  }
  function Wc(r) {
    return r === "em" ? "emph" : r;
  }
  function Sn(r, e) {
    return r === "link" && (e == null ? void 0 : e.href) ? e.href : "";
  }
  function lr(r, e) {
    const t = [
      ...r
    ], n = e.filter((l) => l.type !== "anchor").map((l) => {
      let { start: a, end: c } = l;
      for (; a < c && t[a] === `
`; ) a++;
      for (; c > a && t[c - 1] === `
`; ) c--;
      return {
        ...l,
        start: a,
        end: c
      };
    }).filter((l) => l.start < l.end), i = /* @__PURE__ */ new Map(), s = [];
    for (const l of n) {
      if (l.type === "anchor") {
        s.push(l);
        continue;
      }
      const a = `${xn[l.type] ?? 99}:${Sn(l.type, l.attrs)}`;
      let c = i.get(a);
      c || (c = {
        type: l.type,
        attrs: l.attrs,
        ranges: []
      }, i.set(a, c)), c.ranges.push([
        l.start,
        l.end
      ]);
    }
    const o = [];
    for (const l of i.values()) {
      l.ranges.sort((f, d) => f[0] - d[0]);
      let [a, c] = l.ranges[0];
      for (const [f, d] of l.ranges.slice(1)) f <= c ? c = Math.max(c, d) : (o.push({
        start: a,
        end: c,
        type: l.type,
        attrs: l.attrs
      }), [a, c] = [
        f,
        d
      ]);
      o.push({
        start: a,
        end: c,
        type: l.type,
        attrs: l.attrs
      });
    }
    return o.push(...s), o.sort((l, a) => {
      const c = xn[l.type] ?? 99, f = xn[a.type] ?? 99;
      return l.start - a.start || l.end - a.end || c - f || Sn(l.type, l.attrs).localeCompare(Sn(a.type, a.attrs));
    }), o;
  }
  function Jc(r, e) {
    const t = lr(r, e);
    return {
      text: r,
      lines: [
        {
          kind: "para",
          containers: [],
          continues: false
        }
      ],
      marks: t.map(({ start: n, end: i, type: s, attrs: o }) => s === "link" ? {
        start: n,
        end: i,
        type: "link",
        url: (o == null ? void 0 : o.href) ?? (o == null ? void 0 : o.url) ?? ""
      } : {
        start: n,
        end: i,
        type: s
      }),
      islands: []
    };
  }
  const $c = [
    "p",
    0
  ], jc = [
    "blockquote",
    0
  ], Kc = [
    "hr"
  ], Hc = [
    "pre",
    [
      "code",
      0
    ]
  ], Uc = [
    "br"
  ], Gc = {
    doc: {
      content: "block+"
    },
    paragraph: {
      content: "inline*",
      group: "block",
      parseDOM: [
        {
          tag: "p"
        }
      ],
      toDOM() {
        return $c;
      }
    },
    blockquote: {
      content: "block+",
      group: "block",
      defining: true,
      parseDOM: [
        {
          tag: "blockquote"
        }
      ],
      toDOM() {
        return jc;
      }
    },
    horizontal_rule: {
      group: "block",
      parseDOM: [
        {
          tag: "hr"
        }
      ],
      toDOM() {
        return Kc;
      }
    },
    heading: {
      attrs: {
        level: {
          default: 1,
          validate: "number"
        }
      },
      content: "inline*",
      group: "block",
      defining: true,
      parseDOM: [
        {
          tag: "h1",
          attrs: {
            level: 1
          }
        },
        {
          tag: "h2",
          attrs: {
            level: 2
          }
        },
        {
          tag: "h3",
          attrs: {
            level: 3
          }
        },
        {
          tag: "h4",
          attrs: {
            level: 4
          }
        },
        {
          tag: "h5",
          attrs: {
            level: 5
          }
        },
        {
          tag: "h6",
          attrs: {
            level: 6
          }
        }
      ],
      toDOM(r) {
        return [
          "h" + r.attrs.level,
          0
        ];
      }
    },
    code_block: {
      content: "text*",
      marks: "",
      group: "block",
      code: true,
      defining: true,
      parseDOM: [
        {
          tag: "pre",
          preserveWhitespace: "full"
        }
      ],
      toDOM() {
        return Hc;
      }
    },
    text: {
      group: "inline"
    },
    image: {
      inline: true,
      attrs: {
        src: {
          validate: "string"
        },
        alt: {
          default: null,
          validate: "string|null"
        },
        title: {
          default: null,
          validate: "string|null"
        }
      },
      group: "inline",
      draggable: true,
      parseDOM: [
        {
          tag: "img[src]",
          getAttrs(r) {
            return {
              src: r.getAttribute("src"),
              title: r.getAttribute("title"),
              alt: r.getAttribute("alt")
            };
          }
        }
      ],
      toDOM(r) {
        let { src: e, alt: t, title: n } = r.attrs;
        return [
          "img",
          {
            src: e,
            alt: t,
            title: n
          }
        ];
      }
    },
    hard_break: {
      inline: true,
      group: "inline",
      selectable: false,
      parseDOM: [
        {
          tag: "br"
        }
      ],
      toDOM() {
        return Uc;
      }
    }
  }, Yc = [
    "em",
    0
  ], Qc = [
    "strong",
    0
  ], Xc = [
    "code",
    0
  ], Zc = {
    link: {
      attrs: {
        href: {
          validate: "string"
        },
        title: {
          default: null,
          validate: "string|null"
        }
      },
      inclusive: false,
      parseDOM: [
        {
          tag: "a[href]",
          getAttrs(r) {
            return {
              href: r.getAttribute("href"),
              title: r.getAttribute("title")
            };
          }
        }
      ],
      toDOM(r) {
        let { href: e, title: t } = r.attrs;
        return [
          "a",
          {
            href: e,
            title: t
          },
          0
        ];
      }
    },
    em: {
      parseDOM: [
        {
          tag: "i"
        },
        {
          tag: "em"
        },
        {
          style: "font-style=italic"
        },
        {
          style: "font-style=normal",
          clearMark: (r) => r.type.name == "em"
        }
      ],
      toDOM() {
        return Yc;
      }
    },
    strong: {
      parseDOM: [
        {
          tag: "strong"
        },
        {
          tag: "b",
          getAttrs: (r) => r.style.fontWeight != "normal" && null
        },
        {
          style: "font-weight=400",
          clearMark: (r) => r.type.name == "strong"
        },
        {
          style: "font-weight",
          getAttrs: (r) => /^(bold(er)?|[5-9]\d{2,})$/.test(r) && null
        }
      ],
      toDOM() {
        return Qc;
      }
    },
    code: {
      code: true,
      parseDOM: [
        {
          tag: "code"
        }
      ],
      toDOM() {
        return Xc;
      }
    }
  }, ms = new Gi({
    nodes: Gc,
    marks: Zc
  }), ef = ms.spec.marks.addToEnd("underline", {
    parseDOM: [
      {
        tag: "u"
      }
    ],
    toDOM() {
      return [
        "u",
        0
      ];
    }
  }).addToEnd("strike", {
    parseDOM: [
      {
        tag: "s"
      },
      {
        tag: "del"
      }
    ],
    toDOM() {
      return [
        "s",
        0
      ];
    }
  }), ae = new Gi({
    nodes: ms.spec.nodes,
    marks: ef
  });
  function Vr(r, e) {
    const t = lr(r, e);
    if (r.length === 0) return ae.node("doc", null, [
      ae.node("paragraph")
    ]);
    const n = t.map((a) => {
      var _a2, _b;
      const c = Xt(a.type), f = ae.marks[c];
      if (!f) throw new Error(`unknown mark: ${a.type}`);
      const d = a.type === "link" ? {
        href: ((_a2 = a.attrs) == null ? void 0 : _a2.href) ?? ((_b = a.attrs) == null ? void 0 : _b.url) ?? ""
      } : null;
      return {
        from: a.start,
        to: a.end,
        mark: f.create(d)
      };
    }), i = (a) => n.filter((c) => a >= c.from && a < c.to).map((c) => c.mark), s = [
      ...r
    ], o = [];
    for (let a = 0; a < s.length; a++) o.push(ae.text(s[a], i(a)));
    const l = ae.node("paragraph", null, b.fromArray(o));
    return ae.node("doc", null, [
      l
    ]);
  }
  function Wn(r) {
    const e = r.firstChild;
    if (!e) return {
      text: "",
      marks: []
    };
    let t = "";
    const n = [];
    return e.forEach((i, s) => {
      if (!i.isText) return;
      const o = qc(t), l = i.text ?? "", a = [
        ...l
      ];
      for (let c = 0; c < a.length; c++) {
        const f = o + c;
        for (const d of i.marks) n.push({
          start: f,
          end: f + 1,
          type: Wc(d.type.name),
          attrs: d.type.name === "link" ? {
            href: d.attrs.href
          } : void 0
        });
      }
      t += l;
    }), {
      text: t,
      marks: lr(t, n)
    };
  }
  function tf(r, e) {
    const t = r.firstChild;
    if (!t) return 1;
    let n = 0, i = 1;
    for (let s = 0; s < t.childCount; s++) {
      const o = t.child(s);
      if (!o.isText) continue;
      const l = [
        ...o.text ?? ""
      ].length;
      if (e <= n + l) return i + (e - n);
      n += l, i += o.nodeSize;
    }
    return 1 + t.content.size;
  }
  const nf = Object.assign({
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/Quill.yaml": () => A(() => import("./Quill-DmHwGsWQ.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/__golden__/schema.yaml": () => A(() => import("./schema-B7n9rMG9.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/assets/dod_seal.png": () => A(() => import("./dod_seal-dmEQlZDX.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/assets/dow_seal.png": () => A(() => import("./dow_seal-CTGobuQW.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/fonts/Cinzel/Cinzel[wght].ttf": () => A(() => import("./Cinzel_wght_-BMa-0Db2.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/fonts/CopperplateCC/CopperplateCC-Bold.otf": () => A(() => import("./CopperplateCC-Bold-CcGlpOBX.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/fonts/CopperplateCC/CopperplateCC-Heavy.otf": () => A(() => import("./CopperplateCC-Heavy-CLKxEHtK.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/fonts/LiberationMono/LiberationMono-Regular.ttf": () => A(() => import("./LiberationMono-Regular-DKthgSCf.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/fonts/NimbusRomanNo9L/GNU General Public License.txt": () => A(() => import("./GNU General Public License-CxGU0-Kg.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/fonts/NimbusRomanNo9L/NimbusRomNo9L-Med.otf": () => A(() => import("./NimbusRomNo9L-Med-1vPOi6ZZ.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/fonts/NimbusRomanNo9L/NimbusRomNo9L-MedIta.otf": () => A(() => import("./NimbusRomNo9L-MedIta-DbEmPLPt.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/fonts/NimbusRomanNo9L/NimbusRomNo9L-Reg.otf": () => A(() => import("./NimbusRomNo9L-Reg-BdEVISc6.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/fonts/NimbusRomanNo9L/NimbusRomNo9L-RegIta.otf": () => A(() => import("./NimbusRomNo9L-RegIta-CuadmIOc.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/src/backmatter.typ": () => A(() => import("./backmatter-Cl2oryZN.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/src/body.typ": () => A(() => import("./body-DzgUhK25.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/src/config.typ": () => A(() => import("./config-CS-hYbJE.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/src/frontmatter.typ": () => A(() => import("./frontmatter-Ce5HsnVc.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/src/indorsement.typ": () => A(() => import("./indorsement-B0bwcoPJ.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/src/lib.typ": () => A(() => import("./lib-F2JYY-go.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/src/mainmatter.typ": () => A(() => import("./mainmatter-B8jQgBVi.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/src/primitives.typ": () => A(() => import("./primitives-ED0GTHw4.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/src/utils.typ": () => A(() => import("./utils-BVqhsr7R.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/packages/tonguetoquill-usaf-memo/typst.toml": () => A(() => import("./typst-Cx2F06ol.js"), []).then((r) => r.default),
    "../../../fixtures/resources/quills/usaf_memo/0.2.0/plate.typ": () => A(() => import("./plate-DrzDsrXP.js"), []).then((r) => r.default)
  });
  async function rf() {
    const r = /* @__PURE__ */ new Map(), e = "usaf_memo/0.2.0/";
    if (await Promise.all(Object.entries(nf).map(async ([t, n]) => {
      const i = t.indexOf(e);
      if (i < 0) return;
      const s = t.slice(i + e.length), o = await n(), l = new Uint8Array(await (await fetch(o)).arrayBuffer());
      r.set(s, l);
    })), !r.has("Quill.yaml")) throw new Error("usaf_memo fixture incomplete \u2014 Quill.yaml missing from glob");
    return r;
  }
  function sf(r) {
    if (!r) return {
      text: "",
      marks: []
    };
    const e = [];
    let t = "", n = 0;
    for (; n < r.length; ) if (r.startsWith("**", n)) {
      const i = r.indexOf("**", n + 2);
      if (i === -1) {
        t += r[n++];
        continue;
      }
      const s = t.length;
      t += r.slice(n + 2, i), e.push({
        start: s,
        end: t.length,
        type: "strong"
      }), n = i + 2;
    } else if (r[n] === "*") {
      const i = r.indexOf("*", n + 1);
      if (i === -1) {
        t += r[n++];
        continue;
      }
      const s = t.length;
      t += r.slice(n + 1, i), e.push({
        start: s,
        end: t.length,
        type: "emph"
      }), n = i + 1;
    } else t += r[n++];
    return {
      text: t,
      marks: e
    };
  }
  function Lr(r) {
    if (typeof r == "string") return sf(r);
    if (!r || typeof r != "object") return {
      text: "",
      marks: []
    };
    const e = r, t = e.text ?? "", n = (e.marks ?? []).flatMap((i) => {
      var _a2, _b;
      const s = i.type ?? ((_a2 = i.kind) == null ? void 0 : _a2.type) ?? "";
      if (!s || s === "anchor") return [];
      if (s === "link") {
        const o = i.url ?? ((_b = i.kind) == null ? void 0 : _b.url) ?? "";
        return [
          {
            start: i.start,
            end: i.end,
            type: "link",
            attrs: {
              href: o
            }
          }
        ];
      }
      return s === "emph" ? [
        {
          start: i.start,
          end: i.end,
          type: "emph"
        }
      ] : [
        {
          start: i.start,
          end: i.end,
          type: s
        }
      ];
    });
    return {
      text: t,
      marks: n
    };
  }
  function qr(r) {
    return Jc(r.text, r.marks);
  }
  function Wr(r) {
    return r ? r.endsWith(`
`) ? r.slice(0, -1) : r : "";
  }
  function of(r, e) {
    let t = 0;
    for (let n = 0; n < r.length; n++) {
      if (t === e) return n;
      const i = r.codePointAt(n);
      t += 1, i !== void 0 && i > 65535 && n++;
    }
    return r.length;
  }
  function lf(r, e, t) {
    const n = document.createElement("textarea");
    n.className = "body-editor", n.rows = 10, n.value = Wr(e), n.spellcheck = false, n.setAttribute("aria-label", "Memo body"), r.appendChild(n);
    let i = false;
    return n.addEventListener("input", () => {
      i || t(n.value);
    }), {
      el: n,
      getMarkdown() {
        return n.value;
      },
      focus() {
        n.focus();
      },
      setCursor(s) {
        const o = of(n.value, s);
        n.focus(), n.setSelectionRange(o, o);
      },
      setContent(s) {
        i = true, n.value = Wr(s), i = false;
      }
    };
  }
  const W = function(r) {
    for (var e = 0; ; e++) if (r = r.previousSibling, !r) return e;
  }, st = function(r) {
    let e = r.assignedSlot || r.parentNode;
    return e && e.nodeType == 11 ? e.host : e;
  };
  let Jn = null;
  const ye = function(r, e, t) {
    let n = Jn || (Jn = document.createRange());
    return n.setEnd(r, t ?? r.nodeValue.length), n.setStart(r, e || 0), n;
  }, af = function() {
    Jn = null;
  }, Ye = function(r, e, t, n) {
    return t && (Jr(r, e, t, n, -1) || Jr(r, e, t, n, 1));
  }, cf = /^(img|br|input|textarea|hr)$/i;
  function Jr(r, e, t, n, i) {
    for (var s; ; ) {
      if (r == t && e == n) return true;
      if (e == (i < 0 ? 0 : ie(r))) {
        let o = r.parentNode;
        if (!o || o.nodeType != 1 || At(r) || cf.test(r.nodeName) || r.contentEditable == "false") return false;
        e = W(r) + (i < 0 ? 0 : 1), r = o;
      } else if (r.nodeType == 1) {
        let o = r.childNodes[e + (i < 0 ? -1 : 0)];
        if (o.nodeType == 1 && o.contentEditable == "false") if (!((s = o.pmViewDesc) === null || s === void 0) && s.ignoreForSelection) e += i;
        else return false;
        else r = o, e = i < 0 ? ie(r) : 0;
      } else return false;
    }
  }
  function ie(r) {
    return r.nodeType == 3 ? r.nodeValue.length : r.childNodes.length;
  }
  function ff(r, e) {
    for (; ; ) {
      if (r.nodeType == 3 && e) return r;
      if (r.nodeType == 1 && e > 0) {
        if (r.contentEditable == "false") return null;
        r = r.childNodes[e - 1], e = ie(r);
      } else if (r.parentNode && !At(r)) e = W(r), r = r.parentNode;
      else return null;
    }
  }
  function df(r, e) {
    for (; ; ) {
      if (r.nodeType == 3 && e < r.nodeValue.length) return r;
      if (r.nodeType == 1 && e < r.childNodes.length) {
        if (r.contentEditable == "false") return null;
        r = r.childNodes[e], e = 0;
      } else if (r.parentNode && !At(r)) e = W(r) + 1, r = r.parentNode;
      else return null;
    }
  }
  function uf(r, e, t) {
    for (let n = e == 0, i = e == ie(r); n || i; ) {
      if (r == t) return true;
      let s = W(r);
      if (r = r.parentNode, !r) return false;
      n = n && s == 0, i = i && s == ie(r);
    }
  }
  function At(r) {
    let e;
    for (let t = r; t && !(e = t.pmViewDesc); t = t.parentNode) ;
    return e && e.node && e.node.isBlock && (e.dom == r || e.contentDOM == r);
  }
  const fn = function(r) {
    return r.focusNode && Ye(r.focusNode, r.focusOffset, r.anchorNode, r.anchorOffset);
  };
  function Fe(r, e) {
    let t = document.createEvent("Event");
    return t.initEvent("keydown", true, true), t.keyCode = r, t.key = t.code = e, t;
  }
  function hf(r) {
    let e = r.activeElement;
    for (; e && e.shadowRoot; ) e = e.shadowRoot.activeElement;
    return e;
  }
  function pf(r, e, t) {
    if (r.caretPositionFromPoint) try {
      let n = r.caretPositionFromPoint(e, t);
      if (n) return {
        node: n.offsetNode,
        offset: Math.min(ie(n.offsetNode), n.offset)
      };
    } catch {
    }
    if (r.caretRangeFromPoint) {
      let n = r.caretRangeFromPoint(e, t);
      if (n) return {
        node: n.startContainer,
        offset: Math.min(ie(n.startContainer), n.startOffset)
      };
    }
  }
  const ge = typeof navigator < "u" ? navigator : null, $r = typeof document < "u" ? document : null, Be = ge && ge.userAgent || "", $n = /Edge\/(\d+)/.exec(Be), gs = /MSIE \d/.exec(Be), jn = /Trident\/(?:[7-9]|\d{2,})\..*rv:(\d+)/.exec(Be), Z = !!(gs || jn || $n), De = gs ? document.documentMode : jn ? +jn[1] : $n ? +$n[1] : 0, se = !Z && /gecko\/(\d+)/i.test(Be);
  se && +(/Firefox\/(\d+)/.exec(Be) || [
    0,
    0
  ])[1];
  const Kn = !Z && /Chrome\/(\d+)/.exec(Be), F = !!Kn, _s = Kn ? +Kn[1] : 0, K = !Z && !!ge && /Apple Computer/.test(ge.vendor), ot = K && (/Mobile\/\w+/.test(Be) || !!ge && ge.maxTouchPoints > 2), re = ot || (ge ? /Mac/.test(ge.platform) : false), ys = ge ? /Win/.test(ge.platform) : false, ke = /Android \d/.test(Be), vt = !!$r && "webkitFontSmoothing" in $r.documentElement.style, mf = vt ? +(/\bAppleWebKit\/(\d+)/.exec(navigator.userAgent) || [
    0,
    0
  ])[1] : 0;
  function gf(r) {
    let e = r.defaultView && r.defaultView.visualViewport;
    return e ? {
      left: 0,
      right: e.width,
      top: 0,
      bottom: e.height
    } : {
      left: 0,
      right: r.documentElement.clientWidth,
      top: 0,
      bottom: r.documentElement.clientHeight
    };
  }
  function _e(r, e) {
    return typeof r == "number" ? r : r[e];
  }
  function _f(r) {
    let e = r.getBoundingClientRect(), t = e.width / r.offsetWidth || 1, n = e.height / r.offsetHeight || 1;
    return {
      left: e.left,
      right: e.left + r.clientWidth * t,
      top: e.top,
      bottom: e.top + r.clientHeight * n
    };
  }
  function jr(r, e, t) {
    let n = r.someProp("scrollThreshold") || 0, i = r.someProp("scrollMargin") || 5, s = r.dom.ownerDocument;
    for (let o = t || r.dom; o; ) {
      if (o.nodeType != 1) {
        o = st(o);
        continue;
      }
      let l = o, a = l == s.body, c = a ? gf(s) : _f(l), f = 0, d = 0;
      if (e.top < c.top + _e(n, "top") ? d = -(c.top - e.top + _e(i, "top")) : e.bottom > c.bottom - _e(n, "bottom") && (d = e.bottom - e.top > c.bottom - c.top ? e.top + _e(i, "top") - c.top : e.bottom - c.bottom + _e(i, "bottom")), e.left < c.left + _e(n, "left") ? f = -(c.left - e.left + _e(i, "left")) : e.right > c.right - _e(n, "right") && (f = e.right - c.right + _e(i, "right")), f || d) if (a) s.defaultView.scrollBy(f, d);
      else {
        let m = l.scrollLeft, p = l.scrollTop;
        d && (l.scrollTop += d), f && (l.scrollLeft += f);
        let _ = l.scrollLeft - m, y = l.scrollTop - p;
        e = {
          left: e.left - _,
          top: e.top - y,
          right: e.right - _,
          bottom: e.bottom - y
        };
      }
      let u = a ? "fixed" : getComputedStyle(o).position;
      if (/^(fixed|sticky)$/.test(u)) break;
      o = u == "absolute" ? o.offsetParent : st(o);
    }
  }
  function yf(r) {
    let e = r.dom.getBoundingClientRect(), t = Math.max(0, e.top), n, i;
    for (let s = (e.left + e.right) / 2, o = t + 1; o < Math.min(innerHeight, e.bottom); o += 5) {
      let l = r.root.elementFromPoint(s, o);
      if (!l || l == r.dom || !r.dom.contains(l)) continue;
      let a = l.getBoundingClientRect();
      if (a.top >= t - 20) {
        n = l, i = a.top;
        break;
      }
    }
    return {
      refDOM: n,
      refTop: i,
      stack: bs(r.dom)
    };
  }
  function bs(r) {
    let e = [], t = r.ownerDocument;
    for (let n = r; n && (e.push({
      dom: n,
      top: n.scrollTop,
      left: n.scrollLeft
    }), r != t); n = st(n)) ;
    return e;
  }
  function bf({ refDOM: r, refTop: e, stack: t }) {
    let n = r ? r.getBoundingClientRect().top : 0;
    ws(t, n == 0 ? 0 : n - e);
  }
  function ws(r, e) {
    for (let t = 0; t < r.length; t++) {
      let { dom: n, top: i, left: s } = r[t];
      n.scrollTop != i + e && (n.scrollTop = i + e), n.scrollLeft != s && (n.scrollLeft = s);
    }
  }
  let Xe = null;
  function wf(r) {
    if (r.setActive) return r.setActive();
    if (Xe) return r.focus(Xe);
    let e = bs(r);
    r.focus(Xe == null ? {
      get preventScroll() {
        return Xe = {
          preventScroll: true
        }, true;
      }
    } : void 0), Xe || (Xe = false, ws(e, 0));
  }
  function ks(r, e) {
    let t, n = 2e8, i, s = 0, o = e.top, l = e.top, a, c;
    for (let f = r.firstChild, d = 0; f; f = f.nextSibling, d++) {
      let u;
      if (f.nodeType == 1) u = f.getClientRects();
      else if (f.nodeType == 3) u = ye(f).getClientRects();
      else continue;
      for (let m = 0; m < u.length; m++) {
        let p = u[m];
        if (p.top <= o && p.bottom >= l) {
          o = Math.max(p.bottom, o), l = Math.min(p.top, l);
          let _ = p.left > e.left ? p.left - e.left : p.right < e.left ? e.left - p.right : 0;
          if (_ < n) {
            t = f, n = _, i = _ && t.nodeType == 3 ? {
              left: p.right < e.left ? p.right : p.left,
              top: e.top
            } : e, f.nodeType == 1 && _ && (s = d + (e.left >= (p.left + p.right) / 2 ? 1 : 0));
            continue;
          }
        } else p.top > e.top && !a && p.left <= e.left && p.right >= e.left && (a = f, c = {
          left: Math.max(p.left, Math.min(p.right, e.left)),
          top: p.top
        });
        !t && (e.left >= p.right && e.top >= p.top || e.left >= p.left && e.top >= p.bottom) && (s = d + 1);
      }
    }
    return !t && a && (t = a, i = c, n = 0), t && t.nodeType == 3 ? kf(t, i) : !t || n && t.nodeType == 1 ? {
      node: r,
      offset: s
    } : ks(t, i);
  }
  function kf(r, e) {
    let t = r.nodeValue.length, n = document.createRange(), i;
    for (let s = 0; s < t; s++) {
      n.setEnd(r, s + 1), n.setStart(r, s);
      let o = Ce(n, 1);
      if (o.top != o.bottom && ar(e, o)) {
        i = {
          node: r,
          offset: s + (e.left >= (o.left + o.right) / 2 ? 1 : 0)
        };
        break;
      }
    }
    return n.detach(), i || {
      node: r,
      offset: 0
    };
  }
  function ar(r, e) {
    return r.left >= e.left - 1 && r.left <= e.right + 1 && r.top >= e.top - 1 && r.top <= e.bottom + 1;
  }
  function xf(r, e) {
    let t = r.parentNode;
    return t && /^li$/i.test(t.nodeName) && e.left < r.getBoundingClientRect().left ? t : r;
  }
  function Sf(r, e, t) {
    let { node: n, offset: i } = ks(e, t), s = -1;
    if (n.nodeType == 1 && !n.firstChild) {
      let o = n.getBoundingClientRect();
      s = o.left != o.right && t.left > (o.left + o.right) / 2 ? 1 : -1;
    }
    return r.docView.posFromDOM(n, i, s);
  }
  function Cf(r, e, t, n) {
    let i = -1;
    for (let s = e, o = false; s != r.dom; ) {
      let l = r.docView.nearestDesc(s, true), a;
      if (!l) return null;
      if (l.dom.nodeType == 1 && (l.node.isBlock && l.parent || !l.contentDOM) && ((a = l.dom.getBoundingClientRect()).width || a.height) && (l.node.isBlock && l.parent && !/^T(R|BODY|HEAD|FOOT)$/.test(l.dom.nodeName) && (!o && a.left > n.left || a.top > n.top ? i = l.posBefore : (!o && a.right < n.left || a.bottom < n.top) && (i = l.posAfter), o = true), !l.contentDOM && i < 0 && !l.node.isText)) return (l.node.isBlock ? n.top < (a.top + a.bottom) / 2 : n.left < (a.left + a.right) / 2) ? l.posBefore : l.posAfter;
      s = l.dom.parentNode;
    }
    return i > -1 ? i : r.docView.posFromDOM(e, t, -1);
  }
  function xs(r, e, t) {
    let n = r.childNodes.length;
    if (n && t.top < t.bottom) for (let i = Math.max(0, Math.min(n - 1, Math.floor(n * (e.top - t.top) / (t.bottom - t.top)) - 2)), s = i; ; ) {
      let o = r.childNodes[s];
      if (o.nodeType == 1) {
        let l = o.getClientRects();
        for (let a = 0; a < l.length; a++) {
          let c = l[a];
          if (ar(e, c)) return xs(o, e, c);
        }
      }
      if ((s = (s + 1) % n) == i) break;
    }
    return r;
  }
  function Of(r, e) {
    let t = r.dom.ownerDocument, n, i = 0, s = pf(t, e.left, e.top);
    s && ({ node: n, offset: i } = s);
    let o = (r.root.elementFromPoint ? r.root : t).elementFromPoint(e.left, e.top), l;
    if (!o || !r.dom.contains(o.nodeType != 1 ? o.parentNode : o)) {
      let c = r.dom.getBoundingClientRect();
      if (!ar(e, c) || (o = xs(r.dom, e, c), !o)) return null;
    }
    if (K) for (let c = o; n && c; c = st(c)) c.draggable && (n = void 0);
    if (o = xf(o, e), n) {
      if (se && n.nodeType == 1 && (i = Math.min(i, n.childNodes.length), i < n.childNodes.length)) {
        let f = n.childNodes[i], d;
        f.nodeName == "IMG" && (d = f.getBoundingClientRect()).right <= e.left && d.bottom > e.top && i++;
      }
      let c;
      vt && i && n.nodeType == 1 && (c = n.childNodes[i - 1]).nodeType == 1 && c.contentEditable == "false" && c.getBoundingClientRect().top >= e.top && i--, n == r.dom && i == n.childNodes.length - 1 && n.lastChild.nodeType == 1 && e.top > n.lastChild.getBoundingClientRect().bottom ? l = r.state.doc.content.size : (i == 0 || n.nodeType != 1 || n.childNodes[i - 1].nodeName != "BR") && (l = Cf(r, n, i, e));
    }
    l == null && (l = Sf(r, o, e));
    let a = r.docView.nearestDesc(o, true);
    return {
      pos: l,
      inside: a ? a.posAtStart - a.border : -1
    };
  }
  function Kr(r) {
    return r.top < r.bottom || r.left < r.right;
  }
  function Ce(r, e) {
    let t = r.getClientRects();
    if (t.length) {
      let n = t[e < 0 ? 0 : t.length - 1];
      if (Kr(n)) return n;
    }
    return Array.prototype.find.call(t, Kr) || r.getBoundingClientRect();
  }
  const Mf = /[\u0590-\u05f4\u0600-\u06ff\u0700-\u08ac]/;
  function Ss(r, e, t) {
    let { node: n, offset: i, atom: s } = r.docView.domFromPos(e, t < 0 ? -1 : 1), o = vt || se;
    if (n.nodeType == 3) if (o && (Mf.test(n.nodeValue) || (t < 0 ? !i : i == n.nodeValue.length))) {
      let a = Ce(ye(n, i, i), t);
      if (se && i && /\s/.test(n.nodeValue[i - 1]) && i < n.nodeValue.length) {
        let c = Ce(ye(n, i - 1, i - 1), -1);
        if (c.top == a.top) {
          let f = Ce(ye(n, i, i + 1), -1);
          if (f.top != a.top) return ft(f, f.left < c.left);
        }
      }
      return a;
    } else {
      let a = i, c = i, f = t < 0 ? 1 : -1;
      return t < 0 && !i ? (c++, f = -1) : t >= 0 && i == n.nodeValue.length ? (a--, f = 1) : t < 0 ? a-- : c++, ft(Ce(ye(n, a, c), f), f < 0);
    }
    if (!r.state.doc.resolve(e - (s || 0)).parent.inlineContent) {
      if (s == null && i && (t < 0 || i == ie(n))) {
        let a = n.childNodes[i - 1];
        if (a.nodeType == 1) return Cn(a.getBoundingClientRect(), false);
      }
      if (s == null && i < ie(n)) {
        let a = n.childNodes[i];
        if (a.nodeType == 1) return Cn(a.getBoundingClientRect(), true);
      }
      return Cn(n.getBoundingClientRect(), t >= 0);
    }
    if (s == null && i && (t < 0 || i == ie(n))) {
      let a = n.childNodes[i - 1], c = a.nodeType == 3 ? ye(a, ie(a) - (o ? 0 : 1)) : a.nodeType == 1 && (a.nodeName != "BR" || !a.nextSibling) ? a : null;
      if (c) return ft(Ce(c, 1), false);
    }
    if (s == null && i < ie(n)) {
      let a = n.childNodes[i];
      for (; a.pmViewDesc && a.pmViewDesc.ignoreForCoords; ) a = a.nextSibling;
      let c = a ? a.nodeType == 3 ? ye(a, 0, o ? 0 : 1) : a.nodeType == 1 ? a : null : null;
      if (c) return ft(Ce(c, -1), true);
    }
    return ft(Ce(n.nodeType == 3 ? ye(n) : n, -t), t >= 0);
  }
  function ft(r, e) {
    if (r.width == 0) return r;
    let t = e ? r.left : r.right;
    return {
      top: r.top,
      bottom: r.bottom,
      left: t,
      right: t
    };
  }
  function Cn(r, e) {
    if (r.height == 0) return r;
    let t = e ? r.top : r.bottom;
    return {
      top: t,
      bottom: t,
      left: r.left,
      right: r.right
    };
  }
  function Cs(r, e, t) {
    let n = r.state, i = r.root.activeElement;
    n != e && r.updateState(e), i != r.dom && r.focus();
    try {
      return t();
    } finally {
      n != e && r.updateState(n), i != r.dom && i && i.focus();
    }
  }
  function Nf(r, e, t) {
    let n = e.selection, i = t == "up" ? n.$from : n.$to;
    return Cs(r, e, () => {
      let { node: s } = r.docView.domFromPos(i.pos, t == "up" ? -1 : 1);
      for (; ; ) {
        let l = r.docView.nearestDesc(s, true);
        if (!l) break;
        if (l.node.isBlock) {
          s = l.contentDOM || l.dom;
          break;
        }
        s = l.dom.parentNode;
      }
      let o = Ss(r, i.pos, 1);
      for (let l = s.firstChild; l; l = l.nextSibling) {
        let a;
        if (l.nodeType == 1) a = l.getClientRects();
        else if (l.nodeType == 3) a = ye(l, 0, l.nodeValue.length).getClientRects();
        else continue;
        for (let c = 0; c < a.length; c++) {
          let f = a[c];
          if (f.bottom > f.top + 1 && (t == "up" ? o.top - f.top > (f.bottom - o.top) * 2 : f.bottom - o.bottom > (o.bottom - f.top) * 2)) return false;
        }
      }
      return true;
    });
  }
  const Ef = /[\u0590-\u08ac]/;
  function Tf(r, e, t) {
    let { $head: n } = e.selection;
    if (!n.parent.isTextblock) return false;
    let i = n.parentOffset, s = !i, o = i == n.parent.content.size, l = r.domSelection();
    return l ? !Ef.test(n.parent.textContent) || !l.modify ? t == "left" || t == "backward" ? s : o : Cs(r, e, () => {
      let { focusNode: a, focusOffset: c, anchorNode: f, anchorOffset: d } = r.domSelectionRange(), u = l.caretBidiLevel;
      l.modify("move", t, "character");
      let m = n.depth ? r.docView.domAfterPos(n.before()) : r.dom, { focusNode: p, focusOffset: _ } = r.domSelectionRange(), y = p && !m.contains(p.nodeType == 1 ? p : p.parentNode) || a == p && c == _;
      try {
        l.collapse(f, d), a && (a != f || c != d) && l.extend && l.extend(a, c);
      } catch {
      }
      return u != null && (l.caretBidiLevel = u), y;
    }) : n.pos == n.start() || n.pos == n.end();
  }
  let Hr = null, Ur = null, Gr = false;
  function Df(r, e, t) {
    return Hr == e && Ur == t ? Gr : (Hr = e, Ur = t, Gr = t == "up" || t == "down" ? Nf(r, e, t) : Tf(r, e, t));
  }
  const oe = 0, Yr = 1, qe = 2, de = 3;
  class Rt {
    constructor(e, t, n, i) {
      this.parent = e, this.children = t, this.dom = n, this.contentDOM = i, this.dirty = oe, n.pmViewDesc = this;
    }
    matchesWidget(e) {
      return false;
    }
    matchesMark(e) {
      return false;
    }
    matchesNode(e, t, n) {
      return false;
    }
    matchesHack(e) {
      return false;
    }
    parseRule() {
      return null;
    }
    stopEvent(e) {
      return false;
    }
    get size() {
      let e = 0;
      for (let t = 0; t < this.children.length; t++) e += this.children[t].size;
      return e;
    }
    get border() {
      return 0;
    }
    destroy() {
      this.parent = void 0, this.dom.pmViewDesc == this && (this.dom.pmViewDesc = void 0);
      for (let e = 0; e < this.children.length; e++) this.children[e].destroy();
    }
    posBeforeChild(e) {
      for (let t = 0, n = this.posAtStart; ; t++) {
        let i = this.children[t];
        if (i == e) return n;
        n += i.size;
      }
    }
    get posBefore() {
      return this.parent.posBeforeChild(this);
    }
    get posAtStart() {
      return this.parent ? this.parent.posBeforeChild(this) + this.border : 0;
    }
    get posAfter() {
      return this.posBefore + this.size;
    }
    get posAtEnd() {
      return this.posAtStart + this.size - 2 * this.border;
    }
    localPosFromDOM(e, t, n) {
      if (this.contentDOM && this.contentDOM.contains(e.nodeType == 1 ? e : e.parentNode)) if (n < 0) {
        let s, o;
        if (e == this.contentDOM) s = e.childNodes[t - 1];
        else {
          for (; e.parentNode != this.contentDOM; ) e = e.parentNode;
          s = e.previousSibling;
        }
        for (; s && !((o = s.pmViewDesc) && o.parent == this); ) s = s.previousSibling;
        return s ? this.posBeforeChild(o) + o.size : this.posAtStart;
      } else {
        let s, o;
        if (e == this.contentDOM) s = e.childNodes[t];
        else {
          for (; e.parentNode != this.contentDOM; ) e = e.parentNode;
          s = e.nextSibling;
        }
        for (; s && !((o = s.pmViewDesc) && o.parent == this); ) s = s.nextSibling;
        return s ? this.posBeforeChild(o) : this.posAtEnd;
      }
      let i;
      if (e == this.dom && this.contentDOM) i = t > W(this.contentDOM);
      else if (this.contentDOM && this.contentDOM != this.dom && this.dom.contains(this.contentDOM)) i = e.compareDocumentPosition(this.contentDOM) & 2;
      else if (this.dom.firstChild) {
        if (t == 0) for (let s = e; ; s = s.parentNode) {
          if (s == this.dom) {
            i = false;
            break;
          }
          if (s.previousSibling) break;
        }
        if (i == null && t == e.childNodes.length) for (let s = e; ; s = s.parentNode) {
          if (s == this.dom) {
            i = true;
            break;
          }
          if (s.nextSibling) break;
        }
      }
      return i ?? n > 0 ? this.posAtEnd : this.posAtStart;
    }
    nearestDesc(e, t = false) {
      for (let n = true, i = e; i; i = i.parentNode) {
        let s = this.getDesc(i), o;
        if (s && (!t || s.node)) if (n && (o = s.nodeDOM) && !(o.nodeType == 1 ? o.contains(e.nodeType == 1 ? e : e.parentNode) : o == e)) n = false;
        else return s;
      }
    }
    getDesc(e) {
      let t = e.pmViewDesc;
      for (let n = t; n; n = n.parent) if (n == this) return t;
    }
    posFromDOM(e, t, n) {
      for (let i = e; i; i = i.parentNode) {
        let s = this.getDesc(i);
        if (s) return s.localPosFromDOM(e, t, n);
      }
      return -1;
    }
    descAt(e) {
      for (let t = 0, n = 0; t < this.children.length; t++) {
        let i = this.children[t], s = n + i.size;
        if (n == e && s != n) {
          for (; !i.border && i.children.length; ) for (let o = 0; o < i.children.length; o++) {
            let l = i.children[o];
            if (l.size) {
              i = l;
              break;
            }
          }
          return i;
        }
        if (e < s) return i.descAt(e - n - i.border);
        n = s;
      }
    }
    domFromPos(e, t) {
      if (!this.contentDOM) return {
        node: this.dom,
        offset: 0,
        atom: e + 1
      };
      let n = 0, i = 0;
      for (let s = 0; n < this.children.length; n++) {
        let o = this.children[n], l = s + o.size;
        if (l > e || o instanceof Ms) {
          i = e - s;
          break;
        }
        s = l;
      }
      if (i) return this.children[n].domFromPos(i - this.children[n].border, t);
      for (let s; n && !(s = this.children[n - 1]).size && s instanceof Os && s.side >= 0; n--) ;
      if (t <= 0) {
        let s, o = true;
        for (; s = n ? this.children[n - 1] : null, !(!s || s.dom.parentNode == this.contentDOM); n--, o = false) ;
        return s && t && o && !s.border && !s.domAtom ? s.domFromPos(s.size, t) : {
          node: this.contentDOM,
          offset: s ? W(s.dom) + 1 : 0
        };
      } else {
        let s, o = true;
        for (; s = n < this.children.length ? this.children[n] : null, !(!s || s.dom.parentNode == this.contentDOM); n++, o = false) ;
        return s && o && !s.border && !s.domAtom ? s.domFromPos(0, t) : {
          node: this.contentDOM,
          offset: s ? W(s.dom) : this.contentDOM.childNodes.length
        };
      }
    }
    parseRange(e, t, n = 0) {
      if (this.children.length == 0) return {
        node: this.contentDOM,
        from: e,
        to: t,
        fromOffset: 0,
        toOffset: this.contentDOM.childNodes.length
      };
      let i = -1, s = -1;
      for (let o = n, l = 0; ; l++) {
        let a = this.children[l], c = o + a.size;
        if (i == -1 && e <= c) {
          let f = o + a.border;
          if (e >= f && t <= c - a.border && a.node && a.contentDOM && this.contentDOM.contains(a.contentDOM)) return a.parseRange(e, t, f);
          e = o;
          for (let d = l; d > 0; d--) {
            let u = this.children[d - 1];
            if (u.size && u.dom.parentNode == this.contentDOM && !u.emptyChildAt(1)) {
              i = W(u.dom) + 1;
              break;
            }
            e -= u.size;
          }
          i == -1 && (i = 0);
        }
        if (i > -1 && (c > t || l == this.children.length - 1)) {
          t = c;
          for (let f = l + 1; f < this.children.length; f++) {
            let d = this.children[f];
            if (d.size && d.dom.parentNode == this.contentDOM && !d.emptyChildAt(-1)) {
              s = W(d.dom);
              break;
            }
            t += d.size;
          }
          s == -1 && (s = this.contentDOM.childNodes.length);
          break;
        }
        o = c;
      }
      return {
        node: this.contentDOM,
        from: e,
        to: t,
        fromOffset: i,
        toOffset: s
      };
    }
    emptyChildAt(e) {
      if (this.border || !this.contentDOM || !this.children.length) return false;
      let t = this.children[e < 0 ? 0 : this.children.length - 1];
      return t.size == 0 || t.emptyChildAt(e);
    }
    domAfterPos(e) {
      let { node: t, offset: n } = this.domFromPos(e, 0);
      if (t.nodeType != 1 || n == t.childNodes.length) throw new RangeError("No node after pos " + e);
      return t.childNodes[n];
    }
    setSelection(e, t, n, i = false) {
      let s = Math.min(e, t), o = Math.max(e, t);
      for (let m = 0, p = 0; m < this.children.length; m++) {
        let _ = this.children[m], y = p + _.size;
        if (s > p && o < y) return _.setSelection(e - p - _.border, t - p - _.border, n, i);
        p = y;
      }
      let l = this.domFromPos(e, e ? -1 : 1), a = t == e ? l : this.domFromPos(t, t ? -1 : 1), c = n.root.getSelection(), f = n.domSelectionRange(), d = false;
      if ((se || K) && e == t) {
        let { node: m, offset: p } = l;
        if (m.nodeType == 3) {
          if (d = !!(p && m.nodeValue[p - 1] == `
`), d && p == m.nodeValue.length) for (let _ = m, y; _; _ = _.parentNode) {
            if (y = _.nextSibling) {
              y.nodeName == "BR" && (l = a = {
                node: y.parentNode,
                offset: W(y) + 1
              });
              break;
            }
            let S = _.pmViewDesc;
            if (S && S.node && S.node.isBlock) break;
          }
        } else {
          let _ = m.childNodes[p - 1];
          d = _ && (_.nodeName == "BR" || _.contentEditable == "false");
        }
      }
      if (se && f.focusNode && f.focusNode != a.node && f.focusNode.nodeType == 1) {
        let m = f.focusNode.childNodes[f.focusOffset];
        m && m.contentEditable == "false" && (i = true);
      }
      if (!(i || d && K) && Ye(l.node, l.offset, f.anchorNode, f.anchorOffset) && Ye(a.node, a.offset, f.focusNode, f.focusOffset)) return;
      let u = false;
      if ((c.extend || e == t) && !(d && se)) {
        c.collapse(l.node, l.offset);
        try {
          e != t && c.extend(a.node, a.offset), u = true;
        } catch {
        }
      }
      if (!u) {
        if (e > t) {
          let p = l;
          l = a, a = p;
        }
        let m = document.createRange();
        m.setEnd(a.node, a.offset), m.setStart(l.node, l.offset), c.removeAllRanges(), c.addRange(m);
      }
    }
    ignoreMutation(e) {
      return !this.contentDOM && e.type != "selection";
    }
    get contentLost() {
      return this.contentDOM && this.contentDOM != this.dom && !this.dom.contains(this.contentDOM);
    }
    markDirty(e, t) {
      for (let n = 0, i = 0; i < this.children.length; i++) {
        let s = this.children[i], o = n + s.size;
        if (n == o ? e <= o && t >= n : e < o && t > n) {
          let l = n + s.border, a = o - s.border;
          if (e >= l && t <= a) {
            this.dirty = e == n || t == o ? qe : Yr, e == l && t == a && (s.contentLost || s.dom.parentNode != this.contentDOM) ? s.dirty = de : s.markDirty(e - l, t - l);
            return;
          } else s.dirty = s.dom == s.contentDOM && s.dom.parentNode == this.contentDOM && !s.children.length ? qe : de;
        }
        n = o;
      }
      this.dirty = qe;
    }
    markParentsDirty() {
      let e = 1;
      for (let t = this.parent; t; t = t.parent, e++) {
        let n = e == 1 ? qe : Yr;
        t.dirty < n && (t.dirty = n);
      }
    }
    get domAtom() {
      return false;
    }
    get ignoreForCoords() {
      return false;
    }
    get ignoreForSelection() {
      return false;
    }
    isText(e) {
      return false;
    }
  }
  class Os extends Rt {
    constructor(e, t, n, i) {
      let s, o = t.type.toDOM;
      if (typeof o == "function" && (o = o(n, () => {
        if (!s) return i;
        if (s.parent) return s.parent.posBeforeChild(s);
      })), !t.type.spec.raw) {
        if (o.nodeType != 1) {
          let l = document.createElement("span");
          l.appendChild(o), o = l;
        }
        o.contentEditable = "false", o.classList.add("ProseMirror-widget");
      }
      super(e, [], o, null), this.widget = t, this.widget = t, s = this;
    }
    matchesWidget(e) {
      return this.dirty == oe && e.type.eq(this.widget.type);
    }
    parseRule() {
      return {
        ignore: true
      };
    }
    stopEvent(e) {
      let t = this.widget.spec.stopEvent;
      return t ? t(e) : false;
    }
    ignoreMutation(e) {
      return e.type != "selection" || this.widget.spec.ignoreSelection;
    }
    destroy() {
      this.widget.type.destroy(this.dom), super.destroy();
    }
    get domAtom() {
      return true;
    }
    get ignoreForSelection() {
      return !!this.widget.type.spec.relaxedSide;
    }
    get side() {
      return this.widget.type.side;
    }
  }
  class If extends Rt {
    constructor(e, t, n, i) {
      super(e, [], t, null), this.textDOM = n, this.text = i;
    }
    get size() {
      return this.text.length;
    }
    localPosFromDOM(e, t) {
      return e != this.textDOM ? this.posAtStart + (t ? this.size : 0) : this.posAtStart + t;
    }
    domFromPos(e) {
      return {
        node: this.textDOM,
        offset: e
      };
    }
    ignoreMutation(e) {
      return e.type === "characterData" && e.target.nodeValue == e.oldValue;
    }
  }
  class Ie extends Rt {
    constructor(e, t, n, i, s) {
      super(e, [], n, i), this.mark = t, this.spec = s;
    }
    static create(e, t, n, i) {
      let s = i.nodeViews[t.type.name], o = s && s(t, i, n);
      return (!o || !o.dom) && (o = at.renderSpec(document, t.type.spec.toDOM(t, n), null, t.attrs)), new Ie(e, t, o.dom, o.contentDOM || o.dom, o);
    }
    parseRule() {
      return this.dirty & de || this.mark.type.spec.reparseInView ? null : {
        mark: this.mark.type.name,
        attrs: this.mark.attrs,
        contentElement: this.contentDOM
      };
    }
    matchesMark(e) {
      return this.dirty != de && this.mark.eq(e);
    }
    markDirty(e, t) {
      if (super.markDirty(e, t), this.dirty != oe) {
        let n = this.parent;
        for (; !n.node; ) n = n.parent;
        n.dirty < this.dirty && (n.dirty = this.dirty), this.dirty = oe;
      }
    }
    slice(e, t, n) {
      let i = Ie.create(this.parent, this.mark, true, n), s = this.children, o = this.size;
      t < o && (s = Un(s, t, o, n)), e > 0 && (s = Un(s, 0, e, n));
      for (let l = 0; l < s.length; l++) s[l].parent = i;
      return i.children = s, i;
    }
    ignoreMutation(e) {
      return this.spec.ignoreMutation ? this.spec.ignoreMutation(e) : super.ignoreMutation(e);
    }
    destroy() {
      this.spec.destroy && this.spec.destroy(), super.destroy();
    }
  }
  class Ae extends Rt {
    constructor(e, t, n, i, s, o, l) {
      super(e, [], s, o), this.node = t, this.outerDeco = n, this.innerDeco = i, this.nodeDOM = l;
    }
    static create(e, t, n, i, s, o) {
      let l = s.nodeViews[t.type.name], a, c = l && l(t, s, () => {
        if (!a) return o;
        if (a.parent) return a.parent.posBeforeChild(a);
      }, n, i), f = c && c.dom, d = c && c.contentDOM;
      if (t.isText) {
        if (!f) f = document.createTextNode(t.text);
        else if (f.nodeType != 3) throw new RangeError("Text must be rendered as a DOM text node");
      } else f || ({ dom: f, contentDOM: d } = at.renderSpec(document, t.type.spec.toDOM(t), null, t.attrs));
      !d && !t.isText && f.nodeName != "BR" && (f.hasAttribute("contenteditable") || (f.contentEditable = "false"), t.type.spec.draggable && (f.draggable = true));
      let u = f;
      return f = Ts(f, n, t), c ? a = new Af(e, t, n, i, f, d || null, u, c) : t.isText ? new dn(e, t, n, i, f, u) : new Ae(e, t, n, i, f, d || null, u);
    }
    parseRule() {
      if (this.node.type.spec.reparseInView) return null;
      let e = {
        node: this.node.type.name,
        attrs: this.node.attrs
      };
      if (this.node.type.whitespace == "pre" && (e.preserveWhitespace = "full"), !this.contentDOM) e.getContent = () => this.node.content;
      else if (!this.contentLost) e.contentElement = this.contentDOM;
      else {
        for (let t = this.children.length - 1; t >= 0; t--) {
          let n = this.children[t];
          if (this.dom.contains(n.dom.parentNode)) {
            e.contentElement = n.dom.parentNode;
            break;
          }
        }
        e.contentElement || (e.getContent = () => b.empty);
      }
      return e;
    }
    matchesNode(e, t, n) {
      return this.dirty == oe && e.eq(this.node) && Zt(t, this.outerDeco) && n.eq(this.innerDeco);
    }
    get size() {
      return this.node.nodeSize;
    }
    get border() {
      return this.node.isLeaf ? 0 : 1;
    }
    updateChildren(e, t) {
      let n = this.node.inlineContent, i = t, s = e.composing ? this.localCompositionInfo(e, t) : null, o = s && s.pos > -1 ? s : null, l = s && s.pos < 0, a = new Rf(this, o && o.node, e);
      zf(this.node, this.innerDeco, (c, f, d) => {
        c.spec.marks ? a.syncToMarks(c.spec.marks, n, e, f) : c.type.side >= 0 && !d && a.syncToMarks(f == this.node.childCount ? T.none : this.node.child(f).marks, n, e, f), a.placeWidget(c, e, i);
      }, (c, f, d, u) => {
        a.syncToMarks(c.marks, n, e, u);
        let m;
        a.findNodeMatch(c, f, d, u) || l && e.state.selection.from > i && e.state.selection.to < i + c.nodeSize && (m = a.findIndexWithChild(s.node)) > -1 && a.updateNodeAt(c, f, d, m, e) || a.updateNextNode(c, f, d, e, u, i) || a.addNode(c, f, d, e, i), i += c.nodeSize;
      }), a.syncToMarks([], n, e, 0), this.node.isTextblock && a.addTextblockHacks(), a.destroyRest(), (a.changed || this.dirty == qe) && (o && this.protectLocalComposition(e, o), Ns(this.contentDOM, this.children, e), ot && Ff(this.dom));
    }
    localCompositionInfo(e, t) {
      let { from: n, to: i } = e.state.selection;
      if (!(e.state.selection instanceof E) || n < t || i > t + this.node.content.size) return null;
      let s = e.input.compositionNode;
      if (!s || !this.dom.contains(s.parentNode)) return null;
      if (this.node.inlineContent) {
        let o = s.nodeValue, l = Vf(this.node.content, o, n - t, i - t);
        return l < 0 ? null : {
          node: s,
          pos: l,
          text: o
        };
      } else return {
        node: s,
        pos: -1,
        text: ""
      };
    }
    protectLocalComposition(e, { node: t, pos: n, text: i }) {
      if (this.getDesc(t)) return;
      let s = t;
      for (; s.parentNode != this.contentDOM; s = s.parentNode) {
        for (; s.previousSibling; ) s.parentNode.removeChild(s.previousSibling);
        for (; s.nextSibling; ) s.parentNode.removeChild(s.nextSibling);
        s.pmViewDesc && (s.pmViewDesc = void 0);
      }
      let o = new If(this, s, t, i);
      e.input.compositionNodes.push(o), this.children = Un(this.children, n, n + i.length, e, o);
    }
    update(e, t, n, i) {
      return this.dirty == de || !e.sameMarkup(this.node) ? false : (this.updateInner(e, t, n, i), true);
    }
    updateInner(e, t, n, i) {
      this.updateOuterDeco(t), this.node = e, this.innerDeco = n, this.contentDOM && this.updateChildren(i, this.posAtStart), this.dirty = oe;
    }
    updateOuterDeco(e) {
      if (Zt(e, this.outerDeco)) return;
      let t = this.nodeDOM.nodeType != 1, n = this.dom;
      this.dom = Es(this.dom, this.nodeDOM, Hn(this.outerDeco, this.node, t), Hn(e, this.node, t)), this.dom != n && (n.pmViewDesc = void 0, this.dom.pmViewDesc = this), this.outerDeco = e;
    }
    selectNode() {
      this.nodeDOM.nodeType == 1 && (this.nodeDOM.classList.add("ProseMirror-selectednode"), (this.contentDOM || !this.node.type.spec.draggable) && (this.nodeDOM.draggable = true));
    }
    deselectNode() {
      this.nodeDOM.nodeType == 1 && (this.nodeDOM.classList.remove("ProseMirror-selectednode"), (this.contentDOM || !this.node.type.spec.draggable) && this.nodeDOM.removeAttribute("draggable"));
    }
    get domAtom() {
      return this.node.isAtom;
    }
  }
  function Qr(r, e, t, n, i) {
    Ts(n, e, r);
    let s = new Ae(void 0, r, e, t, n, n, n);
    return s.contentDOM && s.updateChildren(i, 0), s;
  }
  class dn extends Ae {
    constructor(e, t, n, i, s, o) {
      super(e, t, n, i, s, null, o);
    }
    parseRule() {
      let e = this.nodeDOM.parentNode;
      for (; e && e != this.dom && !e.pmIsDeco; ) e = e.parentNode;
      return {
        skip: e || true
      };
    }
    update(e, t, n, i) {
      return this.dirty == de || this.dirty != oe && !this.inParent() || !e.sameMarkup(this.node) ? false : (this.updateOuterDeco(t), (this.dirty != oe || e.text != this.node.text) && e.text != this.nodeDOM.nodeValue && (this.nodeDOM.nodeValue = e.text, i.trackWrites == this.nodeDOM && (i.trackWrites = null)), this.node = e, this.dirty = oe, true);
    }
    inParent() {
      let e = this.parent.contentDOM;
      for (let t = this.nodeDOM; t; t = t.parentNode) if (t == e) return true;
      return false;
    }
    domFromPos(e) {
      return {
        node: this.nodeDOM,
        offset: e
      };
    }
    localPosFromDOM(e, t, n) {
      return e == this.nodeDOM ? this.posAtStart + Math.min(t, this.node.text.length) : super.localPosFromDOM(e, t, n);
    }
    ignoreMutation(e) {
      return e.type != "characterData" && e.type != "selection";
    }
    slice(e, t, n) {
      let i = this.node.cut(e, t), s = document.createTextNode(i.text);
      return new dn(this.parent, i, this.outerDeco, this.innerDeco, s, s);
    }
    markDirty(e, t) {
      super.markDirty(e, t), this.dom != this.nodeDOM && (e == 0 || t == this.nodeDOM.nodeValue.length) && (this.dirty = de);
    }
    get domAtom() {
      return false;
    }
    isText(e) {
      return this.node.text == e;
    }
  }
  class Ms extends Rt {
    parseRule() {
      return {
        ignore: true
      };
    }
    matchesHack(e) {
      return this.dirty == oe && this.dom.nodeName == e;
    }
    get domAtom() {
      return true;
    }
    get ignoreForCoords() {
      return this.dom.nodeName == "IMG";
    }
  }
  class Af extends Ae {
    constructor(e, t, n, i, s, o, l, a) {
      super(e, t, n, i, s, o, l), this.spec = a;
    }
    update(e, t, n, i) {
      if (this.dirty == de) return false;
      if (this.spec.update && (this.node.type == e.type || this.spec.multiType)) {
        let s = this.spec.update(e, t, n);
        return s && this.updateInner(e, t, n, i), s;
      } else return !this.contentDOM && !e.isLeaf ? false : super.update(e, t, n, i);
    }
    selectNode() {
      this.spec.selectNode ? this.spec.selectNode() : super.selectNode();
    }
    deselectNode() {
      this.spec.deselectNode ? this.spec.deselectNode() : super.deselectNode();
    }
    setSelection(e, t, n, i) {
      this.spec.setSelection ? this.spec.setSelection(e, t, n.root) : super.setSelection(e, t, n, i);
    }
    destroy() {
      this.spec.destroy && this.spec.destroy(), super.destroy();
    }
    stopEvent(e) {
      return this.spec.stopEvent ? this.spec.stopEvent(e) : false;
    }
    ignoreMutation(e) {
      return this.spec.ignoreMutation ? this.spec.ignoreMutation(e) : super.ignoreMutation(e);
    }
  }
  function Ns(r, e, t) {
    let n = r.firstChild, i = false;
    for (let s = 0; s < e.length; s++) {
      let o = e[s], l = o.dom;
      if (l.parentNode == r) {
        for (; l != n; ) n = Xr(n), i = true;
        n = n.nextSibling;
      } else i = true, r.insertBefore(l, n);
      if (o instanceof Ie) {
        let a = n ? n.previousSibling : r.lastChild;
        Ns(o.contentDOM, o.children, t), n = a ? a.nextSibling : r.firstChild;
      }
    }
    for (; n; ) n = Xr(n), i = true;
    i && t.trackWrites == r && (t.trackWrites = null);
  }
  const bt = function(r) {
    r && (this.nodeName = r);
  };
  bt.prototype = /* @__PURE__ */ Object.create(null);
  const We = [
    new bt()
  ];
  function Hn(r, e, t) {
    if (r.length == 0) return We;
    let n = t ? We[0] : new bt(), i = [
      n
    ];
    for (let s = 0; s < r.length; s++) {
      let o = r[s].type.attrs;
      if (o) {
        o.nodeName && i.push(n = new bt(o.nodeName));
        for (let l in o) {
          let a = o[l];
          a != null && (t && i.length == 1 && i.push(n = new bt(e.isInline ? "span" : "div")), l == "class" ? n.class = (n.class ? n.class + " " : "") + a : l == "style" ? n.style = (n.style ? n.style + ";" : "") + a : l != "nodeName" && (n[l] = a));
        }
      }
    }
    return i;
  }
  function Es(r, e, t, n) {
    if (t == We && n == We) return e;
    let i = e;
    for (let s = 0; s < n.length; s++) {
      let o = n[s], l = t[s];
      if (s) {
        let a;
        l && l.nodeName == o.nodeName && i != r && (a = i.parentNode) && a.nodeName.toLowerCase() == o.nodeName || (a = document.createElement(o.nodeName), a.pmIsDeco = true, a.appendChild(i), l = We[0]), i = a;
      }
      vf(i, l || We[0], o);
    }
    return i;
  }
  function vf(r, e, t) {
    for (let n in e) n != "class" && n != "style" && n != "nodeName" && !(n in t) && r.removeAttribute(n);
    for (let n in t) n != "class" && n != "style" && n != "nodeName" && t[n] != e[n] && r.setAttribute(n, t[n]);
    if (e.class != t.class) {
      let n = e.class ? e.class.split(" ").filter(Boolean) : [], i = t.class ? t.class.split(" ").filter(Boolean) : [];
      for (let s = 0; s < n.length; s++) i.indexOf(n[s]) == -1 && r.classList.remove(n[s]);
      for (let s = 0; s < i.length; s++) n.indexOf(i[s]) == -1 && r.classList.add(i[s]);
      r.classList.length == 0 && r.removeAttribute("class");
    }
    if (e.style != t.style) {
      if (e.style) {
        let n = /\s*([\w\-\xa1-\uffff]+)\s*:(?:"(?:\\.|[^"])*"|'(?:\\.|[^'])*'|\(.*?\)|[^;])*/g, i;
        for (; i = n.exec(e.style); ) r.style.removeProperty(i[1]);
      }
      t.style && (r.style.cssText += t.style);
    }
  }
  function Ts(r, e, t) {
    return Es(r, r, We, Hn(e, t, r.nodeType != 1));
  }
  function Zt(r, e) {
    if (r.length != e.length) return false;
    for (let t = 0; t < r.length; t++) if (!r[t].type.eq(e[t].type)) return false;
    return true;
  }
  function Xr(r) {
    let e = r.nextSibling;
    return r.parentNode.removeChild(r), e;
  }
  class Rf {
    constructor(e, t, n) {
      this.lock = t, this.view = n, this.index = 0, this.stack = [], this.changed = false, this.top = e, this.preMatch = Pf(e.node.content, e);
    }
    destroyBetween(e, t) {
      if (e != t) {
        for (let n = e; n < t; n++) this.top.children[n].destroy();
        this.top.children.splice(e, t - e), this.changed = true;
      }
    }
    destroyRest() {
      this.destroyBetween(this.index, this.top.children.length);
    }
    syncToMarks(e, t, n, i) {
      let s = 0, o = this.stack.length >> 1, l = Math.min(o, e.length);
      for (; s < l && (s == o - 1 ? this.top : this.stack[s + 1 << 1]).matchesMark(e[s]) && e[s].type.spec.spanning !== false; ) s++;
      for (; s < o; ) this.destroyRest(), this.top.dirty = oe, this.index = this.stack.pop(), this.top = this.stack.pop(), o--;
      for (; o < e.length; ) {
        this.stack.push(this.top, this.index + 1);
        let a = -1, c = this.top.children.length;
        i < this.preMatch.index && (c = Math.min(this.index + 3, c));
        for (let f = this.index; f < c; f++) {
          let d = this.top.children[f];
          if (d.matchesMark(e[o]) && !this.isLocked(d.dom)) {
            a = f;
            break;
          }
        }
        if (a < 0 && this.index < this.top.children.length) {
          let f = this.top.children[this.index];
          f instanceof Ie && f.dirty != de && f.mark.type == e[o].type && f.spec.update && !this.isLocked(f.dom) && f.spec.update(e[o]) && (f.mark = e[o], a = this.index, this.changed = true);
        }
        if (a > -1) a > this.index && (this.changed = true, this.destroyBetween(this.index, a)), this.top = this.top.children[this.index];
        else {
          let f = Ie.create(this.top, e[o], t, n);
          this.top.children.splice(this.index, 0, f), this.top = f, this.changed = true;
        }
        this.index = 0, o++;
      }
    }
    findNodeMatch(e, t, n, i) {
      let s = -1, o;
      if (i >= this.preMatch.index && (o = this.preMatch.matches[i - this.preMatch.index]).parent == this.top && o.matchesNode(e, t, n)) s = this.top.children.indexOf(o, this.index);
      else for (let l = this.index, a = Math.min(this.top.children.length, l + 5); l < a; l++) {
        let c = this.top.children[l];
        if (c.matchesNode(e, t, n) && !this.preMatch.matched.has(c)) {
          s = l;
          break;
        }
      }
      return s < 0 ? false : (this.destroyBetween(this.index, s), this.index++, true);
    }
    updateNodeAt(e, t, n, i, s) {
      let o = this.top.children[i];
      return o.dirty == de && o.dom == o.contentDOM && (o.dirty = qe), o.update(e, t, n, s) ? (this.destroyBetween(this.index, i), this.index++, true) : false;
    }
    findIndexWithChild(e) {
      for (; ; ) {
        let t = e.parentNode;
        if (!t) return -1;
        if (t == this.top.contentDOM) {
          let n = e.pmViewDesc;
          if (n) {
            for (let i = this.index; i < this.top.children.length; i++) if (this.top.children[i] == n) return i;
          }
          return -1;
        }
        e = t;
      }
    }
    updateNextNode(e, t, n, i, s, o) {
      for (let l = this.index; l < this.top.children.length; l++) {
        let a = this.top.children[l];
        if (a instanceof Ae) {
          let c = this.preMatch.matched.get(a);
          if (c != null && c != s) return false;
          let f = a.dom, d, u = this.isLocked(f) && !(e.isText && a.node && a.node.isText && a.nodeDOM.nodeValue == e.text && a.dirty != de && Zt(t, a.outerDeco));
          if (!u && a.update(e, t, n, i)) return this.destroyBetween(this.index, l), a.dom != f && (this.changed = true), this.index++, true;
          if (!u && (d = this.recreateWrapper(a, e, t, n, i, o))) return this.destroyBetween(this.index, l), this.top.children[this.index] = d, d.contentDOM && (d.dirty = qe, d.updateChildren(i, o + 1), d.dirty = oe), this.changed = true, this.index++, true;
          break;
        }
      }
      return false;
    }
    recreateWrapper(e, t, n, i, s, o) {
      if (e.dirty || t.isAtom || !e.children.length || !e.node.content.eq(t.content) || !Zt(n, e.outerDeco) || !i.eq(e.innerDeco)) return null;
      let l = Ae.create(this.top, t, n, i, s, o);
      if (l.contentDOM) {
        l.children = e.children, e.children = [];
        for (let a of l.children) a.parent = l;
      }
      return e.destroy(), l;
    }
    addNode(e, t, n, i, s) {
      let o = Ae.create(this.top, e, t, n, i, s);
      o.contentDOM && o.updateChildren(i, s + 1), this.top.children.splice(this.index++, 0, o), this.changed = true;
    }
    placeWidget(e, t, n) {
      let i = this.index < this.top.children.length ? this.top.children[this.index] : null;
      if (i && i.matchesWidget(e) && (e == i.widget || !i.widget.type.toDOM.parentNode)) this.index++;
      else {
        let s = new Os(this.top, e, t, n);
        this.top.children.splice(this.index++, 0, s), this.changed = true;
      }
    }
    addTextblockHacks() {
      let e = this.top.children[this.index - 1], t = this.top;
      for (; e instanceof Ie; ) t = e, e = t.children[t.children.length - 1];
      (!e || !(e instanceof dn) || /\n$/.test(e.node.text) || this.view.requiresGeckoHackNode && /\s$/.test(e.node.text)) && ((K || F) && e && e.dom.contentEditable == "false" && this.addHackNode("IMG", t), this.addHackNode("BR", this.top));
    }
    addHackNode(e, t) {
      if (t == this.top && this.index < t.children.length && t.children[this.index].matchesHack(e)) this.index++;
      else {
        let n = document.createElement(e);
        e == "IMG" && (n.className = "ProseMirror-separator", n.alt = ""), e == "BR" && (n.className = "ProseMirror-trailingBreak");
        let i = new Ms(this.top, [], n, null);
        t != this.top ? t.children.push(i) : t.children.splice(this.index++, 0, i), this.changed = true;
      }
    }
    isLocked(e) {
      return this.lock && (e == this.lock || e.nodeType == 1 && e.contains(this.lock.parentNode));
    }
  }
  function Pf(r, e) {
    let t = e, n = t.children.length, i = r.childCount, s = /* @__PURE__ */ new Map(), o = [];
    e: for (; i > 0; ) {
      let l;
      for (; ; ) if (n) {
        let c = t.children[n - 1];
        if (c instanceof Ie) t = c, n = c.children.length;
        else {
          l = c, n--;
          break;
        }
      } else {
        if (t == e) break e;
        n = t.parent.children.indexOf(t), t = t.parent;
      }
      let a = l.node;
      if (a) {
        if (a != r.child(i - 1)) break;
        --i, s.set(l, i), o.push(l);
      }
    }
    return {
      index: i,
      matched: s,
      matches: o.reverse()
    };
  }
  function Bf(r, e) {
    return r.type.side - e.type.side;
  }
  function zf(r, e, t, n) {
    let i = e.locals(r), s = 0;
    if (i.length == 0) {
      for (let c = 0; c < r.childCount; c++) {
        let f = r.child(c);
        n(f, i, e.forChild(s, f), c), s += f.nodeSize;
      }
      return;
    }
    let o = 0, l = [], a = null;
    for (let c = 0; ; ) {
      let f, d;
      for (; o < i.length && i[o].to == s; ) {
        let y = i[o++];
        y.widget && (f ? (d || (d = [
          f
        ])).push(y) : f = y);
      }
      if (f) if (d) {
        d.sort(Bf);
        for (let y = 0; y < d.length; y++) t(d[y], c, !!a);
      } else t(f, c, !!a);
      let u, m;
      if (a) m = -1, u = a, a = null;
      else if (c < r.childCount) m = c, u = r.child(c++);
      else break;
      for (let y = 0; y < l.length; y++) l[y].to <= s && l.splice(y--, 1);
      for (; o < i.length && i[o].from <= s && i[o].to > s; ) l.push(i[o++]);
      let p = s + u.nodeSize;
      if (u.isText) {
        let y = p;
        o < i.length && i[o].from < y && (y = i[o].from);
        for (let S = 0; S < l.length; S++) l[S].to < y && (y = l[S].to);
        y < p && (a = u.cut(y - s), u = u.cut(0, y - s), p = y, m = -1);
      } else for (; o < i.length && i[o].to < p; ) o++;
      let _ = u.isInline && !u.isLeaf ? l.filter((y) => !y.inline) : l.slice();
      n(u, _, e.forChild(s, u), m), s = p;
    }
  }
  function Ff(r) {
    if (r.nodeName == "UL" || r.nodeName == "OL") {
      let e = r.style.cssText;
      r.style.cssText = e + "; list-style: square !important", window.getComputedStyle(r).listStyle, r.style.cssText = e;
    }
  }
  function Vf(r, e, t, n) {
    for (let i = 0, s = 0; i < r.childCount && s <= n; ) {
      let o = r.child(i++), l = s;
      if (s += o.nodeSize, !o.isText) continue;
      let a = o.text;
      for (; i < r.childCount; ) {
        let c = r.child(i++);
        if (s += c.nodeSize, !c.isText) break;
        a += c.text;
      }
      if (s >= t) {
        if (s >= n && a.slice(n - e.length - l, n - l) == e) return n - e.length;
        let c = l < n ? a.lastIndexOf(e, n - l - 1) : -1;
        if (c >= 0 && c + e.length + l >= t) return l + c;
        if (t == n && a.length >= n + e.length - l && a.slice(n - l, n - l + e.length) == e) return n;
      }
    }
    return -1;
  }
  function Un(r, e, t, n, i) {
    let s = [];
    for (let o = 0, l = 0; o < r.length; o++) {
      let a = r[o], c = l, f = l += a.size;
      c >= t || f <= e ? s.push(a) : (c < e && s.push(a.slice(0, e - c, n)), i && (s.push(i), i = void 0), f > t && s.push(a.slice(t - c, a.size, n)));
    }
    return s;
  }
  function cr(r, e = null) {
    let t = r.domSelectionRange(), n = r.state.doc;
    if (!t.focusNode) return null;
    let i = r.docView.nearestDesc(t.focusNode), s = i && i.size == 0, o = r.docView.posFromDOM(t.focusNode, t.focusOffset, 1);
    if (o < 0) return null;
    let l = n.resolve(o), a, c;
    if (fn(t)) {
      for (a = o; i && !i.node; ) i = i.parent;
      let d = i.node;
      if (i && d.isAtom && O.isSelectable(d) && i.parent && !(d.isInline && uf(t.focusNode, t.focusOffset, i.dom))) {
        let u = i.posBefore;
        c = new O(o == u ? l : n.resolve(u));
      }
    } else {
      if (t instanceof r.dom.ownerDocument.defaultView.Selection && t.rangeCount > 1) {
        let d = o, u = o;
        for (let m = 0; m < t.rangeCount; m++) {
          let p = t.getRangeAt(m);
          d = Math.min(d, r.docView.posFromDOM(p.startContainer, p.startOffset, 1)), u = Math.max(u, r.docView.posFromDOM(p.endContainer, p.endOffset, -1));
        }
        if (d < 0) return null;
        [a, o] = u == r.state.selection.anchor ? [
          u,
          d
        ] : [
          d,
          u
        ], l = n.resolve(o);
      } else a = r.docView.posFromDOM(t.anchorNode, t.anchorOffset, 1);
      if (a < 0) return null;
    }
    let f = n.resolve(a);
    if (!c) {
      let d = e == "pointer" || r.state.selection.head < l.pos && !s ? 1 : -1;
      c = fr(r, f, l, d);
    }
    return c;
  }
  function Ds(r) {
    return r.editable ? r.hasFocus() : As(r) && document.activeElement && document.activeElement.contains(r.dom);
  }
  function Se(r, e = false) {
    let t = r.state.selection;
    if (Is(r, t), !Ds(r)) return;
    let n = r.input.mouseDown;
    if (!e && F && n) {
      let i = r.domSelectionRange(), s = r.domObserver.currentSelection;
      if (i.anchorNode && s.anchorNode && Ye(i.anchorNode, i.anchorOffset, s.anchorNode, s.anchorOffset) && n.delaySelUpdate()) {
        r.domObserver.setCurSelection();
        return;
      }
    }
    if (r.domObserver.disconnectSelection(), r.cursorWrapper) qf(r);
    else {
      let { anchor: i, head: s } = t, o, l;
      Zr && !(t instanceof E) && (t.$from.parent.inlineContent || (o = ei(r, t.from)), !t.empty && !t.$from.parent.inlineContent && (l = ei(r, t.to))), r.docView.setSelection(i, s, r, e), Zr && (o && ti(o), l && ti(l)), t.visible ? r.dom.classList.remove("ProseMirror-hideselection") : (r.dom.classList.add("ProseMirror-hideselection"), "onselectionchange" in document && Lf(r));
    }
    r.domObserver.setCurSelection(), r.domObserver.connectSelection();
  }
  const Zr = K || F && _s < 63;
  function ei(r, e) {
    let { node: t, offset: n } = r.docView.domFromPos(e, 0), i = n < t.childNodes.length ? t.childNodes[n] : null, s = n ? t.childNodes[n - 1] : null;
    if (K && i && i.contentEditable == "false") return On(i);
    if ((!i || i.contentEditable == "false") && (!s || s.contentEditable == "false")) {
      if (i) return On(i);
      if (s) return On(s);
    }
  }
  function On(r) {
    return r.contentEditable = "true", K && r.draggable && (r.draggable = false, r.wasDraggable = true), r;
  }
  function ti(r) {
    r.contentEditable = "false", r.wasDraggable && (r.draggable = true, r.wasDraggable = null);
  }
  function Lf(r) {
    let e = r.dom.ownerDocument;
    e.removeEventListener("selectionchange", r.input.hideSelectionGuard);
    let t = r.domSelectionRange(), n = t.anchorNode, i = t.anchorOffset;
    e.addEventListener("selectionchange", r.input.hideSelectionGuard = () => {
      (t.anchorNode != n || t.anchorOffset != i) && (e.removeEventListener("selectionchange", r.input.hideSelectionGuard), setTimeout(() => {
        (!Ds(r) || r.state.selection.visible) && r.dom.classList.remove("ProseMirror-hideselection");
      }, 20));
    });
  }
  function qf(r) {
    let e = r.domSelection();
    if (!e) return;
    let t = r.cursorWrapper.dom, n = t.nodeName == "IMG";
    n ? e.collapse(t.parentNode, W(t) + 1) : e.collapse(t, 0), !n && !r.state.selection.visible && Z && De <= 11 && (t.disabled = true, t.disabled = false);
  }
  function Is(r, e) {
    if (e instanceof O) {
      let t = r.docView.descAt(e.from);
      t != r.lastSelectedViewDesc && (ni(r), t && t.selectNode(), r.lastSelectedViewDesc = t);
    } else ni(r);
  }
  function ni(r) {
    r.lastSelectedViewDesc && (r.lastSelectedViewDesc.parent && r.lastSelectedViewDesc.deselectNode(), r.lastSelectedViewDesc = void 0);
  }
  function fr(r, e, t, n) {
    return r.someProp("createSelectionBetween", (i) => i(r, e, t)) || E.between(e, t, n);
  }
  function ri(r) {
    return r.editable && !r.hasFocus() ? false : As(r);
  }
  function As(r) {
    let e = r.domSelectionRange();
    if (!e.anchorNode) return false;
    try {
      return r.dom.contains(e.anchorNode.nodeType == 3 ? e.anchorNode.parentNode : e.anchorNode) && (r.editable || r.dom.contains(e.focusNode.nodeType == 3 ? e.focusNode.parentNode : e.focusNode));
    } catch {
      return false;
    }
  }
  function Wf(r) {
    let e = r.docView.domFromPos(r.state.selection.anchor, 0), t = r.domSelectionRange();
    return Ye(e.node, e.offset, t.anchorNode, t.anchorOffset);
  }
  function Gn(r, e) {
    let { $anchor: t, $head: n } = r.selection, i = e > 0 ? t.max(n) : t.min(n), s = i.parent.inlineContent ? i.depth ? r.doc.resolve(e > 0 ? i.after() : i.before()) : null : i;
    return s && I.findFrom(s, e);
  }
  function Oe(r, e) {
    return r.dispatch(r.state.tr.setSelection(e).scrollIntoView()), true;
  }
  function ii(r, e, t) {
    let n = r.state.selection;
    if (n instanceof E) if (t.indexOf("s") > -1) {
      let { $head: i } = n, s = i.textOffset ? null : e < 0 ? i.nodeBefore : i.nodeAfter;
      if (!s || s.isText || !s.isLeaf) return false;
      let o = r.state.doc.resolve(i.pos + s.nodeSize * (e < 0 ? -1 : 1));
      return Oe(r, new E(n.$anchor, o));
    } else if (n.empty) {
      if (r.endOfTextblock(e > 0 ? "forward" : "backward")) {
        let i = Gn(r.state, e);
        return i && i instanceof O ? Oe(r, i) : false;
      } else if (!(re && t.indexOf("m") > -1)) {
        let i = n.$head, s = i.textOffset ? null : e < 0 ? i.nodeBefore : i.nodeAfter, o;
        if (!s || s.isText) return false;
        let l = e < 0 ? i.pos - s.nodeSize : i.pos;
        return s.isAtom || (o = r.docView.descAt(l)) && !o.contentDOM ? O.isSelectable(s) ? Oe(r, new O(e < 0 ? r.state.doc.resolve(i.pos - s.nodeSize) : i)) : vt ? Oe(r, new E(r.state.doc.resolve(e < 0 ? l : l + s.nodeSize))) : false : false;
      }
    } else return false;
    else {
      if (n instanceof O && n.node.isInline) return Oe(r, new E(e > 0 ? n.$to : n.$from));
      {
        let i = Gn(r.state, e);
        return i ? Oe(r, i) : false;
      }
    }
  }
  function en(r) {
    return r.nodeType == 3 ? r.nodeValue.length : r.childNodes.length;
  }
  function wt(r, e) {
    let t = r.pmViewDesc;
    return t && t.size == 0 && (e < 0 || r.nextSibling || r.nodeName != "BR");
  }
  function Ze(r, e) {
    return e < 0 ? Jf(r) : $f(r);
  }
  function Jf(r) {
    let e = r.domSelectionRange(), t = e.focusNode, n = e.focusOffset;
    if (!t) return;
    let i, s, o = false;
    for (se && t.nodeType == 1 && n < en(t) && wt(t.childNodes[n], -1) && (o = true); ; ) if (n > 0) {
      if (t.nodeType != 1) break;
      {
        let l = t.childNodes[n - 1];
        if (wt(l, -1)) i = t, s = --n;
        else if (l.nodeType == 3) t = l, n = t.nodeValue.length;
        else break;
      }
    } else {
      if (vs(t)) break;
      {
        let l = t.previousSibling;
        for (; l && wt(l, -1); ) i = t.parentNode, s = W(l), l = l.previousSibling;
        if (l) t = l, n = en(t);
        else {
          if (t = t.parentNode, t == r.dom) break;
          n = 0;
        }
      }
    }
    o ? Yn(r, t, n) : i && Yn(r, i, s);
  }
  function $f(r) {
    let e = r.domSelectionRange(), t = e.focusNode, n = e.focusOffset;
    if (!t) return;
    let i = en(t), s, o;
    for (; ; ) if (n < i) {
      if (t.nodeType != 1) break;
      let l = t.childNodes[n];
      if (wt(l, 1)) s = t, o = ++n;
      else break;
    } else {
      if (vs(t)) break;
      {
        let l = t.nextSibling;
        for (; l && wt(l, 1); ) s = l.parentNode, o = W(l) + 1, l = l.nextSibling;
        if (l) t = l, n = 0, i = en(t);
        else {
          if (t = t.parentNode, t == r.dom) break;
          n = i = 0;
        }
      }
    }
    s && Yn(r, s, o);
  }
  function vs(r) {
    let e = r.pmViewDesc;
    return e && e.node && e.node.isBlock;
  }
  function jf(r, e) {
    for (; r && e == r.childNodes.length && !At(r); ) e = W(r) + 1, r = r.parentNode;
    for (; r && e < r.childNodes.length; ) {
      let t = r.childNodes[e];
      if (t.nodeType == 3) return t;
      if (t.nodeType == 1 && t.contentEditable == "false") break;
      r = t, e = 0;
    }
  }
  function Kf(r, e) {
    for (; r && !e && !At(r); ) e = W(r), r = r.parentNode;
    for (; r && e; ) {
      let t = r.childNodes[e - 1];
      if (t.nodeType == 3) return t;
      if (t.nodeType == 1 && t.contentEditable == "false") break;
      r = t, e = r.childNodes.length;
    }
  }
  function Yn(r, e, t) {
    if (e.nodeType != 3) {
      let s, o;
      (o = jf(e, t)) ? (e = o, t = 0) : (s = Kf(e, t)) && (e = s, t = s.nodeValue.length);
    }
    let n = r.domSelection();
    if (!n) return;
    if (fn(n)) {
      let s = document.createRange();
      s.setEnd(e, t), s.setStart(e, t), n.removeAllRanges(), n.addRange(s);
    } else n.extend && n.extend(e, t);
    r.domObserver.setCurSelection();
    let { state: i } = r;
    setTimeout(() => {
      r.state == i && Se(r);
    }, 50);
  }
  function si(r, e) {
    let t = r.state.doc.resolve(e);
    if (!(F || ys) && t.parent.inlineContent) {
      let i = r.coordsAtPos(e);
      if (e > t.start()) {
        let s = r.coordsAtPos(e - 1), o = (s.top + s.bottom) / 2;
        if (o > i.top && o < i.bottom && Math.abs(s.left - i.left) > 1) return s.left < i.left ? "ltr" : "rtl";
      }
      if (e < t.end()) {
        let s = r.coordsAtPos(e + 1), o = (s.top + s.bottom) / 2;
        if (o > i.top && o < i.bottom && Math.abs(s.left - i.left) > 1) return s.left > i.left ? "ltr" : "rtl";
      }
    }
    return getComputedStyle(r.dom).direction == "rtl" ? "rtl" : "ltr";
  }
  function oi(r, e, t) {
    let n = r.state.selection;
    if (n instanceof E && !n.empty || t.indexOf("s") > -1 || re && t.indexOf("m") > -1) return false;
    let { $from: i, $to: s } = n;
    if (!i.parent.inlineContent || r.endOfTextblock(e < 0 ? "up" : "down")) {
      let o = Gn(r.state, e);
      if (o && o instanceof O) return Oe(r, o);
    }
    if (!i.parent.inlineContent) {
      let o = e < 0 ? i : s, l = n instanceof ne ? I.near(o, e) : I.findFrom(o, e);
      return l ? Oe(r, l) : false;
    }
    return false;
  }
  function li(r, e) {
    if (!(r.state.selection instanceof E)) return true;
    let { $head: t, $anchor: n, empty: i } = r.state.selection;
    if (!t.sameParent(n)) return true;
    if (!i) return false;
    if (r.endOfTextblock(e > 0 ? "forward" : "backward")) return true;
    let s = !t.textOffset && (e < 0 ? t.nodeBefore : t.nodeAfter);
    if (s && !s.isText) {
      let o = r.state.tr;
      return e < 0 ? o.delete(t.pos - s.nodeSize, t.pos) : o.delete(t.pos, t.pos + s.nodeSize), r.dispatch(o), true;
    }
    return false;
  }
  function ai(r, e, t) {
    r.domObserver.stop(), e.contentEditable = t, r.domObserver.start();
  }
  function Hf(r) {
    if (!K || r.state.selection.$head.parentOffset > 0) return false;
    let { focusNode: e, focusOffset: t } = r.domSelectionRange();
    if (e && e.nodeType == 1 && t == 0 && e.firstChild && e.firstChild.contentEditable == "false") {
      let n = e.firstChild;
      ai(r, n, "true"), setTimeout(() => ai(r, n, "false"), 20);
    }
    return false;
  }
  function Uf(r) {
    let e = "";
    return r.ctrlKey && (e += "c"), r.metaKey && (e += "m"), r.altKey && (e += "a"), r.shiftKey && (e += "s"), e;
  }
  function Gf(r, e) {
    let t = e.keyCode, n = Uf(e);
    if (t == 8 || re && t == 72 && n == "c") return li(r, -1) || Ze(r, -1);
    if (t == 46 && !e.shiftKey || re && t == 68 && n == "c") return li(r, 1) || Ze(r, 1);
    if (t == 13 || t == 27) return true;
    if (t == 37 || re && t == 66 && n == "c") {
      let i = t == 37 ? si(r, r.state.selection.from) == "ltr" ? -1 : 1 : -1;
      return ii(r, i, n) || Ze(r, i);
    } else if (t == 39 || re && t == 70 && n == "c") {
      let i = t == 39 ? si(r, r.state.selection.from) == "ltr" ? 1 : -1 : 1;
      return ii(r, i, n) || Ze(r, i);
    } else {
      if (t == 38 || re && t == 80 && n == "c") return oi(r, -1, n) || Ze(r, -1);
      if (t == 40 || re && t == 78 && n == "c") return Hf(r) || oi(r, 1, n) || Ze(r, 1);
      if (n == (re ? "m" : "c") && (t == 66 || t == 73 || t == 89 || t == 90)) return true;
    }
    return false;
  }
  function dr(r, e) {
    r.someProp("transformCopied", (m) => {
      e = m(e, r);
    });
    let t = [], { content: n, openStart: i, openEnd: s } = e;
    for (; i > 1 && s > 1 && n.childCount == 1 && n.firstChild.childCount == 1; ) {
      i--, s--;
      let m = n.firstChild;
      t.push(m.type.name, m.attrs != m.type.defaultAttrs ? m.attrs : null), n = m.content;
    }
    let o = r.someProp("clipboardSerializer") || at.fromSchema(r.state.schema), l = Vs(), a = l.createElement("div");
    a.appendChild(o.serializeFragment(n, {
      document: l
    }));
    let c = a.firstChild, f, d = 0;
    for (; c && c.nodeType == 1 && (f = Fs[c.nodeName.toLowerCase()]); ) {
      for (let m = f.length - 1; m >= 0; m--) {
        let p = l.createElement(f[m]);
        for (; a.firstChild; ) p.appendChild(a.firstChild);
        a.appendChild(p), d++;
      }
      c = a.firstChild;
    }
    c && c.nodeType == 1 && c.setAttribute("data-pm-slice", `${i} ${s}${d ? ` -${d}` : ""} ${JSON.stringify(t)}`);
    let u = r.someProp("clipboardTextSerializer", (m) => m(e, r)) || e.content.textBetween(0, e.content.size, `

`);
    return {
      dom: a,
      text: u,
      slice: e
    };
  }
  function Rs(r, e, t, n, i) {
    let s = i.parent.type.spec.code, o, l;
    if (!t && !e) return null;
    let a = !!e && (n || s || !t);
    if (a) {
      if (r.someProp("transformPastedText", (u) => {
        e = u(e, s || n, r);
      }), s) return l = new w(b.from(r.state.schema.text(e.replace(/\r\n?/g, `
`))), 0, 0), r.someProp("transformPasted", (u) => {
        l = u(l, r, true);
      }), l;
      let d = r.someProp("clipboardTextParser", (u) => u(e, i, n, r));
      if (d) l = d;
      else {
        let u = i.marks(), { schema: m } = r.state, p = at.fromSchema(m);
        o = document.createElement("div"), e.split(/(?:\r\n?|\n)+/).forEach((_) => {
          let y = o.appendChild(document.createElement("p"));
          _ && y.appendChild(p.serializeNode(m.text(_, u)));
        });
      }
    } else r.someProp("transformPastedHTML", (d) => {
      t = d(t, r);
    }), o = Zf(t), vt && ed(o);
    let c = o && o.querySelector("[data-pm-slice]"), f = c && /^(\d+) (\d+)(?: -(\d+))? (.*)/.exec(c.getAttribute("data-pm-slice") || "");
    if (f && f[3]) for (let d = +f[3]; d > 0; d--) {
      let u = o.firstChild;
      for (; u && u.nodeType != 1; ) u = u.nextSibling;
      if (!u) break;
      o = u;
    }
    if (l || (l = (r.someProp("clipboardParser") || r.someProp("domParser") || Ct.fromSchema(r.state.schema)).parseSlice(o, {
      preserveWhitespace: !!(a || f),
      context: i,
      ruleFromNode(u) {
        return u.nodeName == "BR" && !u.nextSibling && u.parentNode && !Yf.test(u.parentNode.nodeName) ? {
          ignore: true
        } : null;
      }
    })), f) l = td(ci(l, +f[1], +f[2]), f[4]);
    else if (l = w.maxOpen(Qf(l.content, i), true), l.openStart || l.openEnd) {
      let d = 0, u = 0;
      for (let m = l.content.firstChild; d < l.openStart && !m.type.spec.isolating; d++, m = m.firstChild) ;
      for (let m = l.content.lastChild; u < l.openEnd && !m.type.spec.isolating; u++, m = m.lastChild) ;
      l = ci(l, d, u);
    }
    return r.someProp("transformPasted", (d) => {
      l = d(l, r, a);
    }), l;
  }
  const Yf = /^(a|abbr|acronym|b|cite|code|del|em|i|ins|kbd|label|output|q|ruby|s|samp|span|strong|sub|sup|time|u|tt|var)$/i;
  function Qf(r, e) {
    if (r.childCount < 2) return r;
    for (let t = e.depth; t >= 0; t--) {
      let i = e.node(t).contentMatchAt(e.index(t)), s, o = [];
      if (r.forEach((l) => {
        if (!o) return;
        let a = i.findWrapping(l.type), c;
        if (!a) return o = null;
        if (c = o.length && s.length && Bs(a, s, l, o[o.length - 1], 0)) o[o.length - 1] = c;
        else {
          o.length && (o[o.length - 1] = zs(o[o.length - 1], s.length));
          let f = Ps(l, a);
          o.push(f), i = i.matchType(f.type), s = a;
        }
      }), o) return b.from(o);
    }
    return r;
  }
  function Ps(r, e, t = 0) {
    for (let n = e.length - 1; n >= t; n--) r = e[n].create(null, b.from(r));
    return r;
  }
  function Bs(r, e, t, n, i) {
    if (i < r.length && i < e.length && r[i] == e[i]) {
      let s = Bs(r, e, t, n.lastChild, i + 1);
      if (s) return n.copy(n.content.replaceChild(n.childCount - 1, s));
      if (n.contentMatchAt(n.childCount).matchType(i == r.length - 1 ? t.type : r[i + 1])) return n.copy(n.content.append(b.from(Ps(t, r, i + 1))));
    }
  }
  function zs(r, e) {
    if (e == 0) return r;
    let t = r.content.replaceChild(r.childCount - 1, zs(r.lastChild, e - 1)), n = r.contentMatchAt(r.childCount).fillBefore(b.empty, true);
    return r.copy(t.append(n));
  }
  function Qn(r, e, t, n, i, s) {
    let o = e < 0 ? r.firstChild : r.lastChild, l = o.content;
    return r.childCount > 1 && (s = 0), i < n - 1 && (l = Qn(l, e, t, n, i + 1, s)), i >= t && (l = e < 0 ? o.contentMatchAt(0).fillBefore(l, s <= i).append(l) : l.append(o.contentMatchAt(o.childCount).fillBefore(b.empty, true))), r.replaceChild(e < 0 ? 0 : r.childCount - 1, o.copy(l));
  }
  function ci(r, e, t) {
    return e < r.openStart && (r = new w(Qn(r.content, -1, e, r.openStart, 0, r.openEnd), e, r.openEnd)), t < r.openEnd && (r = new w(Qn(r.content, 1, t, r.openEnd, 0, 0), r.openStart, t)), r;
  }
  const Fs = {
    thead: [
      "table"
    ],
    tbody: [
      "table"
    ],
    tfoot: [
      "table"
    ],
    caption: [
      "table"
    ],
    colgroup: [
      "table"
    ],
    col: [
      "table",
      "colgroup"
    ],
    tr: [
      "table",
      "tbody"
    ],
    td: [
      "table",
      "tbody",
      "tr"
    ],
    th: [
      "table",
      "tbody",
      "tr"
    ]
  };
  function Vs() {
    return document.implementation.createHTMLDocument("title");
  }
  let Mn = null;
  function Xf(r) {
    let e = window.trustedTypes;
    return e ? (Mn || (Mn = e.defaultPolicy || e.createPolicy("ProseMirrorClipboard", {
      createHTML: (t) => t
    })), Mn.createHTML(r)) : r;
  }
  function Zf(r) {
    let e = /^(\s*<meta [^>]*>)*/.exec(r);
    e && (r = r.slice(e[0].length));
    let t = Vs(), n = t.body, i = /<([a-z][^>\s]+)/i.exec(r), s;
    if ((s = i && Fs[i[1].toLowerCase()]) && (r = s.map((o) => "<" + o + ">").join("") + r + s.map((o) => "</" + o + ">").reverse().join("")), n.innerHTML = Xf(r), s) for (let o = 0; o < s.length; o++) n = n.querySelector(s[o]) || n;
    for (let o = 0; o < t.styleSheets.length; o++) {
      let l = t.styleSheets[o];
      for (let a = 0; a < l.rules.length; a++) {
        let c = l.rules[a];
        if (c instanceof CSSStyleRule) {
          let f = n.querySelectorAll(c.selectorText);
          for (let d = 0; d < f.length; d++) f[d].style.cssText += c.style.cssText;
        }
      }
    }
    return n;
  }
  function ed(r) {
    let e = r.querySelectorAll(F ? "span:not([class]):not([style])" : "span.Apple-converted-space");
    for (let t = 0; t < e.length; t++) {
      let n = e[t];
      n.childNodes.length == 1 && n.textContent == "\xA0" && n.parentNode && n.parentNode.replaceChild(r.ownerDocument.createTextNode(" "), n);
    }
  }
  function td(r, e) {
    if (!r.size) return r;
    let t = r.content.firstChild.type.schema, n;
    try {
      n = JSON.parse(e);
    } catch {
      return r;
    }
    let { content: i, openStart: s, openEnd: o } = r;
    for (let l = n.length - 2; l >= 0; l -= 2) {
      let a = t.nodes[n[l]];
      if (!a || a.hasRequiredAttrs()) break;
      i = b.from(a.create(n[l + 1], i)), s++, o++;
    }
    return new w(i, s, o);
  }
  const G = {}, Y = {}, nd = {
    touchstart: true,
    touchmove: true
  };
  class rd {
    constructor() {
      this.shiftKey = false, this.mouseDown = null, this.lastKeyCode = null, this.lastKeyCodeTime = 0, this.lastClick = {
        time: 0,
        x: 0,
        y: 0,
        type: "",
        button: 0
      }, this.lastSelectionOrigin = null, this.lastSelectionTime = 0, this.lastIOSEnter = 0, this.lastIOSEnterFallbackTimeout = -1, this.lastFocus = 0, this.lastTouch = 0, this.lastChromeDelete = 0, this.composing = false, this.compositionNode = null, this.composingTimeout = -1, this.compositionNodes = [], this.compositionEndedAt = -2e8, this.compositionID = 1, this.badSafariComposition = false, this.compositionPendingChanges = 0, this.domChangeCount = 0, this.eventHandlers = /* @__PURE__ */ Object.create(null), this.hideSelectionGuard = null;
    }
  }
  function id(r) {
    for (let e in G) {
      let t = G[e];
      r.dom.addEventListener(e, r.input.eventHandlers[e] = (n) => {
        od(r, n) && !ur(r, n) && (r.editable || !(n.type in Y)) && t(r, n);
      }, nd[e] ? {
        passive: true
      } : void 0);
    }
    K && r.dom.addEventListener("input", () => null), Xn(r);
  }
  function xe(r, e) {
    r.input.lastSelectionOrigin = e, r.input.lastSelectionTime = Date.now();
  }
  function sd(r) {
    r.input.mouseDown && r.input.mouseDown.done(), r.domObserver.stop();
    for (let e in r.input.eventHandlers) r.dom.removeEventListener(e, r.input.eventHandlers[e]);
    clearTimeout(r.input.composingTimeout), clearTimeout(r.input.lastIOSEnterFallbackTimeout);
  }
  function Xn(r) {
    r.someProp("handleDOMEvents", (e) => {
      for (let t in e) r.input.eventHandlers[t] || r.dom.addEventListener(t, r.input.eventHandlers[t] = (n) => ur(r, n));
    });
  }
  function ur(r, e) {
    return r.someProp("handleDOMEvents", (t) => {
      let n = t[e.type];
      return n ? n(r, e) || e.defaultPrevented : false;
    });
  }
  function od(r, e) {
    if (!e.bubbles) return true;
    if (e.defaultPrevented) return false;
    for (let t = e.target; t != r.dom; t = t.parentNode) if (!t || t.nodeType == 11 || t.pmViewDesc && t.pmViewDesc.stopEvent(e)) return false;
    return true;
  }
  function ld(r, e) {
    !ur(r, e) && G[e.type] && (r.editable || !(e.type in Y)) && G[e.type](r, e);
  }
  Y.keydown = (r, e) => {
    let t = e;
    if (r.input.shiftKey = t.keyCode == 16 || t.shiftKey, !Js(r) && (r.input.lastKeyCode = t.keyCode, r.input.lastKeyCodeTime = Date.now(), !(ke && F && t.keyCode == 13))) if (t.keyCode != 229 && r.domObserver.forceFlush(), ot && t.keyCode == 13 && !t.ctrlKey && !t.altKey && !t.metaKey) {
      let n = Date.now();
      r.input.lastIOSEnter = n, r.input.lastIOSEnterFallbackTimeout = setTimeout(() => {
        r.input.lastIOSEnter == n && (r.someProp("handleKeyDown", (i) => i(r, Fe(13, "Enter"))), r.input.lastIOSEnter = 0);
      }, 200);
    } else r.someProp("handleKeyDown", (n) => n(r, t)) || Gf(r, t) ? t.preventDefault() : xe(r, "key");
  };
  Y.keyup = (r, e) => {
    e.keyCode == 16 && (r.input.shiftKey = false);
  };
  Y.keypress = (r, e) => {
    let t = e;
    if (Js(r) || !t.charCode || t.ctrlKey && !t.altKey || re && t.metaKey) return;
    if (r.someProp("handleKeyPress", (i) => i(r, t))) {
      t.preventDefault();
      return;
    }
    let n = r.state.selection;
    if (!(n instanceof E) || !n.$from.sameParent(n.$to)) {
      let i = String.fromCharCode(t.charCode), s = () => r.state.tr.insertText(i).scrollIntoView();
      !/[\r\n]/.test(i) && !r.someProp("handleTextInput", (o) => o(r, n.$from.pos, n.$to.pos, i, s)) && r.dispatch(s()), t.preventDefault();
    }
  };
  function Pt(r) {
    return {
      left: r.clientX,
      top: r.clientY
    };
  }
  function ad(r, e) {
    let t = e.x - r.clientX, n = e.y - r.clientY;
    return t * t + n * n < 100;
  }
  function hr(r, e, t, n, i) {
    if (n == -1) return false;
    let s = r.state.doc.resolve(n);
    for (let o = s.depth + 1; o > 0; o--) if (r.someProp(e, (l) => o > s.depth ? l(r, t, s.nodeAfter, s.before(o), i, true) : l(r, t, s.node(o), s.before(o), i, false))) return true;
    return false;
  }
  function Bt(r, e, t) {
    if (r.focused || r.focus(), r.state.selection.eq(e)) return;
    let n = r.state.tr.setSelection(e);
    n.setMeta("pointer", true), r.dispatch(n);
  }
  function cd(r, e) {
    if (e == -1) return false;
    let t = r.state.doc.resolve(e), n = t.nodeAfter;
    return n && n.isAtom && O.isSelectable(n) ? (Bt(r, new O(t)), true) : false;
  }
  function fd(r, e) {
    if (e == -1) return false;
    let t = r.state.selection, n, i;
    t instanceof O && (n = t.node);
    let s = r.state.doc.resolve(e);
    for (let o = s.depth + 1; o > 0; o--) {
      let l = o > s.depth ? s.nodeAfter : s.node(o);
      if (O.isSelectable(l)) {
        n && t.$from.depth > 0 && o >= t.$from.depth && s.before(t.$from.depth + 1) == t.$from.pos ? i = s.before(t.$from.depth) : i = s.before(o);
        break;
      }
    }
    return i != null ? (Bt(r, O.create(r.state.doc, i)), true) : false;
  }
  function dd(r, e, t, n, i) {
    return hr(r, "handleClickOn", e, t, n) || r.someProp("handleClick", (s) => s(r, e, n)) || (i ? fd(r, t) : cd(r, t));
  }
  function ud(r, e, t, n) {
    return hr(r, "handleDoubleClickOn", e, t, n) || r.someProp("handleDoubleClick", (i) => i(r, e, n));
  }
  function hd(r, e, t, n) {
    return hr(r, "handleTripleClickOn", e, t, n) || r.someProp("handleTripleClick", (i) => i(r, e, n)) || pd(r, t, n);
  }
  function pd(r, e, t) {
    if (t.button != 0) return false;
    let n = Ls(r, e, true), i = r.state.doc;
    return n ? (Bt(r, n), n instanceof E && i.eq(r.state.doc) && (r.input.mouseDown = new gd(r, n)), true) : false;
  }
  function Ls(r, e, t) {
    let n = r.state.doc;
    if (e == -1) return n.inlineContent ? E.create(n, 0, n.content.size) : null;
    let i = n.resolve(e);
    for (let s = i.depth + 1; s > 0; s--) {
      let o = s > i.depth ? i.nodeAfter : i.node(s), l = i.before(s);
      if (o.inlineContent) return E.create(n, l + 1, l + 1 + o.content.size);
      if (t && O.isSelectable(o)) return O.create(n, l);
    }
    return null;
  }
  function pr(r) {
    return tn(r);
  }
  const qs = re ? "metaKey" : "ctrlKey";
  G.mousedown = (r, e) => {
    let t = e;
    r.input.shiftKey = t.shiftKey;
    let n = pr(r), i = Date.now(), s = "singleClick";
    i - r.input.lastClick.time < 500 && ad(t, r.input.lastClick) && !t[qs] && r.input.lastClick.button == t.button && (r.input.lastClick.type == "singleClick" ? s = "doubleClick" : r.input.lastClick.type == "doubleClick" && (s = "tripleClick")), r.input.lastClick = {
      time: i,
      x: t.clientX,
      y: t.clientY,
      type: s,
      button: t.button
    }, r.input.mouseDown && r.input.mouseDown.done();
    let o = r.posAtCoords(Pt(t));
    o && (s == "singleClick" ? r.input.mouseDown = new md(r, o, t, !!n) : (s == "doubleClick" ? ud : hd)(r, o.pos, o.inside, t) ? t.preventDefault() : xe(r, "pointer"));
  };
  class Ws {
    constructor(e) {
      this.view = e, this.mightDrag = null, e.root.addEventListener("mouseup", this.up = this.up.bind(this)), e.root.addEventListener("mousemove", this.move = this.move.bind(this));
    }
    up(e) {
      this.done();
    }
    move(e) {
      e.buttons == 0 && this.done();
    }
    done() {
      this.view.root.removeEventListener("mouseup", this.up), this.view.root.removeEventListener("mousemove", this.move), this.view.input.mouseDown == this && (this.view.input.mouseDown = null);
    }
    delaySelUpdate() {
      return false;
    }
  }
  class md extends Ws {
    constructor(e, t, n, i) {
      super(e), this.pos = t, this.event = n, this.flushed = i, this.delayedSelectionSync = false, this.startDoc = e.state.doc, this.selectNode = !!n[qs], this.allowDefault = n.shiftKey;
      let s, o;
      if (t.inside > -1) s = e.state.doc.nodeAt(t.inside), o = t.inside;
      else {
        let f = e.state.doc.resolve(t.pos);
        s = f.parent, o = f.depth ? f.before() : 0;
      }
      const l = i ? null : n.target, a = l ? e.docView.nearestDesc(l, true) : null;
      this.target = a && a.nodeDOM.nodeType == 1 ? a.nodeDOM : null;
      let { selection: c } = e.state;
      n.button == 0 && (s.type.spec.draggable && s.type.spec.selectable !== false || c instanceof O && c.from <= o && c.to > o) && (this.mightDrag = {
        node: s,
        pos: o,
        addAttr: !!(this.target && !this.target.draggable),
        setUneditable: !!(this.target && se && !this.target.hasAttribute("contentEditable"))
      }), this.target && this.mightDrag && (this.mightDrag.addAttr || this.mightDrag.setUneditable) && (this.view.domObserver.stop(), this.mightDrag.addAttr && (this.target.draggable = true), this.mightDrag.setUneditable && setTimeout(() => {
        this.view.input.mouseDown == this && this.target.setAttribute("contentEditable", "false");
      }, 20), this.view.domObserver.start()), xe(e, "pointer");
    }
    done() {
      super.done(), this.mightDrag && this.target && (this.view.domObserver.stop(), this.mightDrag.addAttr && this.target.removeAttribute("draggable"), this.mightDrag.setUneditable && this.target.removeAttribute("contentEditable"), this.view.domObserver.start()), this.delayedSelectionSync && setTimeout(() => {
        this.view.isDestroyed || Se(this.view);
      });
    }
    up(e) {
      if (this.done(), !this.view.dom.contains(e.target)) return;
      let t = this.pos;
      this.view.state.doc != this.startDoc && (t = this.view.posAtCoords(Pt(e))), this.updateAllowDefault(e), this.allowDefault || !t ? xe(this.view, "pointer") : dd(this.view, t.pos, t.inside, e, this.selectNode) ? e.preventDefault() : e.button == 0 && (this.flushed || K && this.mightDrag && !this.mightDrag.node.isAtom || F && !this.view.state.selection.visible && Math.min(Math.abs(t.pos - this.view.state.selection.from), Math.abs(t.pos - this.view.state.selection.to)) <= 2) ? (Bt(this.view, I.near(this.view.state.doc.resolve(t.pos))), e.preventDefault()) : xe(this.view, "pointer");
    }
    move(e) {
      this.updateAllowDefault(e), xe(this.view, "pointer"), super.move(e);
    }
    updateAllowDefault(e) {
      !this.allowDefault && (Math.abs(this.event.x - e.clientX) > 4 || Math.abs(this.event.y - e.clientY) > 4) && (this.allowDefault = true);
    }
    delaySelUpdate() {
      return this.allowDefault ? (this.delayedSelectionSync = true, true) : false;
    }
  }
  class gd extends Ws {
    constructor(e, t) {
      super(e), this.startSelection = t, this.startDoc = e.state.doc;
    }
    move(e) {
      if (e.buttons == 0 || this.view.isDestroyed || !this.view.state.doc.eq(this.startDoc)) {
        this.done();
        return;
      }
      e.preventDefault(), xe(this.view, "pointer");
      let t = this.view.posAtCoords(Pt(e)), n = t && Ls(this.view, t.inside, false);
      if (!n) return;
      let { doc: i } = this.view.state, s = this.startSelection, [o, l] = n.from < s.from ? [
        s.to,
        n.from
      ] : [
        s.from,
        n.to
      ];
      Bt(this.view, E.create(i, o, l));
    }
  }
  G.touchstart = (r) => {
    r.input.lastTouch = Date.now(), pr(r), xe(r, "pointer");
  };
  G.touchmove = (r) => {
    r.input.lastTouch = Date.now(), xe(r, "pointer");
  };
  G.contextmenu = (r) => pr(r);
  function Js(r, e) {
    return r.composing ? true : K && Math.abs(Date.now() - r.input.compositionEndedAt) < 500 ? (r.input.compositionEndedAt = -2e8, true) : false;
  }
  const _d = ke ? 5e3 : -1;
  Y.compositionstart = Y.compositionupdate = (r) => {
    if (!r.composing) {
      r.domObserver.flush();
      let { state: e } = r, t = e.selection.$to;
      if (e.selection instanceof E && (e.storedMarks || !t.textOffset && t.parentOffset && t.nodeBefore.marks.some((n) => n.type.spec.inclusive === false) || F && ys && yd(r))) r.markCursor = r.state.storedMarks || t.marks(), tn(r, true), r.markCursor = null;
      else if (tn(r, !e.selection.empty), se && e.selection.empty && t.parentOffset && !t.textOffset && t.nodeBefore.marks.length) {
        let n = r.domSelectionRange();
        for (let i = n.focusNode, s = n.focusOffset; i && i.nodeType == 1 && s != 0; ) {
          let o = s < 0 ? i.lastChild : i.childNodes[s - 1];
          if (!o) break;
          if (o.nodeType == 3) {
            let l = r.domSelection();
            l && l.collapse(o, o.nodeValue.length);
            break;
          } else i = o, s = -1;
        }
      }
      r.input.composing = true;
    }
    $s(r, _d);
  };
  function yd(r) {
    let { focusNode: e, focusOffset: t } = r.domSelectionRange();
    if (!e || e.nodeType != 1 || t >= e.childNodes.length) return false;
    let n = e.childNodes[t];
    return n.nodeType == 1 && n.contentEditable == "false";
  }
  Y.compositionend = (r, e) => {
    r.composing && (r.input.composing = false, r.input.compositionEndedAt = Date.now(), r.input.compositionPendingChanges = r.domObserver.pendingRecords().length ? r.input.compositionID : 0, r.input.compositionNode = null, r.input.badSafariComposition ? r.domObserver.forceFlush() : r.input.compositionPendingChanges && Promise.resolve().then(() => r.domObserver.flush()), r.input.compositionID++, $s(r, 20));
  };
  function $s(r, e) {
    clearTimeout(r.input.composingTimeout), e > -1 && (r.input.composingTimeout = setTimeout(() => tn(r), e));
  }
  function js(r) {
    for (r.composing && (r.input.composing = false, r.input.compositionEndedAt = Date.now()); r.input.compositionNodes.length > 0; ) r.input.compositionNodes.pop().markParentsDirty();
  }
  function bd(r) {
    let e = r.domSelectionRange();
    if (!e.focusNode) return null;
    let t = ff(e.focusNode, e.focusOffset), n = df(e.focusNode, e.focusOffset);
    if (t && n && t != n) {
      let i = n.pmViewDesc, s = r.domObserver.lastChangedTextNode;
      if (t == s || n == s) return s;
      if (!i || !i.isText(n.nodeValue)) return n;
      if (r.input.compositionNode == n) {
        let o = t.pmViewDesc;
        if (!(!o || !o.isText(t.nodeValue))) return n;
      }
    }
    return t || n;
  }
  function tn(r, e = false) {
    if (!(ke && r.domObserver.flushingSoon >= 0)) {
      if (r.domObserver.forceFlush(), js(r), e || r.docView && r.docView.dirty) {
        let t = cr(r), n = r.state.selection;
        return t && !t.eq(n) ? r.dispatch(r.state.tr.setSelection(t)) : (r.markCursor || e) && !n.$from.node(n.$from.sharedDepth(n.to)).inlineContent ? r.dispatch(r.state.tr.deleteSelection()) : r.updateState(r.state), true;
      }
      return false;
    }
  }
  function wd(r, e) {
    if (!r.dom.parentNode) return;
    let t = r.dom.parentNode.appendChild(document.createElement("div"));
    t.appendChild(e), t.style.cssText = "position: fixed; left: -10000px; top: 10px";
    let n = getSelection(), i = document.createRange();
    i.selectNodeContents(e), r.dom.blur(), n.removeAllRanges(), n.addRange(i), setTimeout(() => {
      t.parentNode && t.parentNode.removeChild(t), r.focus();
    }, 50);
  }
  const Et = Z && De < 15 || ot && mf < 604;
  G.copy = Y.cut = (r, e) => {
    let t = e, n = r.state.selection, i = t.type == "cut";
    if (n.empty) return;
    let s = Et ? null : t.clipboardData, o = n.content(), { dom: l, text: a } = dr(r, o);
    s ? (t.preventDefault(), s.clearData(), s.setData("text/html", l.innerHTML), s.setData("text/plain", a)) : wd(r, l), i && r.dispatch(r.state.tr.deleteSelection().scrollIntoView().setMeta("uiEvent", "cut"));
  };
  function kd(r) {
    return r.openStart == 0 && r.openEnd == 0 && r.content.childCount == 1 ? r.content.firstChild : null;
  }
  function xd(r, e) {
    if (!r.dom.parentNode) return;
    let t = r.input.shiftKey || r.state.selection.$from.parent.type.spec.code, n = r.dom.parentNode.appendChild(document.createElement(t ? "textarea" : "div"));
    t || (n.contentEditable = "true"), n.style.cssText = "position: fixed; left: -10000px; top: 10px", n.focus();
    let i = r.input.shiftKey && r.input.lastKeyCode != 45;
    setTimeout(() => {
      r.focus(), n.parentNode && n.parentNode.removeChild(n), t ? Tt(r, n.value, null, i, e) : Tt(r, n.textContent, n.innerHTML, i, e);
    }, 50);
  }
  function Tt(r, e, t, n, i) {
    let s = Rs(r, e, t, n, r.state.selection.$from);
    if (r.someProp("handlePaste", (a) => a(r, i, s || w.empty))) return true;
    if (!s) return false;
    let o = kd(s), l = o ? r.state.tr.replaceSelectionWith(o, n) : r.state.tr.replaceSelection(s);
    return r.dispatch(l.scrollIntoView().setMeta("paste", true).setMeta("uiEvent", "paste")), true;
  }
  function Ks(r) {
    let e = r.getData("text/plain") || r.getData("Text");
    if (e) return e;
    let t = r.getData("text/uri-list");
    return t ? t.replace(/\r?\n/g, " ") : "";
  }
  Y.paste = (r, e) => {
    let t = e;
    if (r.composing && !ke) return;
    let n = Et ? null : t.clipboardData, i = r.input.shiftKey && r.input.lastKeyCode != 45;
    n && Tt(r, Ks(n), n.getData("text/html"), i, t) ? t.preventDefault() : xd(r, t);
  };
  class Hs {
    constructor(e, t, n) {
      this.slice = e, this.move = t, this.node = n;
    }
  }
  const Sd = re ? "altKey" : "ctrlKey";
  function Us(r, e) {
    let t;
    return r.someProp("dragCopies", (n) => {
      t = t || n(e);
    }), t != null ? !t : !e[Sd];
  }
  G.dragstart = (r, e) => {
    let t = e, n = r.input.mouseDown;
    if (n && n.done(), !t.dataTransfer) return;
    let i = r.state.selection, s = i.empty ? null : r.posAtCoords(Pt(t)), o;
    if (!(s && s.pos >= i.from && s.pos <= (i instanceof O ? i.to - 1 : i.to))) {
      if (n && n.mightDrag) o = O.create(r.state.doc, n.mightDrag.pos);
      else if (t.target && t.target.nodeType == 1) {
        let d = r.docView.nearestDesc(t.target, true);
        d && d.node.type.spec.draggable && d != r.docView && (o = O.create(r.state.doc, d.posBefore));
      }
    }
    let l = (o || r.state.selection).content(), { dom: a, text: c, slice: f } = dr(r, l);
    (!t.dataTransfer.files.length || !F || _s > 120) && t.dataTransfer.clearData(), t.dataTransfer.setData(Et ? "Text" : "text/html", a.innerHTML), t.dataTransfer.effectAllowed = "copyMove", Et || t.dataTransfer.setData("text/plain", c), r.dragging = new Hs(f, Us(r, t), o);
  };
  G.dragend = (r) => {
    let e = r.dragging;
    window.setTimeout(() => {
      r.dragging == e && (r.dragging = null);
    }, 50);
  };
  Y.dragover = Y.dragenter = (r, e) => e.preventDefault();
  Y.drop = (r, e) => {
    try {
      Cd(r, e, r.dragging);
    } finally {
      r.dragging = null;
    }
  };
  function Cd(r, e, t) {
    if (!e.dataTransfer) return;
    let n = r.posAtCoords(Pt(e));
    if (!n) return;
    let i = r.state.doc.resolve(n.pos), s = t && t.slice;
    s ? r.someProp("transformPasted", (m) => {
      s = m(s, r, false);
    }) : s = Rs(r, Ks(e.dataTransfer), Et ? null : e.dataTransfer.getData("text/html"), false, i);
    let o = !!(t && Us(r, e));
    if (r.someProp("handleDrop", (m) => m(r, e, s || w.empty, o))) {
      e.preventDefault();
      return;
    }
    if (!s) return;
    e.preventDefault();
    let l = s ? Tc(r.state.doc, i.pos, s) : i.pos;
    l == null && (l = i.pos);
    let a = r.state.tr;
    if (o) {
      let { node: m } = t;
      m ? m.replace(a) : a.deleteSelection();
    }
    let c = a.mapping.map(l), f = s.openStart == 0 && s.openEnd == 0 && s.content.childCount == 1, d = a.doc;
    if (f ? a.replaceRangeWith(c, c, s.content.firstChild) : a.replaceRange(c, c, s), a.doc.eq(d)) return;
    let u = a.doc.resolve(c);
    if (f && O.isSelectable(s.content.firstChild) && u.nodeAfter && u.nodeAfter.sameMarkup(s.content.firstChild)) a.setSelection(new O(u));
    else {
      let m = a.mapping.map(l);
      a.mapping.maps[a.mapping.maps.length - 1].forEach((p, _, y, S) => m = S), a.setSelection(fr(r, u, a.doc.resolve(m)));
    }
    r.focus(), r.dispatch(a.setMeta("uiEvent", "drop"));
  }
  G.focus = (r) => {
    r.input.lastFocus = Date.now(), r.focused || (r.domObserver.stop(), r.dom.classList.add("ProseMirror-focused"), r.domObserver.start(), r.focused = true, setTimeout(() => {
      r.docView && r.hasFocus() && !r.domObserver.currentSelection.eq(r.domSelectionRange()) && Se(r);
    }, 20));
  };
  G.blur = (r, e) => {
    let t = e;
    r.focused && (r.domObserver.stop(), r.dom.classList.remove("ProseMirror-focused"), r.domObserver.start(), t.relatedTarget && r.dom.contains(t.relatedTarget) && r.domObserver.currentSelection.clear(), r.focused = false);
  };
  G.beforeinput = (r, e) => {
    if (F && ke && e.inputType == "deleteContentBackward") {
      r.domObserver.flushSoon();
      let { domChangeCount: n } = r.input;
      setTimeout(() => {
        if (r.input.domChangeCount != n || (r.dom.blur(), r.focus(), r.someProp("handleKeyDown", (s) => s(r, Fe(8, "Backspace"))))) return;
        let { $cursor: i } = r.state.selection;
        i && i.pos > 0 && r.dispatch(r.state.tr.delete(i.pos - 1, i.pos).scrollIntoView());
      }, 50);
    }
  };
  for (let r in Y) G[r] = Y[r];
  function Dt(r, e) {
    if (r == e) return true;
    for (let t in r) if (r[t] !== e[t]) return false;
    for (let t in e) if (!(t in r)) return false;
    return true;
  }
  class nn {
    constructor(e, t) {
      this.toDOM = e, this.spec = t || je, this.side = this.spec.side || 0;
    }
    map(e, t, n, i) {
      let { pos: s, deleted: o } = e.mapResult(t.from + i, this.side < 0 ? -1 : 1);
      return o ? null : new fe(s - n, s - n, this);
    }
    valid() {
      return true;
    }
    eq(e) {
      return this == e || e instanceof nn && (this.spec.key && this.spec.key == e.spec.key || this.toDOM == e.toDOM && Dt(this.spec, e.spec));
    }
    destroy(e) {
      this.spec.destroy && this.spec.destroy(e);
    }
  }
  class ve {
    constructor(e, t) {
      this.attrs = e, this.spec = t || je;
    }
    map(e, t, n, i) {
      let s = e.map(t.from + i, this.spec.inclusiveStart ? -1 : 1) - n, o = e.map(t.to + i, this.spec.inclusiveEnd ? 1 : -1) - n;
      return s >= o ? null : new fe(s, o, this);
    }
    valid(e, t) {
      return t.from < t.to;
    }
    eq(e) {
      return this == e || e instanceof ve && Dt(this.attrs, e.attrs) && Dt(this.spec, e.spec);
    }
    static is(e) {
      return e.type instanceof ve;
    }
    destroy() {
    }
  }
  class mr {
    constructor(e, t) {
      this.attrs = e, this.spec = t || je;
    }
    map(e, t, n, i) {
      let s = e.mapResult(t.from + i, 1);
      if (s.deleted) return null;
      let o = e.mapResult(t.to + i, -1);
      return o.deleted || o.pos <= s.pos ? null : new fe(s.pos - n, o.pos - n, this);
    }
    valid(e, t) {
      let { index: n, offset: i } = e.content.findIndex(t.from), s;
      return i == t.from && !(s = e.child(n)).isText && i + s.nodeSize == t.to;
    }
    eq(e) {
      return this == e || e instanceof mr && Dt(this.attrs, e.attrs) && Dt(this.spec, e.spec);
    }
    destroy() {
    }
  }
  class fe {
    constructor(e, t, n) {
      this.from = e, this.to = t, this.type = n;
    }
    copy(e, t) {
      return new fe(e, t, this.type);
    }
    eq(e, t = 0) {
      return this.type.eq(e.type) && this.from + t == e.from && this.to + t == e.to;
    }
    map(e, t, n) {
      return this.type.map(e, this, t, n);
    }
    static widget(e, t, n) {
      return new fe(e, e, new nn(t, n));
    }
    static inline(e, t, n, i) {
      return new fe(e, t, new ve(n, i));
    }
    static node(e, t, n, i) {
      return new fe(e, t, new mr(n, i));
    }
    get spec() {
      return this.type.spec;
    }
    get inline() {
      return this.type instanceof ve;
    }
    get widget() {
      return this.type instanceof nn;
    }
  }
  const tt = [], je = {};
  class z {
    constructor(e, t) {
      this.local = e.length ? e : tt, this.children = t.length ? t : tt;
    }
    static create(e, t) {
      return t.length ? rn(t, e, 0, je) : $;
    }
    find(e, t, n) {
      let i = [];
      return this.findInner(e ?? 0, t ?? 1e9, i, 0, n), i;
    }
    findInner(e, t, n, i, s) {
      for (let o = 0; o < this.local.length; o++) {
        let l = this.local[o];
        l.from <= t && l.to >= e && (!s || s(l.spec)) && n.push(l.copy(l.from + i, l.to + i));
      }
      for (let o = 0; o < this.children.length; o += 3) if (this.children[o] < t && this.children[o + 1] > e) {
        let l = this.children[o] + 1;
        this.children[o + 2].findInner(e - l, t - l, n, i + l, s);
      }
    }
    map(e, t, n) {
      return this == $ || e.maps.length == 0 ? this : this.mapInner(e, t, 0, 0, n || je);
    }
    mapInner(e, t, n, i, s) {
      let o;
      for (let l = 0; l < this.local.length; l++) {
        let a = this.local[l].map(e, n, i);
        a && a.type.valid(t, a) ? (o || (o = [])).push(a) : s.onRemove && s.onRemove(this.local[l].spec);
      }
      return this.children.length ? Od(this.children, o || [], e, t, n, i, s) : o ? new z(o.sort(Ke), tt) : $;
    }
    add(e, t) {
      return t.length ? this == $ ? z.create(e, t) : this.addInner(e, t, 0) : this;
    }
    addInner(e, t, n) {
      let i, s = 0;
      e.forEach((l, a) => {
        let c = a + n, f;
        if (f = Ys(t, l, c)) {
          for (i || (i = this.children.slice()); s < i.length && i[s] < a; ) s += 3;
          i[s] == a ? i[s + 2] = i[s + 2].addInner(l, f, c + 1) : i.splice(s, 0, a, a + l.nodeSize, rn(f, l, c + 1, je)), s += 3;
        }
      });
      let o = Gs(s ? Qs(t) : t, -n);
      for (let l = 0; l < o.length; l++) o[l].type.valid(e, o[l]) || o.splice(l--, 1);
      return new z(o.length ? this.local.concat(o).sort(Ke) : this.local, i || this.children);
    }
    remove(e) {
      return e.length == 0 || this == $ ? this : this.removeInner(e, 0);
    }
    removeInner(e, t) {
      let n = this.children, i = this.local;
      for (let s = 0; s < n.length; s += 3) {
        let o, l = n[s] + t, a = n[s + 1] + t;
        for (let f = 0, d; f < e.length; f++) (d = e[f]) && d.from > l && d.to < a && (e[f] = null, (o || (o = [])).push(d));
        if (!o) continue;
        n == this.children && (n = this.children.slice());
        let c = n[s + 2].removeInner(o, l + 1);
        c != $ ? n[s + 2] = c : (n.splice(s, 3), s -= 3);
      }
      if (i.length) {
        for (let s = 0, o; s < e.length; s++) if (o = e[s]) for (let l = 0; l < i.length; l++) i[l].eq(o, t) && (i == this.local && (i = this.local.slice()), i.splice(l--, 1));
      }
      return n == this.children && i == this.local ? this : i.length || n.length ? new z(i, n) : $;
    }
    forChild(e, t) {
      if (this == $) return this;
      if (t.isLeaf) return z.empty;
      let n, i;
      for (let l = 0; l < this.children.length; l += 3) if (this.children[l] >= e) {
        this.children[l] == e && (n = this.children[l + 2]);
        break;
      }
      let s = e + 1, o = s + t.content.size;
      for (let l = 0; l < this.local.length; l++) {
        let a = this.local[l];
        if (a.from < o && a.to > s && a.type instanceof ve) {
          let c = Math.max(s, a.from) - s, f = Math.min(o, a.to) - s;
          c < f && (i || (i = [])).push(a.copy(c, f));
        }
      }
      if (i) {
        let l = new z(i.sort(Ke), tt);
        return n ? new Ne([
          l,
          n
        ]) : l;
      }
      return n || $;
    }
    eq(e) {
      if (this == e) return true;
      if (!(e instanceof z) || this.local.length != e.local.length || this.children.length != e.children.length) return false;
      for (let t = 0; t < this.local.length; t++) if (!this.local[t].eq(e.local[t])) return false;
      for (let t = 0; t < this.children.length; t += 3) if (this.children[t] != e.children[t] || this.children[t + 1] != e.children[t + 1] || !this.children[t + 2].eq(e.children[t + 2])) return false;
      return true;
    }
    locals(e) {
      return gr(this.localsInner(e));
    }
    localsInner(e) {
      if (this == $) return tt;
      if (e.inlineContent || !this.local.some(ve.is)) return this.local;
      let t = [];
      for (let n = 0; n < this.local.length; n++) this.local[n].type instanceof ve || t.push(this.local[n]);
      return t;
    }
    forEachSet(e) {
      e(this);
    }
  }
  z.empty = new z([], []);
  z.removeOverlap = gr;
  const $ = z.empty;
  class Ne {
    constructor(e) {
      this.members = e;
    }
    map(e, t) {
      const n = this.members.map((i) => i.map(e, t, je));
      return Ne.from(n);
    }
    forChild(e, t) {
      if (t.isLeaf) return z.empty;
      let n = [];
      for (let i = 0; i < this.members.length; i++) {
        let s = this.members[i].forChild(e, t);
        s != $ && (s instanceof Ne ? n = n.concat(s.members) : n.push(s));
      }
      return Ne.from(n);
    }
    eq(e) {
      if (!(e instanceof Ne) || e.members.length != this.members.length) return false;
      for (let t = 0; t < this.members.length; t++) if (!this.members[t].eq(e.members[t])) return false;
      return true;
    }
    locals(e) {
      let t, n = true;
      for (let i = 0; i < this.members.length; i++) {
        let s = this.members[i].localsInner(e);
        if (s.length) if (!t) t = s;
        else {
          n && (t = t.slice(), n = false);
          for (let o = 0; o < s.length; o++) t.push(s[o]);
        }
      }
      return t ? gr(n ? t : t.sort(Ke)) : tt;
    }
    static from(e) {
      switch (e.length) {
        case 0:
          return $;
        case 1:
          return e[0];
        default:
          return new Ne(e.every((t) => t instanceof z) ? e : e.reduce((t, n) => t.concat(n instanceof z ? n : n.members), []));
      }
    }
    forEachSet(e) {
      for (let t = 0; t < this.members.length; t++) this.members[t].forEachSet(e);
    }
  }
  function Od(r, e, t, n, i, s, o) {
    let l = r.slice();
    for (let c = 0, f = s; c < t.maps.length; c++) {
      let d = 0;
      t.maps[c].forEach((u, m, p, _) => {
        let y = _ - p - (m - u);
        for (let S = 0; S < l.length; S += 3) {
          let L = l[S + 1];
          if (L < 0 || u > L + f - d) continue;
          let R = l[S] + f - d;
          m >= R ? l[S + 1] = u <= R ? -2 : -1 : u >= f && y && (l[S] += y, l[S + 1] += y);
        }
        d += y;
      }), f = t.maps[c].map(f, -1);
    }
    let a = false;
    for (let c = 0; c < l.length; c += 3) if (l[c + 1] < 0) {
      if (l[c + 1] == -2) {
        a = true, l[c + 1] = -1;
        continue;
      }
      let f = t.map(r[c] + s), d = f - i;
      if (d < 0 || d >= n.content.size) {
        a = true;
        continue;
      }
      let u = t.map(r[c + 1] + s, -1), m = u - i, { index: p, offset: _ } = n.content.findIndex(d), y = n.maybeChild(p);
      if (y && _ == d && _ + y.nodeSize == m) {
        let S = l[c + 2].mapInner(t, y, f + 1, r[c] + s + 1, o);
        S != $ ? (l[c] = d, l[c + 1] = m, l[c + 2] = S) : (l[c + 1] = -2, a = true);
      } else a = true;
    }
    if (a) {
      let c = Md(l, r, e, t, i, s, o), f = rn(c, n, 0, o);
      e = f.local;
      for (let d = 0; d < l.length; d += 3) l[d + 1] < 0 && (l.splice(d, 3), d -= 3);
      for (let d = 0, u = 0; d < f.children.length; d += 3) {
        let m = f.children[d];
        for (; u < l.length && l[u] < m; ) u += 3;
        l.splice(u, 0, f.children[d], f.children[d + 1], f.children[d + 2]);
      }
    }
    return new z(e.sort(Ke), l);
  }
  function Gs(r, e) {
    if (!e || !r.length) return r;
    let t = [];
    for (let n = 0; n < r.length; n++) {
      let i = r[n];
      t.push(new fe(i.from + e, i.to + e, i.type));
    }
    return t;
  }
  function Md(r, e, t, n, i, s, o) {
    function l(a, c) {
      for (let f = 0; f < a.local.length; f++) {
        let d = a.local[f].map(n, i, c);
        d ? t.push(d) : o.onRemove && o.onRemove(a.local[f].spec);
      }
      for (let f = 0; f < a.children.length; f += 3) l(a.children[f + 2], a.children[f] + c + 1);
    }
    for (let a = 0; a < r.length; a += 3) r[a + 1] == -1 && l(r[a + 2], e[a] + s + 1);
    return t;
  }
  function Ys(r, e, t) {
    if (e.isLeaf) return null;
    let n = t + e.nodeSize, i = null;
    for (let s = 0, o; s < r.length; s++) (o = r[s]) && o.from > t && o.to < n && ((i || (i = [])).push(o), r[s] = null);
    return i;
  }
  function Qs(r) {
    let e = [];
    for (let t = 0; t < r.length; t++) r[t] != null && e.push(r[t]);
    return e;
  }
  function rn(r, e, t, n) {
    let i = [], s = false;
    e.forEach((l, a) => {
      let c = Ys(r, l, a + t);
      if (c) {
        s = true;
        let f = rn(c, l, t + a + 1, n);
        f != $ && i.push(a, a + l.nodeSize, f);
      }
    });
    let o = Gs(s ? Qs(r) : r, -t).sort(Ke);
    for (let l = 0; l < o.length; l++) o[l].type.valid(e, o[l]) || (n.onRemove && n.onRemove(o[l].spec), o.splice(l--, 1));
    return o.length || i.length ? new z(o, i) : $;
  }
  function Ke(r, e) {
    return r.from - e.from || r.to - e.to;
  }
  function gr(r) {
    let e = r;
    for (let t = 0; t < e.length - 1; t++) {
      let n = e[t];
      if (n.from != n.to) for (let i = t + 1; i < e.length; i++) {
        let s = e[i];
        if (s.from == n.from) {
          s.to != n.to && (e == r && (e = r.slice()), e[i] = s.copy(s.from, n.to), fi(e, i + 1, s.copy(n.to, s.to)));
          continue;
        } else {
          s.from < n.to && (e == r && (e = r.slice()), e[t] = n.copy(n.from, s.from), fi(e, i, n.copy(s.from, n.to)));
          break;
        }
      }
    }
    return e;
  }
  function fi(r, e, t) {
    for (; e < r.length && Ke(t, r[e]) > 0; ) e++;
    r.splice(e, 0, t);
  }
  function Nn(r) {
    let e = [];
    return r.someProp("decorations", (t) => {
      let n = t(r.state);
      n && n != $ && e.push(n);
    }), r.cursorWrapper && e.push(z.create(r.state.doc, [
      r.cursorWrapper.deco
    ])), Ne.from(e);
  }
  const Nd = {
    childList: true,
    characterData: true,
    characterDataOldValue: true,
    attributes: true,
    attributeOldValue: true,
    subtree: true
  }, Ed = Z && De <= 11;
  class Td {
    constructor() {
      this.anchorNode = null, this.anchorOffset = 0, this.focusNode = null, this.focusOffset = 0;
    }
    set(e) {
      this.anchorNode = e.anchorNode, this.anchorOffset = e.anchorOffset, this.focusNode = e.focusNode, this.focusOffset = e.focusOffset;
    }
    clear() {
      this.anchorNode = this.focusNode = null;
    }
    eq(e) {
      return e.anchorNode == this.anchorNode && e.anchorOffset == this.anchorOffset && e.focusNode == this.focusNode && e.focusOffset == this.focusOffset;
    }
  }
  class Dd {
    constructor(e, t) {
      this.view = e, this.handleDOMChange = t, this.queue = [], this.flushingSoon = -1, this.observer = null, this.currentSelection = new Td(), this.onCharData = null, this.suppressingSelectionUpdates = false, this.lastChangedTextNode = null, this.observer = window.MutationObserver && new window.MutationObserver((n) => {
        for (let i = 0; i < n.length; i++) this.queue.push(n[i]);
        Z && De <= 11 && n.some((i) => i.type == "childList" && i.removedNodes.length || i.type == "characterData" && i.oldValue.length > i.target.nodeValue.length) ? this.flushSoon() : K && e.composing && n.some((i) => i.type == "childList" && i.target.nodeName == "TR") ? (e.input.badSafariComposition = true, this.flushSoon()) : this.flush();
      }), Ed && (this.onCharData = (n) => {
        this.queue.push({
          target: n.target,
          type: "characterData",
          oldValue: n.prevValue
        }), this.flushSoon();
      }), this.onSelectionChange = this.onSelectionChange.bind(this);
    }
    flushSoon() {
      this.flushingSoon < 0 && (this.flushingSoon = window.setTimeout(() => {
        this.flushingSoon = -1, this.flush();
      }, 20));
    }
    forceFlush() {
      this.flushingSoon > -1 && (window.clearTimeout(this.flushingSoon), this.flushingSoon = -1, this.flush());
    }
    start() {
      this.observer && (this.observer.takeRecords(), this.observer.observe(this.view.dom, Nd)), this.onCharData && this.view.dom.addEventListener("DOMCharacterDataModified", this.onCharData), this.connectSelection();
    }
    stop() {
      if (this.observer) {
        let e = this.observer.takeRecords();
        if (e.length) {
          for (let t = 0; t < e.length; t++) this.queue.push(e[t]);
          window.setTimeout(() => this.flush(), 20);
        }
        this.observer.disconnect();
      }
      this.onCharData && this.view.dom.removeEventListener("DOMCharacterDataModified", this.onCharData), this.disconnectSelection();
    }
    connectSelection() {
      this.view.dom.ownerDocument.addEventListener("selectionchange", this.onSelectionChange);
    }
    disconnectSelection() {
      this.view.dom.ownerDocument.removeEventListener("selectionchange", this.onSelectionChange);
    }
    suppressSelectionUpdates() {
      this.suppressingSelectionUpdates = true, setTimeout(() => this.suppressingSelectionUpdates = false, 50);
    }
    onSelectionChange() {
      if (ri(this.view)) {
        if (this.suppressingSelectionUpdates) return Se(this.view);
        if (Z && De <= 11 && !this.view.state.selection.empty) {
          let e = this.view.domSelectionRange();
          if (e.focusNode && Ye(e.focusNode, e.focusOffset, e.anchorNode, e.anchorOffset)) return this.flushSoon();
        }
        this.flush();
      }
    }
    setCurSelection() {
      this.currentSelection.set(this.view.domSelectionRange());
    }
    ignoreSelectionChange(e) {
      if (!e.focusNode) return true;
      let t = /* @__PURE__ */ new Set(), n;
      for (let s = e.focusNode; s; s = st(s)) t.add(s);
      for (let s = e.anchorNode; s; s = st(s)) if (t.has(s)) {
        n = s;
        break;
      }
      let i = n && this.view.docView.nearestDesc(n);
      if (i && i.ignoreMutation({
        type: "selection",
        target: n.nodeType == 3 ? n.parentNode : n
      })) return this.setCurSelection(), true;
    }
    pendingRecords() {
      if (this.observer) for (let e of this.observer.takeRecords()) this.queue.push(e);
      return this.queue;
    }
    flush() {
      let { view: e } = this;
      if (!e.docView || this.flushingSoon > -1) return;
      let t = this.pendingRecords();
      t.length && (this.queue = []);
      let n = e.domSelectionRange(), i = !this.suppressingSelectionUpdates && !this.currentSelection.eq(n) && ri(e) && !this.ignoreSelectionChange(n), s = -1, o = -1, l = false, a = [];
      if (e.editable) for (let f = 0; f < t.length; f++) {
        let d = this.registerMutation(t[f], a);
        d && (s = s < 0 ? d.from : Math.min(d.from, s), o = o < 0 ? d.to : Math.max(d.to, o), d.typeOver && (l = true));
      }
      if (a.some((f) => f.nodeName == "BR") && (e.input.lastKeyCode == 8 || e.input.lastKeyCode == 46 || F && (e.composing || e.input.compositionEndedAt > Date.now() - 50) && t.some((f) => f.type == "childList" && f.removedNodes.length))) {
        for (let f of a) if (f.nodeName == "BR" && f.parentNode) {
          let d = f.nextSibling;
          for (; d && d.nodeType == 1; ) {
            if (d.contentEditable == "false") {
              f.parentNode.removeChild(f);
              break;
            }
            d = d.firstChild;
          }
        }
      } else if (se && a.length) {
        let f = a.filter((d) => d.nodeName == "BR");
        if (f.length == 2) {
          let [d, u] = f;
          d.parentNode && d.parentNode.parentNode == u.parentNode ? u.remove() : d.remove();
        } else {
          let { focusNode: d } = this.currentSelection;
          for (let u of f) {
            let m = u.parentNode;
            m && m.nodeName == "LI" && (!d || vd(e, d) != m) && u.remove();
          }
        }
      }
      let c = null;
      s < 0 && i && e.input.lastFocus > Date.now() - 200 && Math.max(e.input.lastTouch, e.input.lastClick.time) < Date.now() - 300 && fn(n) && (c = cr(e)) && c.eq(I.near(e.state.doc.resolve(0), 1)) ? (e.input.lastFocus = 0, Se(e), this.currentSelection.set(n), e.scrollToSelection()) : (s > -1 || i) && (s > -1 && (e.docView.markDirty(s, o), Id(e)), e.input.badSafariComposition && (e.input.badSafariComposition = false, Rd(e, a)), this.handleDOMChange(s, o, l, a), e.docView && e.docView.dirty ? e.updateState(e.state) : this.currentSelection.eq(n) || Se(e), this.currentSelection.set(n));
    }
    registerMutation(e, t) {
      if (t.indexOf(e.target) > -1) return null;
      let n = this.view.docView.nearestDesc(e.target);
      if (e.type == "attributes" && (n == this.view.docView || e.attributeName == "contenteditable" || e.attributeName == "style" && !e.oldValue && !e.target.getAttribute("style")) || !n || n.ignoreMutation(e)) return null;
      if (e.type == "childList") {
        for (let f = 0; f < e.addedNodes.length; f++) {
          let d = e.addedNodes[f];
          t.push(d), d.nodeType == 3 && (this.lastChangedTextNode = d);
        }
        if (n.contentDOM && n.contentDOM != n.dom && !n.contentDOM.contains(e.target)) return {
          from: n.posBefore,
          to: n.posAfter
        };
        let i = e.previousSibling, s = e.nextSibling;
        if (Z && De <= 11 && e.addedNodes.length) for (let f = 0; f < e.addedNodes.length; f++) {
          let { previousSibling: d, nextSibling: u } = e.addedNodes[f];
          (!d || Array.prototype.indexOf.call(e.addedNodes, d) < 0) && (i = d), (!u || Array.prototype.indexOf.call(e.addedNodes, u) < 0) && (s = u);
        }
        let o = i && i.parentNode == e.target ? W(i) + 1 : 0, l = n.localPosFromDOM(e.target, o, -1), a = s && s.parentNode == e.target ? W(s) : e.target.childNodes.length, c = n.localPosFromDOM(e.target, a, 1);
        return {
          from: l,
          to: c
        };
      } else return e.type == "attributes" ? {
        from: n.posAtStart - n.border,
        to: n.posAtEnd + n.border
      } : (this.lastChangedTextNode = e.target, {
        from: n.posAtStart,
        to: n.posAtEnd,
        typeOver: e.target.nodeValue == e.oldValue
      });
    }
  }
  let di = /* @__PURE__ */ new WeakMap(), ui = false;
  function Id(r) {
    if (!di.has(r) && (di.set(r, null), [
      "normal",
      "nowrap",
      "pre-line"
    ].indexOf(getComputedStyle(r.dom).whiteSpace) !== -1)) {
      if (r.requiresGeckoHackNode = se, ui) return;
      console.warn("ProseMirror expects the CSS white-space property to be set, preferably to 'pre-wrap'. It is recommended to load style/prosemirror.css from the prosemirror-view package."), ui = true;
    }
  }
  function hi(r, e) {
    let t = e.startContainer, n = e.startOffset, i = e.endContainer, s = e.endOffset, o = r.domAtPos(r.state.selection.anchor);
    return Ye(o.node, o.offset, i, s) && ([t, n, i, s] = [
      i,
      s,
      t,
      n
    ]), {
      anchorNode: t,
      anchorOffset: n,
      focusNode: i,
      focusOffset: s
    };
  }
  function Ad(r, e) {
    if (e.getComposedRanges) {
      let i = e.getComposedRanges(r.root)[0];
      if (i) return hi(r, i);
    }
    let t;
    function n(i) {
      i.preventDefault(), i.stopImmediatePropagation(), t = i.getTargetRanges()[0];
    }
    return r.dom.addEventListener("beforeinput", n, true), document.execCommand("indent"), r.dom.removeEventListener("beforeinput", n, true), t ? hi(r, t) : null;
  }
  function vd(r, e) {
    for (let t = e.parentNode; t && t != r.dom; t = t.parentNode) {
      let n = r.docView.nearestDesc(t, true);
      if (n && n.node.isBlock) return t;
    }
    return null;
  }
  function Rd(r, e) {
    var t;
    let { focusNode: n, focusOffset: i } = r.domSelectionRange();
    for (let s of e) if (((t = s.parentNode) === null || t === void 0 ? void 0 : t.nodeName) == "TR") {
      let o = s.nextSibling;
      for (; o && o.nodeName != "TD" && o.nodeName != "TH"; ) o = o.nextSibling;
      if (o) {
        let l = o;
        for (; ; ) {
          let a = l.firstChild;
          if (!a || a.nodeType != 1 || a.contentEditable == "false" || /^(BR|IMG)$/.test(a.nodeName)) break;
          l = a;
        }
        l.insertBefore(s, l.firstChild), n == s && r.domSelection().collapse(s, i);
      } else s.parentNode.removeChild(s);
    }
  }
  function Pd(r, e, t) {
    let { node: n, fromOffset: i, toOffset: s, from: o, to: l } = r.docView.parseRange(e, t), a = r.domSelectionRange(), c, f = a.anchorNode;
    if (f && r.dom.contains(f.nodeType == 1 ? f : f.parentNode) && (c = [
      {
        node: f,
        offset: a.anchorOffset
      }
    ], fn(a) || c.push({
      node: a.focusNode,
      offset: a.focusOffset
    })), F && r.input.lastKeyCode === 8) for (let y = s; y > i; y--) {
      let S = n.childNodes[y - 1], L = S.pmViewDesc;
      if (S.nodeName == "BR" && !L) {
        s = y;
        break;
      }
      if (!L || L.size) break;
    }
    let d = r.state.doc, u = r.someProp("domParser") || Ct.fromSchema(r.state.schema), m = d.resolve(o), p = null, _ = u.parse(n, {
      topNode: m.parent,
      topMatch: m.parent.contentMatchAt(m.index()),
      topOpen: true,
      from: i,
      to: s,
      preserveWhitespace: m.parent.type.whitespace == "pre" ? "full" : true,
      findPositions: c,
      ruleFromNode: Bd,
      context: m
    });
    if (c && c[0].pos != null) {
      let y = c[0].pos, S = c[1] && c[1].pos;
      S == null && (S = y), p = {
        anchor: y + o,
        head: S + o
      };
    }
    return {
      doc: _,
      sel: p,
      from: o,
      to: l
    };
  }
  function Bd(r) {
    let e = r.pmViewDesc;
    if (e) return e.parseRule();
    if (r.nodeName == "BR" && r.parentNode) {
      if (K && /^(ul|ol)$/i.test(r.parentNode.nodeName)) {
        let t = document.createElement("div");
        return t.appendChild(document.createElement("li")), {
          skip: t
        };
      } else if (r.parentNode.lastChild == r || K && /^(tr|table)$/i.test(r.parentNode.nodeName)) return {
        ignore: true
      };
    } else if (r.nodeName == "IMG" && r.getAttribute("mark-placeholder")) return {
      ignore: true
    };
    return null;
  }
  const zd = /^(a|abbr|acronym|b|bd[io]|big|br|button|cite|code|data(list)?|del|dfn|em|i|img|ins|kbd|label|map|mark|meter|output|q|ruby|s|samp|small|span|strong|su[bp]|time|u|tt|var)$/i;
  function Fd(r, e, t, n, i) {
    let s = r.input.compositionPendingChanges || (r.composing ? r.input.compositionID : 0);
    if (r.input.compositionPendingChanges = 0, e < 0) {
      let M = r.input.lastSelectionTime > Date.now() - 50 ? r.input.lastSelectionOrigin : null, P = cr(r, M);
      if (P && !r.state.selection.eq(P)) {
        if (F && ke && r.input.lastKeyCode === 13 && Date.now() - 100 < r.input.lastKeyCodeTime && r.someProp("handleKeyDown", (co) => co(r, Fe(13, "Enter")))) return;
        let Q = r.state.tr.setSelection(P);
        M == "pointer" ? Q.setMeta("pointer", true) : M == "key" && Q.scrollIntoView(), s && Q.setMeta("composition", s), r.dispatch(Q);
      }
      return;
    }
    let o = r.state.doc.resolve(e), l = o.sharedDepth(t);
    e = o.before(l + 1), t = r.state.doc.resolve(t).after(l + 1);
    let a = r.state.selection, c = Pd(r, e, t), f = r.state.doc, d = f.slice(c.from, c.to), u, m;
    r.input.lastKeyCode === 8 && Date.now() - 100 < r.input.lastKeyCodeTime ? (u = r.state.selection.to, m = "end") : (u = r.state.selection.from, m = "start"), r.input.lastKeyCode = null;
    let p = qd(d.content, c.doc.content, c.from, u, m);
    if (p && r.input.domChangeCount++, (ot && r.input.lastIOSEnter > Date.now() - 225 || ke) && i.some((M) => M.nodeType == 1 && !zd.test(M.nodeName)) && (!p || p.endA >= p.endB) && r.someProp("handleKeyDown", (M) => M(r, Fe(13, "Enter")))) {
      r.input.lastIOSEnter = 0;
      return;
    }
    if (!p) if (n && a instanceof E && !a.empty && a.$head.sameParent(a.$anchor) && !r.composing && !(c.sel && c.sel.anchor != c.sel.head)) p = {
      start: a.from,
      endA: a.to,
      endB: a.to
    };
    else {
      if (c.sel) {
        let M = pi(r, r.state.doc, c.sel);
        if (M && !M.eq(r.state.selection)) {
          let P = r.state.tr.setSelection(M);
          s && P.setMeta("composition", s), r.dispatch(P);
        }
      }
      return;
    }
    r.state.selection.from < r.state.selection.to && p.start == p.endB && r.state.selection instanceof E && (p.start > r.state.selection.from && p.start <= r.state.selection.from + 2 && r.state.selection.from >= c.from ? p.start = r.state.selection.from : p.endA < r.state.selection.to && p.endA >= r.state.selection.to - 2 && r.state.selection.to <= c.to && (p.endB += r.state.selection.to - p.endA, p.endA = r.state.selection.to)), Z && De <= 11 && p.endB == p.start + 1 && p.endA == p.start && p.start > c.from && c.doc.textBetween(p.start - c.from - 1, p.start - c.from + 1) == " \xA0" && (p.start--, p.endA--, p.endB--);
    let _ = c.doc.resolveNoCache(p.start - c.from), y = c.doc.resolveNoCache(p.endB - c.from), S = f.resolve(p.start), L = _.sameParent(y) && _.parent.inlineContent && S.end() >= p.endA;
    if ((ot && r.input.lastIOSEnter > Date.now() - 225 && (!L || i.some((M) => M.nodeName == "DIV" || M.nodeName == "P")) || !L && _.pos < c.doc.content.size && (!_.sameParent(y) || !_.parent.inlineContent) && _.pos < y.pos && !/\S/.test(c.doc.textBetween(_.pos, y.pos, "", ""))) && r.someProp("handleKeyDown", (M) => M(r, Fe(13, "Enter")))) {
      r.input.lastIOSEnter = 0;
      return;
    }
    if (r.state.selection.anchor > p.start && Ld(f, p.start, p.endA, _, y) && r.someProp("handleKeyDown", (M) => M(r, Fe(8, "Backspace")))) {
      ke && F && r.domObserver.suppressSelectionUpdates();
      return;
    }
    F && p.endB == p.start && (r.input.lastChromeDelete = Date.now()), ke && !L && _.start() != y.start() && y.parentOffset == 0 && _.depth == y.depth && c.sel && c.sel.anchor == c.sel.head && c.sel.head == p.endA && (p.endB -= 2, y = c.doc.resolveNoCache(p.endB - c.from), setTimeout(() => {
      r.someProp("handleKeyDown", function(M) {
        return M(r, Fe(13, "Enter"));
      });
    }, 20));
    let R = p.start, ze = p.endA, ct = (M) => {
      let P = M || r.state.tr.replace(R, ze, c.doc.slice(p.start - c.from, p.endB - c.from));
      if (c.sel) {
        let Q = pi(r, P.doc, c.sel);
        Q && !(F && r.composing && Q.empty && (p.start != p.endB || r.input.lastChromeDelete < Date.now() - 100) && (Q.head == R || Q.head == P.mapping.map(ze) - 1) || Z && Q.empty && Q.head == R) && P.setSelection(Q);
      }
      return s && P.setMeta("composition", s), P.scrollIntoView();
    }, zt;
    if (L) if (_.pos == y.pos) {
      Z && De <= 11 && _.parentOffset == 0 && (r.domObserver.suppressSelectionUpdates(), setTimeout(() => Se(r), 20));
      let M = ct(r.state.tr.delete(R, ze)), P = f.resolve(p.start).marksAcross(f.resolve(p.endA));
      P && M.ensureMarks(P), r.dispatch(M);
    } else if (p.endA == p.endB && (zt = Vd(_.parent.content.cut(_.parentOffset, y.parentOffset), S.parent.content.cut(S.parentOffset, p.endA - S.start())))) {
      let M = ct(r.state.tr);
      zt.type == "add" ? M.addMark(R, ze, zt.mark) : M.removeMark(R, ze, zt.mark), r.dispatch(M);
    } else if (_.parent.child(_.index()).isText && _.index() == y.index() - (y.textOffset ? 0 : 1)) {
      let M = _.parent.textBetween(_.parentOffset, y.parentOffset), P = () => ct(r.state.tr.insertText(M, R, ze));
      r.someProp("handleTextInput", (Q) => Q(r, R, ze, M, P)) || r.dispatch(P());
    } else r.dispatch(ct());
    else r.dispatch(ct());
  }
  function pi(r, e, t) {
    return Math.max(t.anchor, t.head) > e.content.size ? null : fr(r, e.resolve(t.anchor), e.resolve(t.head));
  }
  function Vd(r, e) {
    let t = r.firstChild.marks, n = e.firstChild.marks, i = t, s = n, o, l, a;
    for (let f = 0; f < n.length; f++) i = n[f].removeFromSet(i);
    for (let f = 0; f < t.length; f++) s = t[f].removeFromSet(s);
    if (i.length == 1 && s.length == 0) l = i[0], o = "add", a = (f) => f.mark(l.addToSet(f.marks));
    else if (i.length == 0 && s.length == 1) l = s[0], o = "remove", a = (f) => f.mark(l.removeFromSet(f.marks));
    else return null;
    let c = [];
    for (let f = 0; f < e.childCount; f++) c.push(a(e.child(f)));
    if (b.from(c).eq(r)) return {
      mark: l,
      type: o
    };
  }
  function Ld(r, e, t, n, i) {
    if (t - e <= i.pos - n.pos || En(n, true, false) < i.pos) return false;
    let s = r.resolve(e);
    if (!n.parent.isTextblock) {
      let l = s.nodeAfter;
      return l != null && t == e + l.nodeSize;
    }
    if (s.parentOffset < s.parent.content.size || !s.parent.isTextblock) return false;
    let o = r.resolve(En(s, true, true));
    return !o.parent.isTextblock || o.pos > t || En(o, true, false) < t ? false : n.parent.content.cut(n.parentOffset).eq(o.parent.content);
  }
  function En(r, e, t) {
    let n = r.depth, i = e ? r.end() : r.pos;
    for (; n > 0 && (e || r.indexAfter(n) == r.node(n).childCount); ) n--, i++, e = false;
    if (t) {
      let s = r.node(n).maybeChild(r.indexAfter(n));
      for (; s && !s.isLeaf; ) s = s.firstChild, i++;
    }
    return i;
  }
  function qd(r, e, t, n, i) {
    let s = r.findDiffStart(e, t), o = t + r.size, l = t + e.size;
    if (s == null) return null;
    let { a, b: c } = r.findDiffEnd(e, o, l);
    if (i == "end") {
      let f = Math.max(0, s - Math.min(a, c));
      n -= a + f - s;
    }
    if (a < s && o < l) {
      let f = n <= s && n >= a ? s - n : 0;
      s -= f, c = s + (c - a), a = s;
    } else if (c < s) {
      let f = n <= s && n >= c ? s - n : 0;
      s -= f, a = s + (a - c), c = s;
    }
    return {
      start: s,
      endA: a,
      endB: c
    };
  }
  class Xs {
    constructor(e, t) {
      this._root = null, this.focused = false, this.trackWrites = null, this.mounted = false, this.markCursor = null, this.cursorWrapper = null, this.lastSelectedViewDesc = void 0, this.input = new rd(), this.prevDirectPlugins = [], this.pluginViews = [], this.requiresGeckoHackNode = false, this.dragging = null, this._props = t, this.state = t.state, this.directPlugins = t.plugins || [], this.directPlugins.forEach(bi), this.dispatch = this.dispatch.bind(this), this.dom = e && e.mount || document.createElement("div"), e && (e.appendChild ? e.appendChild(this.dom) : typeof e == "function" ? e(this.dom) : e.mount && (this.mounted = true)), this.editable = _i(this), gi(this), this.nodeViews = yi(this), this.docView = Qr(this.state.doc, mi(this), Nn(this), this.dom, this), this.domObserver = new Dd(this, (n, i, s, o) => Fd(this, n, i, s, o)), this.domObserver.start(), id(this), this.updatePluginViews();
    }
    get composing() {
      return this.input.composing;
    }
    get props() {
      if (this._props.state != this.state) {
        let e = this._props;
        this._props = {};
        for (let t in e) this._props[t] = e[t];
        this._props.state = this.state;
      }
      return this._props;
    }
    update(e) {
      e.handleDOMEvents != this._props.handleDOMEvents && Xn(this);
      let t = this._props;
      this._props = e, e.plugins && (e.plugins.forEach(bi), this.directPlugins = e.plugins), this.updateStateInner(e.state, t);
    }
    setProps(e) {
      let t = {};
      for (let n in this._props) t[n] = this._props[n];
      t.state = this.state;
      for (let n in e) t[n] = e[n];
      this.update(t);
    }
    updateState(e) {
      this.updateStateInner(e, this._props);
    }
    updateStateInner(e, t) {
      var n;
      let i = this.state, s = false, o = false;
      e.storedMarks && this.composing && (js(this), o = true), this.state = e;
      let l = i.plugins != e.plugins || this._props.plugins != t.plugins;
      if (l || this._props.plugins != t.plugins || this._props.nodeViews != t.nodeViews) {
        let m = yi(this);
        Jd(m, this.nodeViews) && (this.nodeViews = m, s = true);
      }
      (l || t.handleDOMEvents != this._props.handleDOMEvents) && Xn(this), this.editable = _i(this), gi(this);
      let a = Nn(this), c = mi(this), f = i.plugins != e.plugins && !i.doc.eq(e.doc) ? "reset" : e.scrollToSelection > i.scrollToSelection ? "to selection" : "preserve", d = s || !this.docView.matchesNode(e.doc, c, a);
      (d || !e.selection.eq(i.selection)) && (o = true);
      let u = f == "preserve" && o && this.dom.style.overflowAnchor == null && yf(this);
      if (o) {
        this.domObserver.stop();
        let m = d && (Z || F) && !this.composing && !i.selection.empty && !e.selection.empty && Wd(i.selection, e.selection);
        if (d) {
          let _ = F ? this.trackWrites = this.domSelectionRange().focusNode : null;
          this.composing && (this.input.compositionNode = bd(this)), (s || !this.docView.update(e.doc, c, a, this)) && (this.docView.updateOuterDeco(c), this.docView.destroy(), this.docView = Qr(e.doc, c, a, this.dom, this)), _ && (!this.trackWrites || !this.dom.contains(this.trackWrites)) && (m = true);
        }
        let p = this.input.mouseDown;
        m || !(p && this.domObserver.currentSelection.eq(this.domSelectionRange()) && Wf(this) && p.delaySelUpdate()) ? Se(this, m) : (Is(this, e.selection), this.domObserver.setCurSelection()), this.domObserver.start();
      }
      this.updatePluginViews(i), !((n = this.dragging) === null || n === void 0) && n.node && !i.doc.eq(e.doc) && this.updateDraggedNode(this.dragging, i), f == "reset" ? this.dom.scrollTop = 0 : f == "to selection" ? this.scrollToSelection() : u && bf(u);
    }
    scrollToSelection() {
      let e = this.domSelectionRange().focusNode;
      if (!(!e || !this.dom.contains(e.nodeType == 1 ? e : e.parentNode))) {
        if (!this.someProp("handleScrollToSelection", (t) => t(this))) if (this.state.selection instanceof O) {
          let t = this.docView.domAfterPos(this.state.selection.from);
          t.nodeType == 1 && jr(this, t.getBoundingClientRect(), e);
        } else jr(this, this.coordsAtPos(this.state.selection.head, 1), e);
      }
    }
    destroyPluginViews() {
      let e;
      for (; e = this.pluginViews.pop(); ) e.destroy && e.destroy();
    }
    updatePluginViews(e) {
      if (!e || e.plugins != this.state.plugins || this.directPlugins != this.prevDirectPlugins) {
        this.prevDirectPlugins = this.directPlugins, this.destroyPluginViews();
        for (let t = 0; t < this.directPlugins.length; t++) {
          let n = this.directPlugins[t];
          n.spec.view && this.pluginViews.push(n.spec.view(this));
        }
        for (let t = 0; t < this.state.plugins.length; t++) {
          let n = this.state.plugins[t];
          n.spec.view && this.pluginViews.push(n.spec.view(this));
        }
      } else for (let t = 0; t < this.pluginViews.length; t++) {
        let n = this.pluginViews[t];
        n.update && n.update(this, e);
      }
    }
    updateDraggedNode(e, t) {
      let n = e.node, i = -1;
      if (n.from < this.state.doc.content.size && this.state.doc.nodeAt(n.from) == n.node) i = n.from;
      else {
        let s = n.from + (this.state.doc.content.size - t.doc.content.size);
        (s > 0 && s < this.state.doc.content.size && this.state.doc.nodeAt(s)) == n.node && (i = s);
      }
      this.dragging = new Hs(e.slice, e.move, i < 0 ? void 0 : O.create(this.state.doc, i));
    }
    someProp(e, t) {
      let n = this._props && this._props[e], i;
      if (n != null && (i = t ? t(n) : n)) return i;
      for (let o = 0; o < this.directPlugins.length; o++) {
        let l = this.directPlugins[o].props[e];
        if (l != null && (i = t ? t(l) : l)) return i;
      }
      let s = this.state.plugins;
      if (s) for (let o = 0; o < s.length; o++) {
        let l = s[o].props[e];
        if (l != null && (i = t ? t(l) : l)) return i;
      }
    }
    hasFocus() {
      if (Z) {
        let e = this.root.activeElement;
        if (e == this.dom) return true;
        if (!e || !this.dom.contains(e)) return false;
        for (; e && this.dom != e && this.dom.contains(e); ) {
          if (e.contentEditable == "false") return false;
          e = e.parentElement;
        }
        return true;
      }
      return this.root.activeElement == this.dom;
    }
    focus() {
      this.domObserver.stop(), this.editable && wf(this.dom), Se(this), this.domObserver.start();
    }
    get root() {
      let e = this._root;
      if (e == null) {
        for (let t = this.dom.parentNode; t; t = t.parentNode) if (t.nodeType == 9 || t.nodeType == 11 && t.host) return t.getSelection || (Object.getPrototypeOf(t).getSelection = () => t.ownerDocument.getSelection()), this._root = t;
      }
      return e || document;
    }
    updateRoot() {
      this._root = null;
    }
    posAtCoords(e) {
      return Of(this, e);
    }
    coordsAtPos(e, t = 1) {
      return Ss(this, e, t);
    }
    domAtPos(e, t = 0) {
      return this.docView.domFromPos(e, t);
    }
    nodeDOM(e) {
      let t = this.docView.descAt(e);
      return t ? t.nodeDOM : null;
    }
    posAtDOM(e, t, n = -1) {
      let i = this.docView.posFromDOM(e, t, n);
      if (i == null) throw new RangeError("DOM position not inside the editor");
      return i;
    }
    endOfTextblock(e, t) {
      return Df(this, t || this.state, e);
    }
    pasteHTML(e, t) {
      return Tt(this, "", e, false, t || new ClipboardEvent("paste"));
    }
    pasteText(e, t) {
      return Tt(this, e, null, true, t || new ClipboardEvent("paste"));
    }
    serializeForClipboard(e) {
      return dr(this, e);
    }
    destroy() {
      this.docView && (sd(this), this.destroyPluginViews(), this.mounted ? (this.docView.update(this.state.doc, [], Nn(this), this), this.dom.textContent = "") : this.dom.parentNode && this.dom.parentNode.removeChild(this.dom), this.docView.destroy(), this.docView = null, af());
    }
    get isDestroyed() {
      return this.docView == null;
    }
    dispatchEvent(e) {
      return ld(this, e);
    }
    domSelectionRange() {
      let e = this.domSelection();
      return e ? K && this.root.nodeType === 11 && hf(this.dom.ownerDocument) == this.dom && Ad(this, e) || e : {
        focusNode: null,
        focusOffset: 0,
        anchorNode: null,
        anchorOffset: 0
      };
    }
    domSelection() {
      return this.root.getSelection();
    }
  }
  Xs.prototype.dispatch = function(r) {
    let e = this._props.dispatchTransaction;
    e ? e.call(this, r) : this.updateState(this.state.apply(r));
  };
  function mi(r) {
    let e = /* @__PURE__ */ Object.create(null);
    return e.class = "ProseMirror", e.contenteditable = String(r.editable), r.someProp("attributes", (t) => {
      if (typeof t == "function" && (t = t(r.state)), t) for (let n in t) n == "class" ? e.class += " " + t[n] : n == "style" ? e.style = (e.style ? e.style + ";" : "") + t[n] : !e[n] && n != "contenteditable" && n != "nodeName" && (e[n] = String(t[n]));
    }), e.translate || (e.translate = "no"), [
      fe.node(0, r.state.doc.content.size, e)
    ];
  }
  function gi(r) {
    if (r.markCursor) {
      let e = document.createElement("img");
      e.className = "ProseMirror-separator", e.setAttribute("mark-placeholder", "true"), e.setAttribute("alt", ""), r.cursorWrapper = {
        dom: e,
        deco: fe.widget(r.state.selection.from, e, {
          raw: true,
          marks: r.markCursor
        })
      };
    } else r.cursorWrapper = null;
  }
  function _i(r) {
    return !r.someProp("editable", (e) => e(r.state) === false);
  }
  function Wd(r, e) {
    let t = Math.min(r.$anchor.sharedDepth(r.head), e.$anchor.sharedDepth(e.head));
    return r.$anchor.start(t) != e.$anchor.start(t);
  }
  function yi(r) {
    let e = /* @__PURE__ */ Object.create(null);
    function t(n) {
      for (let i in n) Object.prototype.hasOwnProperty.call(e, i) || (e[i] = n[i]);
    }
    return r.someProp("nodeViews", t), r.someProp("markViews", t), e;
  }
  function Jd(r, e) {
    let t = 0, n = 0;
    for (let i in r) {
      if (r[i] != e[i]) return true;
      t++;
    }
    for (let i in e) n++;
    return t != n;
  }
  function bi(r) {
    if (r.spec.state || r.spec.filterTransaction || r.spec.appendTransaction) throw new RangeError("Plugins passed directly to the view must not have a state component");
  }
  var Pe = {
    8: "Backspace",
    9: "Tab",
    10: "Enter",
    12: "NumLock",
    13: "Enter",
    16: "Shift",
    17: "Control",
    18: "Alt",
    20: "CapsLock",
    27: "Escape",
    32: " ",
    33: "PageUp",
    34: "PageDown",
    35: "End",
    36: "Home",
    37: "ArrowLeft",
    38: "ArrowUp",
    39: "ArrowRight",
    40: "ArrowDown",
    44: "PrintScreen",
    45: "Insert",
    46: "Delete",
    59: ";",
    61: "=",
    91: "Meta",
    92: "Meta",
    106: "*",
    107: "+",
    108: ",",
    109: "-",
    110: ".",
    111: "/",
    144: "NumLock",
    145: "ScrollLock",
    160: "Shift",
    161: "Shift",
    162: "Control",
    163: "Control",
    164: "Alt",
    165: "Alt",
    173: "-",
    186: ";",
    187: "=",
    188: ",",
    189: "-",
    190: ".",
    191: "/",
    192: "`",
    219: "[",
    220: "\\",
    221: "]",
    222: "'"
  }, sn = {
    48: ")",
    49: "!",
    50: "@",
    51: "#",
    52: "$",
    53: "%",
    54: "^",
    55: "&",
    56: "*",
    57: "(",
    59: ":",
    61: "+",
    173: "_",
    186: ":",
    187: "+",
    188: "<",
    189: "_",
    190: ">",
    191: "?",
    192: "~",
    219: "{",
    220: "|",
    221: "}",
    222: '"'
  }, $d = typeof navigator < "u" && /Mac/.test(navigator.platform), jd = typeof navigator < "u" && /MSIE \d|Trident\/(?:[7-9]|\d{2,})\..*rv:(\d+)/.exec(navigator.userAgent);
  for (var J = 0; J < 10; J++) Pe[48 + J] = Pe[96 + J] = String(J);
  for (var J = 1; J <= 24; J++) Pe[J + 111] = "F" + J;
  for (var J = 65; J <= 90; J++) Pe[J] = String.fromCharCode(J + 32), sn[J] = String.fromCharCode(J);
  for (var Tn in Pe) sn.hasOwnProperty(Tn) || (sn[Tn] = Pe[Tn]);
  function Kd(r) {
    var e = $d && r.metaKey && r.shiftKey && !r.ctrlKey && !r.altKey || jd && r.shiftKey && r.key && r.key.length == 1 || r.key == "Unidentified", t = !e && r.key || (r.shiftKey ? sn : Pe)[r.keyCode] || r.key || "Unidentified";
    return t == "Esc" && (t = "Escape"), t == "Del" && (t = "Delete"), t == "Left" && (t = "ArrowLeft"), t == "Up" && (t = "ArrowUp"), t == "Right" && (t = "ArrowRight"), t == "Down" && (t = "ArrowDown"), t;
  }
  const Hd = typeof navigator < "u" && /Mac|iP(hone|[oa]d)/.test(navigator.platform), Ud = typeof navigator < "u" && /Win/.test(navigator.platform);
  function Gd(r) {
    let e = r.split(/-(?!$)/), t = e[e.length - 1];
    t == "Space" && (t = " ");
    let n, i, s, o;
    for (let l = 0; l < e.length - 1; l++) {
      let a = e[l];
      if (/^(cmd|meta|m)$/i.test(a)) o = true;
      else if (/^a(lt)?$/i.test(a)) n = true;
      else if (/^(c|ctrl|control)$/i.test(a)) i = true;
      else if (/^s(hift)?$/i.test(a)) s = true;
      else if (/^mod$/i.test(a)) Hd ? o = true : i = true;
      else throw new Error("Unrecognized modifier name: " + a);
    }
    return n && (t = "Alt-" + t), i && (t = "Ctrl-" + t), o && (t = "Meta-" + t), s && (t = "Shift-" + t), t;
  }
  function Yd(r) {
    let e = /* @__PURE__ */ Object.create(null);
    for (let t in r) e[Gd(t)] = r[t];
    return e;
  }
  function Dn(r, e, t = true) {
    return e.altKey && (r = "Alt-" + r), e.ctrlKey && (r = "Ctrl-" + r), e.metaKey && (r = "Meta-" + r), t && e.shiftKey && (r = "Shift-" + r), r;
  }
  function wi(r) {
    return new us({
      props: {
        handleKeyDown: Qd(r)
      }
    });
  }
  function Qd(r) {
    let e = Yd(r);
    return function(t, n) {
      let i = Kd(n), s, o = e[Dn(i, n)];
      if (o && o(t.state, t.dispatch, t)) return true;
      if (i.length == 1 && i != " ") {
        if (n.shiftKey) {
          let l = e[Dn(i, n, false)];
          if (l && l(t.state, t.dispatch, t)) return true;
        }
        if ((n.altKey || n.metaKey || n.ctrlKey) && !(Ud && n.ctrlKey && n.altKey) && (s = Pe[n.keyCode]) && s != i) {
          let l = e[Dn(s, n)];
          if (l && l(t.state, t.dispatch, t)) return true;
        }
      }
      return false;
    };
  }
  var on = 200, V = function() {
  };
  V.prototype.append = function(e) {
    return e.length ? (e = V.from(e), !this.length && e || e.length < on && this.leafAppend(e) || this.length < on && e.leafPrepend(this) || this.appendInner(e)) : this;
  };
  V.prototype.prepend = function(e) {
    return e.length ? V.from(e).append(this) : this;
  };
  V.prototype.appendInner = function(e) {
    return new Xd(this, e);
  };
  V.prototype.slice = function(e, t) {
    return e === void 0 && (e = 0), t === void 0 && (t = this.length), e >= t ? V.empty : this.sliceInner(Math.max(0, e), Math.min(this.length, t));
  };
  V.prototype.get = function(e) {
    if (!(e < 0 || e >= this.length)) return this.getInner(e);
  };
  V.prototype.forEach = function(e, t, n) {
    t === void 0 && (t = 0), n === void 0 && (n = this.length), t <= n ? this.forEachInner(e, t, n, 0) : this.forEachInvertedInner(e, t, n, 0);
  };
  V.prototype.map = function(e, t, n) {
    t === void 0 && (t = 0), n === void 0 && (n = this.length);
    var i = [];
    return this.forEach(function(s, o) {
      return i.push(e(s, o));
    }, t, n), i;
  };
  V.from = function(e) {
    return e instanceof V ? e : e && e.length ? new Zs(e) : V.empty;
  };
  var Zs = (function(r) {
    function e(n) {
      r.call(this), this.values = n;
    }
    r && (e.__proto__ = r), e.prototype = Object.create(r && r.prototype), e.prototype.constructor = e;
    var t = {
      length: {
        configurable: true
      },
      depth: {
        configurable: true
      }
    };
    return e.prototype.flatten = function() {
      return this.values;
    }, e.prototype.sliceInner = function(i, s) {
      return i == 0 && s == this.length ? this : new e(this.values.slice(i, s));
    }, e.prototype.getInner = function(i) {
      return this.values[i];
    }, e.prototype.forEachInner = function(i, s, o, l) {
      for (var a = s; a < o; a++) if (i(this.values[a], l + a) === false) return false;
    }, e.prototype.forEachInvertedInner = function(i, s, o, l) {
      for (var a = s - 1; a >= o; a--) if (i(this.values[a], l + a) === false) return false;
    }, e.prototype.leafAppend = function(i) {
      if (this.length + i.length <= on) return new e(this.values.concat(i.flatten()));
    }, e.prototype.leafPrepend = function(i) {
      if (this.length + i.length <= on) return new e(i.flatten().concat(this.values));
    }, t.length.get = function() {
      return this.values.length;
    }, t.depth.get = function() {
      return 0;
    }, Object.defineProperties(e.prototype, t), e;
  })(V);
  V.empty = new Zs([]);
  var Xd = (function(r) {
    function e(t, n) {
      r.call(this), this.left = t, this.right = n, this.length = t.length + n.length, this.depth = Math.max(t.depth, n.depth) + 1;
    }
    return r && (e.__proto__ = r), e.prototype = Object.create(r && r.prototype), e.prototype.constructor = e, e.prototype.flatten = function() {
      return this.left.flatten().concat(this.right.flatten());
    }, e.prototype.getInner = function(n) {
      return n < this.left.length ? this.left.get(n) : this.right.get(n - this.left.length);
    }, e.prototype.forEachInner = function(n, i, s, o) {
      var l = this.left.length;
      if (i < l && this.left.forEachInner(n, i, Math.min(s, l), o) === false || s > l && this.right.forEachInner(n, Math.max(i - l, 0), Math.min(this.length, s) - l, o + l) === false) return false;
    }, e.prototype.forEachInvertedInner = function(n, i, s, o) {
      var l = this.left.length;
      if (i > l && this.right.forEachInvertedInner(n, i - l, Math.max(s, l) - l, o + l) === false || s < l && this.left.forEachInvertedInner(n, Math.min(i, l), s, o) === false) return false;
    }, e.prototype.sliceInner = function(n, i) {
      if (n == 0 && i == this.length) return this;
      var s = this.left.length;
      return i <= s ? this.left.slice(n, i) : n >= s ? this.right.slice(n - s, i - s) : this.left.slice(n, s).append(this.right.slice(0, i - s));
    }, e.prototype.leafAppend = function(n) {
      var i = this.right.leafAppend(n);
      if (i) return new e(this.left, i);
    }, e.prototype.leafPrepend = function(n) {
      var i = this.left.leafPrepend(n);
      if (i) return new e(i, this.right);
    }, e.prototype.appendInner = function(n) {
      return this.left.depth >= Math.max(this.right.depth, n.depth) + 1 ? new e(this.left, new e(this.right, n)) : new e(this, n);
    }, e;
  })(V);
  const Zd = 500;
  class ce {
    constructor(e, t) {
      this.items = e, this.eventCount = t;
    }
    popEvent(e, t) {
      if (this.eventCount == 0) return null;
      let n = this.items.length;
      for (; ; n--) if (this.items.get(n - 1).selection) {
        --n;
        break;
      }
      let i, s;
      t && (i = this.remapping(n, this.items.length), s = i.maps.length);
      let o = e.tr, l, a, c = [], f = [];
      return this.items.forEach((d, u) => {
        if (!d.step) {
          i || (i = this.remapping(n, u + 1), s = i.maps.length), s--, f.push(d);
          return;
        }
        if (i) {
          f.push(new ue(d.map));
          let m = d.step.map(i.slice(s)), p;
          m && o.maybeStep(m).doc && (p = o.mapping.maps[o.mapping.maps.length - 1], c.push(new ue(p, void 0, void 0, c.length + f.length))), s--, p && i.appendMap(p, s);
        } else o.maybeStep(d.step);
        if (d.selection) return l = i ? d.selection.map(i.slice(s)) : d.selection, a = new ce(this.items.slice(0, n).append(f.reverse().concat(c)), this.eventCount - 1), false;
      }, this.items.length, 0), {
        remaining: a,
        transform: o,
        selection: l
      };
    }
    addTransform(e, t, n, i) {
      let s = [], o = this.eventCount, l = this.items, a = !i && l.length ? l.get(l.length - 1) : null;
      for (let f = 0; f < e.steps.length; f++) {
        let d = e.steps[f].invert(e.docs[f]), u = new ue(e.mapping.maps[f], d, t), m;
        (m = a && a.merge(u)) && (u = m, f ? s.pop() : l = l.slice(0, l.length - 1)), s.push(u), t && (o++, t = void 0), i || (a = u);
      }
      let c = o - n.depth;
      return c > tu && (l = eu(l, c), o -= c), new ce(l.append(s), o);
    }
    remapping(e, t) {
      let n = new Mt();
      return this.items.forEach((i, s) => {
        let o = i.mirrorOffset != null && s - i.mirrorOffset >= e ? n.maps.length - i.mirrorOffset : void 0;
        n.appendMap(i.map, o);
      }, e, t), n;
    }
    addMaps(e) {
      return this.eventCount == 0 ? this : new ce(this.items.append(e.map((t) => new ue(t))), this.eventCount);
    }
    rebased(e, t) {
      if (!this.eventCount) return this;
      let n = [], i = Math.max(0, this.items.length - t), s = e.mapping, o = e.steps.length, l = this.eventCount;
      this.items.forEach((u) => {
        u.selection && l--;
      }, i);
      let a = t;
      this.items.forEach((u) => {
        let m = s.getMirror(--a);
        if (m == null) return;
        o = Math.min(o, m);
        let p = s.maps[m];
        if (u.step) {
          let _ = e.steps[m].invert(e.docs[m]), y = u.selection && u.selection.map(s.slice(a + 1, m));
          y && l++, n.push(new ue(p, _, y));
        } else n.push(new ue(p));
      }, i);
      let c = [];
      for (let u = t; u < o; u++) c.push(new ue(s.maps[u]));
      let f = this.items.slice(0, i).append(c).append(n), d = new ce(f, l);
      return d.emptyItemCount() > Zd && (d = d.compress(this.items.length - n.length)), d;
    }
    emptyItemCount() {
      let e = 0;
      return this.items.forEach((t) => {
        t.step || e++;
      }), e;
    }
    compress(e = this.items.length) {
      let t = this.remapping(0, e), n = t.maps.length, i = [], s = 0;
      return this.items.forEach((o, l) => {
        if (l >= e) i.push(o), o.selection && s++;
        else if (o.step) {
          let a = o.step.map(t.slice(n)), c = a && a.getMap();
          if (n--, c && t.appendMap(c, n), a) {
            let f = o.selection && o.selection.map(t.slice(n));
            f && s++;
            let d = new ue(c.invert(), a, f), u, m = i.length - 1;
            (u = i.length && i[m].merge(d)) ? i[m] = u : i.push(d);
          }
        } else o.map && n--;
      }, this.items.length, 0), new ce(V.from(i.reverse()), s);
    }
  }
  ce.empty = new ce(V.empty, 0);
  function eu(r, e) {
    let t;
    return r.forEach((n, i) => {
      if (n.selection && e-- == 0) return t = i, false;
    }), r.slice(t);
  }
  class ue {
    constructor(e, t, n, i) {
      this.map = e, this.step = t, this.selection = n, this.mirrorOffset = i;
    }
    merge(e) {
      if (this.step && e.step && !e.selection) {
        let t = e.step.merge(this.step);
        if (t) return new ue(t.getMap().invert(), t, this.selection);
      }
    }
  }
  class Me {
    constructor(e, t, n, i, s) {
      this.done = e, this.undone = t, this.prevRanges = n, this.prevTime = i, this.prevComposition = s;
    }
  }
  const tu = 20;
  function nu(r, e, t, n) {
    let i = t.getMeta(He), s;
    if (i) return i.historyState;
    t.getMeta(su) && (r = new Me(r.done, r.undone, null, 0, -1));
    let o = t.getMeta("appendedTransaction");
    if (t.steps.length == 0) return r;
    if (o && o.getMeta(He)) return o.getMeta(He).redo ? new Me(r.done.addTransform(t, void 0, n, Ut(e)), r.undone, ki(t.mapping.maps), r.prevTime, r.prevComposition) : new Me(r.done, r.undone.addTransform(t, void 0, n, Ut(e)), null, r.prevTime, r.prevComposition);
    if (t.getMeta("addToHistory") !== false && !(o && o.getMeta("addToHistory") === false)) {
      let l = t.getMeta("composition"), a = r.prevTime == 0 || !o && r.prevComposition != l && (r.prevTime < (t.time || 0) - n.newGroupDelay || !ru(t, r.prevRanges)), c = o ? In(r.prevRanges, t.mapping) : ki(t.mapping.maps);
      return new Me(r.done.addTransform(t, a ? e.selection.getBookmark() : void 0, n, Ut(e)), ce.empty, c, t.time, l ?? r.prevComposition);
    } else return (s = t.getMeta("rebased")) ? new Me(r.done.rebased(t, s), r.undone.rebased(t, s), In(r.prevRanges, t.mapping), r.prevTime, r.prevComposition) : new Me(r.done.addMaps(t.mapping.maps), r.undone.addMaps(t.mapping.maps), In(r.prevRanges, t.mapping), r.prevTime, r.prevComposition);
  }
  function ru(r, e) {
    if (!e) return false;
    if (!r.docChanged) return true;
    let t = false;
    return r.mapping.maps[0].forEach((n, i) => {
      for (let s = 0; s < e.length; s += 2) n <= e[s + 1] && i >= e[s] && (t = true);
    }), t;
  }
  function ki(r) {
    let e = [];
    for (let t = r.length - 1; t >= 0 && e.length == 0; t--) r[t].forEach((n, i, s, o) => e.push(s, o));
    return e;
  }
  function In(r, e) {
    if (!r) return null;
    let t = [];
    for (let n = 0; n < r.length; n += 2) {
      let i = e.map(r[n], 1), s = e.map(r[n + 1], -1);
      i <= s && t.push(i, s);
    }
    return t;
  }
  function iu(r, e, t) {
    let n = Ut(e), i = He.get(e).spec.config, s = (t ? r.undone : r.done).popEvent(e, n);
    if (!s) return null;
    let o = s.selection.resolve(s.transform.doc), l = (t ? r.done : r.undone).addTransform(s.transform, e.selection.getBookmark(), i, n), a = new Me(t ? l : s.remaining, t ? s.remaining : l, null, 0, -1);
    return s.transform.setSelection(o).setMeta(He, {
      redo: t,
      historyState: a
    });
  }
  let An = false, xi = null;
  function Ut(r) {
    let e = r.plugins;
    if (xi != e) {
      An = false, xi = e;
      for (let t = 0; t < e.length; t++) if (e[t].spec.historyPreserveItems) {
        An = true;
        break;
      }
    }
    return An;
  }
  const He = new ps("history"), su = new ps("closeHistory");
  function ou(r = {}) {
    return r = {
      depth: r.depth || 100,
      newGroupDelay: r.newGroupDelay || 500
    }, new us({
      key: He,
      state: {
        init() {
          return new Me(ce.empty, ce.empty, null, 0, -1);
        },
        apply(e, t, n) {
          return nu(t, n, e, r);
        }
      },
      config: r,
      props: {
        handleDOMEvents: {
          beforeinput(e, t) {
            let n = t.inputType, i = n == "historyUndo" ? to : n == "historyRedo" ? Zn : null;
            return !i || !e.editable ? false : (t.preventDefault(), i(e.state, e.dispatch));
          }
        }
      }
    });
  }
  function eo(r, e) {
    return (t, n) => {
      let i = He.getState(t);
      if (!i || (r ? i.undone : i.done).eventCount == 0) return false;
      if (n) {
        let s = iu(i, t, r);
        s && n(e ? s.scrollIntoView() : s);
      }
      return true;
    };
  }
  const to = eo(false, true), Zn = eo(true, true), no = (r, e) => r.selection.empty ? false : (e && e(r.tr.deleteSelection().scrollIntoView()), true);
  function lu(r, e) {
    let { $cursor: t } = r.selection;
    return !t || (e ? !e.endOfTextblock("backward", r) : t.parentOffset > 0) ? null : t;
  }
  const au = (r, e, t) => {
    let n = lu(r, t);
    if (!n) return false;
    let i = ro(n);
    if (!i) {
      let o = n.blockRange(), l = o && ir(o);
      return l == null ? false : (e && e(r.tr.lift(o, l).scrollIntoView()), true);
    }
    let s = i.nodeBefore;
    if (so(r, i, e, -1)) return true;
    if (n.parent.content.size == 0 && (lt(s, "end") || O.isSelectable(s))) for (let o = n.depth; ; o--) {
      let l = sr(r.doc, n.before(o), n.after(o), w.empty);
      if (l && l.slice.size < l.to - l.from) {
        if (e) {
          let a = r.tr.step(l);
          a.setSelection(lt(s, "end") ? I.findFrom(a.doc.resolve(a.mapping.map(i.pos, -1)), -1) : O.create(a.doc, i.pos - s.nodeSize)), e(a.scrollIntoView());
        }
        return true;
      }
      if (o == 1 || n.node(o - 1).childCount > 1) break;
    }
    return s.isAtom && i.depth == n.depth - 1 ? (e && e(r.tr.delete(i.pos - s.nodeSize, i.pos).scrollIntoView()), true) : false;
  };
  function lt(r, e, t = false) {
    for (let n = r; n; n = e == "start" ? n.firstChild : n.lastChild) {
      if (n.isTextblock) return true;
      if (t && n.childCount != 1) return false;
    }
    return false;
  }
  const cu = (r, e, t) => {
    let { $head: n, empty: i } = r.selection, s = n;
    if (!i) return false;
    if (n.parent.isTextblock) {
      if (t ? !t.endOfTextblock("backward", r) : n.parentOffset > 0) return false;
      s = ro(n);
    }
    let o = s && s.nodeBefore;
    return !o || !O.isSelectable(o) ? false : (e && e(r.tr.setSelection(O.create(r.doc, s.pos - o.nodeSize)).scrollIntoView()), true);
  };
  function ro(r) {
    if (!r.parent.type.spec.isolating) for (let e = r.depth - 1; e >= 0; e--) {
      if (r.index(e) > 0) return r.doc.resolve(r.before(e + 1));
      if (r.node(e).type.spec.isolating) break;
    }
    return null;
  }
  function fu(r, e) {
    let { $cursor: t } = r.selection;
    return !t || (e ? !e.endOfTextblock("forward", r) : t.parentOffset < t.parent.content.size) ? null : t;
  }
  const du = (r, e, t) => {
    let n = fu(r, t);
    if (!n) return false;
    let i = io(n);
    if (!i) return false;
    let s = i.nodeAfter;
    if (so(r, i, e, 1)) return true;
    if (n.parent.content.size == 0 && (lt(s, "start") || O.isSelectable(s))) {
      let o = sr(r.doc, n.before(), n.after(), w.empty);
      if (o && o.slice.size < o.to - o.from) {
        if (e) {
          let l = r.tr.step(o);
          l.setSelection(lt(s, "start") ? I.findFrom(l.doc.resolve(l.mapping.map(i.pos)), 1) : O.create(l.doc, l.mapping.map(i.pos))), e(l.scrollIntoView());
        }
        return true;
      }
    }
    return s.isAtom && i.depth == n.depth - 1 ? (e && e(r.tr.delete(i.pos, i.pos + s.nodeSize).scrollIntoView()), true) : false;
  }, uu = (r, e, t) => {
    let { $head: n, empty: i } = r.selection, s = n;
    if (!i) return false;
    if (n.parent.isTextblock) {
      if (t ? !t.endOfTextblock("forward", r) : n.parentOffset < n.parent.content.size) return false;
      s = io(n);
    }
    let o = s && s.nodeAfter;
    return !o || !O.isSelectable(o) ? false : (e && e(r.tr.setSelection(O.create(r.doc, s.pos)).scrollIntoView()), true);
  };
  function io(r) {
    if (!r.parent.type.spec.isolating) for (let e = r.depth - 1; e >= 0; e--) {
      let t = r.node(e);
      if (r.index(e) + 1 < t.childCount) return r.doc.resolve(r.after(e + 1));
      if (t.type.spec.isolating) break;
    }
    return null;
  }
  const hu = (r, e) => {
    let { $head: t, $anchor: n } = r.selection;
    return !t.parent.type.spec.code || !t.sameParent(n) ? false : (e && e(r.tr.insertText(`
`).scrollIntoView()), true);
  };
  function _r(r) {
    for (let e = 0; e < r.edgeCount; e++) {
      let { type: t } = r.edge(e);
      if (t.isTextblock && !t.hasRequiredAttrs()) return t;
    }
    return null;
  }
  const pu = (r, e) => {
    let { $head: t, $anchor: n } = r.selection;
    if (!t.parent.type.spec.code || !t.sameParent(n)) return false;
    let i = t.node(-1), s = t.indexAfter(-1), o = _r(i.contentMatchAt(s));
    if (!o || !i.canReplaceWith(s, s, o)) return false;
    if (e) {
      let l = t.after(), a = r.tr.replaceWith(l, l, o.createAndFill());
      a.setSelection(I.near(a.doc.resolve(l), 1)), e(a.scrollIntoView());
    }
    return true;
  }, mu = (r, e) => {
    let t = r.selection, { $from: n, $to: i } = t;
    if (t instanceof ne || n.parent.inlineContent || i.parent.inlineContent) return false;
    let s = _r(i.parent.contentMatchAt(i.indexAfter()));
    if (!s || !s.isTextblock) return false;
    if (e) {
      let o = (!n.parentOffset && i.index() < i.parent.childCount ? n : i).pos, l = r.tr.insert(o, s.createAndFill());
      l.setSelection(E.create(l.doc, o + 1)), e(l.scrollIntoView());
    }
    return true;
  }, gu = (r, e) => {
    let { $cursor: t } = r.selection;
    if (!t || t.parent.content.size) return false;
    if (t.depth > 1 && t.after() != t.end(-1)) {
      let s = t.before();
      if (Ht(r.doc, s)) return e && e(r.tr.split(s).scrollIntoView()), true;
    }
    let n = t.blockRange(), i = n && ir(n);
    return i == null ? false : (e && e(r.tr.lift(n, i).scrollIntoView()), true);
  };
  function _u(r) {
    return (e, t) => {
      let { $from: n, $to: i } = e.selection;
      if (e.selection instanceof O && e.selection.node.isBlock) return !n.parentOffset || !Ht(e.doc, n.pos) ? false : (t && t(e.tr.split(n.pos).scrollIntoView()), true);
      if (!n.depth) return false;
      let s = [], o, l, a = false, c = false;
      for (let m = n.depth; ; m--) if (n.node(m).isBlock) {
        a = n.end(m) == n.pos + (n.depth - m), c = n.start(m) == n.pos - (n.depth - m), l = _r(n.node(m - 1).contentMatchAt(n.indexAfter(m - 1))), s.unshift(a && l ? {
          type: l
        } : null), o = m;
        break;
      } else {
        if (m == 1) return false;
        s.unshift(null);
      }
      let f = e.tr;
      (e.selection instanceof E || e.selection instanceof ne) && f.deleteSelection();
      let d = f.mapping.map(n.pos), u = Ht(f.doc, d, s.length, s);
      if (u || (s[0] = l ? {
        type: l
      } : null, u = Ht(f.doc, d, s.length, s)), !u) return false;
      if (f.split(d, s.length, s), !a && c && n.node(o).type != l) {
        let m = f.mapping.map(n.before(o)), p = f.doc.resolve(m);
        l && n.node(o - 1).canReplaceWith(p.index(), p.index() + 1, l) && f.setNodeMarkup(f.mapping.map(n.before(o)), l);
      }
      return t && t(f.scrollIntoView()), true;
    };
  }
  const yu = _u(), bu = (r, e) => (e && e(r.tr.setSelection(new ne(r.doc))), true);
  function wu(r, e, t) {
    let n = e.nodeBefore, i = e.nodeAfter, s = e.index();
    return !n || !i || !n.type.compatibleContent(i.type) ? false : !n.content.size && e.parent.canReplace(s - 1, s) ? (t && t(r.tr.delete(e.pos - n.nodeSize, e.pos).scrollIntoView()), true) : !e.parent.canReplace(s, s + 1) || !(i.isTextblock || ss(r.doc, e.pos)) ? false : (t && t(r.tr.join(e.pos).scrollIntoView()), true);
  }
  function so(r, e, t, n) {
    let i = e.nodeBefore, s = e.nodeAfter, o, l, a = i.type.spec.isolating || s.type.spec.isolating;
    if (!a && wu(r, e, t)) return true;
    let c = !a && e.parent.canReplace(e.index(), e.index() + 1);
    if (c && (o = (l = i.contentMatchAt(i.childCount)).findWrapping(s.type)) && l.matchType(o[0] || s.type).validEnd) {
      if (t) {
        let m = e.pos + s.nodeSize, p = b.empty;
        for (let S = o.length - 1; S >= 0; S--) p = b.from(o[S].create(null, p));
        p = b.from(i.copy(p));
        let _ = r.tr.step(new te(e.pos - 1, m, e.pos, m, new w(p, 1, 0), o.length, true)), y = _.doc.resolve(m + 2 * o.length);
        y.nodeAfter && y.nodeAfter.type == i.type && ss(_.doc, y.pos) && _.join(y.pos), t(_.scrollIntoView());
      }
      return true;
    }
    let f = s.type.spec.isolating || n > 0 && a ? null : I.findFrom(e, 1), d = f && f.$from.blockRange(f.$to), u = d && ir(d);
    if (u != null && u >= e.depth) return t && t(r.tr.lift(d, u).scrollIntoView()), true;
    if (c && lt(s, "start", true) && lt(i, "end")) {
      let m = i, p = [];
      for (; p.push(m), !m.isTextblock; ) m = m.lastChild;
      let _ = s, y = 1;
      for (; !_.isTextblock; _ = _.firstChild) y++;
      if (m.canReplace(m.childCount, m.childCount, _.content)) {
        if (t) {
          let S = b.empty;
          for (let R = p.length - 1; R >= 0; R--) S = b.from(p[R].copy(S));
          let L = r.tr.step(new te(e.pos - p.length, e.pos + s.nodeSize, e.pos + y, e.pos + s.nodeSize - y, new w(S, p.length, 0), 0, true));
          t(L.scrollIntoView());
        }
        return true;
      }
    }
    return false;
  }
  function oo(r) {
    return function(e, t) {
      let n = e.selection, i = r < 0 ? n.$from : n.$to, s = i.depth;
      for (; i.node(s).isInline; ) {
        if (!s) return false;
        s--;
      }
      return i.node(s).isTextblock ? (t && t(e.tr.setSelection(E.create(e.doc, r < 0 ? i.start(s) : i.end(s)))), true) : false;
    };
  }
  const ku = oo(-1), xu = oo(1);
  function Su(r, e, t, n) {
    for (let i = 0; i < e.length; i++) {
      let { $from: s, $to: o } = e[i], l = s.depth == 0 ? r.inlineContent && r.type.allowsMarkType(t) : false;
      if (r.nodesBetween(s.pos, o.pos, (a, c) => {
        if (l) return false;
        l = a.inlineContent && a.type.allowsMarkType(t);
      }), l) return true;
    }
    return false;
  }
  function Si(r, e = null, t) {
    return function(n, i) {
      let { empty: s, $cursor: o, ranges: l } = n.selection;
      if (s && !o || !Su(n.doc, l, r)) return false;
      if (i) if (o) r.isInSet(n.storedMarks || o.marks()) ? i(n.tr.removeStoredMark(r)) : i(n.tr.addStoredMark(r.create(e)));
      else {
        let a, c = n.tr;
        a = !l.some((f) => n.doc.rangeHasMark(f.$from.pos, f.$to.pos, r));
        for (let f = 0; f < l.length; f++) {
          let { $from: d, $to: u } = l[f];
          if (!a) c.removeMark(d.pos, u.pos, r);
          else {
            let m = d.pos, p = u.pos, _ = d.nodeAfter, y = u.nodeBefore, S = _ && _.isText ? /^\s*/.exec(_.text)[0].length : 0, L = y && y.isText ? /\s*$/.exec(y.text)[0].length : 0;
            m + S < p && (m += S, p -= L), c.addMark(m, p, r.create(e));
          }
        }
        i(c.scrollIntoView());
      }
      return true;
    };
  }
  function yr(...r) {
    return function(e, t, n) {
      for (let i = 0; i < r.length; i++) if (r[i](e, t, n)) return true;
      return false;
    };
  }
  let vn = yr(no, au, cu), Ci = yr(no, du, uu);
  const be = {
    Enter: yr(hu, mu, gu, yu),
    "Mod-Enter": pu,
    Backspace: vn,
    "Mod-Backspace": vn,
    "Shift-Backspace": vn,
    Delete: Ci,
    "Mod-Delete": Ci,
    "Mod-a": bu
  }, lo = {
    "Ctrl-h": be.Backspace,
    "Alt-Backspace": be["Mod-Backspace"],
    "Ctrl-d": be.Delete,
    "Ctrl-Alt-Backspace": be["Mod-Delete"],
    "Alt-Delete": be["Mod-Delete"],
    "Alt-d": be["Mod-Delete"],
    "Ctrl-a": ku,
    "Ctrl-e": xu
  };
  for (let r in be) lo[r] = be[r];
  const Cu = typeof navigator < "u" ? /Mac|iP(hone|[oa]d)/.test(navigator.platform) : typeof os < "u" && os.platform ? os.platform() == "darwin" : false, Ou = Cu ? lo : be;
  function Oi(r, e, t) {
    const n = Vr(e.text, e.marks);
    let i = false;
    const s = Le.create({
      doc: n,
      schema: ae,
      plugins: [
        ou(),
        wi({
          "Mod-z": to,
          "Mod-y": Zn,
          "Mod-Shift-z": Zn,
          ...Object.fromEntries([
            "strong",
            "emph",
            "underline"
          ].map((l) => {
            const a = Xt(l), c = ae.marks[a];
            return [
              `Mod-${l === "strong" ? "b" : l === "emph" ? "i" : "u"}`,
              Si(c)
            ];
          }))
        }),
        wi(Ou)
      ]
    }), o = new Xs(r, {
      state: s,
      dispatchTransaction(l) {
        const a = o.state.applyTransaction(l);
        o.updateState(a.state), !(i || !l.docChanged) && t(Wn(a.state.doc));
      }
    });
    return {
      view: o,
      setContent(l) {
        i = true;
        const a = Vr(l.text, l.marks);
        o.updateState(Le.create({
          doc: a,
          schema: ae,
          plugins: o.state.plugins
        })), i = false;
      },
      toggleMark(l) {
        const a = Xt(l), c = ae.marks[a];
        c && (Si(c)(o.state, o.dispatch), o.focus());
      },
      focus() {
        o.focus();
      },
      setCursor(l) {
        i = true;
        const a = tf(o.state.doc, l);
        o.dispatch(o.state.tr.setSelection(E.create(o.state.doc, a))), i = false, o.focus();
      },
      destroy() {
        o.destroy();
      }
    };
  }
  function Mi(r, e) {
    const t = [
      ...r.querySelectorAll("[data-mark]")
    ], n = () => {
      var _a2;
      for (const i of t) {
        const s = i.getAttribute("data-mark"), o = Xt(s), a = (_a2 = ae.marks[o]) == null ? void 0 : _a2.isInSet(e.view.state.storedMarks || e.view.state.selection.$from.marks());
        i.classList.toggle("active", !!a);
      }
    };
    e.view.dom.addEventListener("focusin", n), e.view.dom.addEventListener("keyup", n), e.view.dom.addEventListener("mouseup", n);
    for (const i of t) i.addEventListener("click", () => {
      e.toggleMark(i.getAttribute("data-mark")), n();
    });
  }
  function er(r, e, t, n = {}) {
    var _a2;
    const i = n.densityScale ?? window.devicePixelRatio ?? 1, s = r.pageSize(e), l = (n.maxCssWidth ?? (((_a2 = t.parentElement) == null ? void 0 : _a2.clientWidth) ? Math.min(t.parentElement.clientWidth - 48, 720) : 612)) / s.widthPt, a = t.getContext("2d");
    if (!a) throw new Error("2d context unavailable");
    const c = r.paint(a, e, {
      layoutScale: l,
      densityScale: i
    });
    return t.style.width = `${c.layoutWidth}px`, t.style.height = `${c.layoutHeight}px`, {
      ...c,
      layoutScale: l,
      pageSize: s
    };
  }
  function Mu(r, e, t, n, i) {
    const s = n.querySelector("canvas");
    if (!s) return;
    const [o, l, a, c] = e.rect, f = n.getBoundingClientRect(), d = s.getBoundingClientRect(), u = o / t.widthPt * 100, m = (1 - c / t.heightPt) * 100, p = (a - o) / t.widthPt * 100, _ = (c - l) / t.heightPt * 100;
    r.hidden = false, i && (r.dataset.field = i), r.style.left = `${d.left - f.left + u / 100 * d.width}px`, r.style.top = `${d.top - f.top + m / 100 * d.height}px`, r.style.width = `${p / 100 * d.width}px`, r.style.height = `${_ / 100 * d.height}px`;
  }
  function ao(r, e) {
    return r.find((t) => t.field === e && t.page === 0);
  }
  function Nu(r, e, t, n) {
    const i = r.getBoundingClientRect(), s = t - i.left, o = n - i.top;
    return {
      x: s / i.width * e.widthPt,
      y: e.heightPt - o / i.height * e.heightPt
    };
  }
  function Eu(r, e, t, n) {
    const i = r.getBoundingClientRect();
    return {
      x: t / e.widthPt * i.width,
      y: (1 - n / e.heightPt) * i.height
    };
  }
  function Tu(r, e, t) {
    const [n, i, s, o] = t.rect, l = (n + s) / 2, a = (i + o) / 2;
    return Eu(r, e, l, a);
  }
  function Du(r, e, t, n) {
    r.addEventListener("click", (i) => {
      const s = e();
      if (!s) return;
      const o = s.pageSize(0), { x: l, y: a } = Nu(r, o, i.clientX, i.clientY), c = s.positionAt(0, l, a), f = (c == null ? void 0 : c.field) ?? s.fieldAt(0, l, a);
      !f || !t[f] || (n(f), c && typeof c.pos == "number" ? t[f].setCursor(c.pos) : t[f].focus());
    });
  }
  function Ni(r, e) {
    var _a2, _b;
    return (_b = (_a2 = r == null ? void 0 : r.payloadItems) == null ? void 0 : _a2.find((t) => t.type === "field" && t.key === e)) == null ? void 0 : _b.value;
  }
  const Ei = document.getElementById("status"), kt = document.getElementById("page-canvas"), Iu = document.querySelector(".preview-pane"), Rn = document.getElementById("region-highlight"), Au = document.getElementById("regions-dump"), vu = document.getElementById("body-editor"), Ru = document.getElementById("subject-editor"), Pu = document.getElementById("subject-toolbar"), Bu = document.getElementById("tag-line-editor"), zu = document.getElementById("tag-line-toolbar");
  window.__POC__ = {
    ready: false,
    error: null,
    get activeField() {
      return un;
    },
    regionClickPoint(r) {
      if (!X) return null;
      const e = ao(X.regions(), r);
      return e ? Tu(kt, X.pageSize(0), e) : null;
    }
  };
  let X = null, he = null;
  const U = {};
  let un = null, Ti = 0, Pn = false;
  function Ve(r, e = "") {
    Ei.textContent = r, Ei.className = `status ${e}`.trim();
  }
  function Fu(r) {
    const e = r.seedDocument();
    return e.setFields({
      memo_for: [
        "HQ USAF/A1"
      ],
      subject: "**Richtext** form POC",
      signature_block: [
        "JANE DOE, Col, USAF",
        "Director of Testing"
      ],
      letterhead_caption: [
        "HEADQUARTERS EXAMPLE WING"
      ],
      tag_line: "**Semper** *Supra*"
    }), e.replaceBody(`The first paragraph. Top-level paragraphs are auto-numbered; do not add manual numbering.

- Nested bullets are automatically lettered.`), e;
  }
  async function Vu(r) {
    if (!(!X || !he || !U.$body || !U.subject || !U.tag_line) && !Pn) {
      he.replaceBody(U.$body.getMarkdown()), he.setField("subject", qr(Wn(U.subject.view.state.doc))), he.setField("tag_line", qr(Wn(U.tag_line.view.state.doc))), Pn = true;
      try {
        const e = performance.now(), t = X.apply(he);
        er(X, 0, kt), ln();
        const n = Math.round(performance.now() - e);
        Ve(`Applied (${r}) \u2014 ${n}ms, dirty pages: [${t.dirtyPages.join(", ")}]`, "ok");
      } catch (e) {
        const t = e instanceof Error ? e.message : String(e);
        Ve(`Apply failed: ${t}`, "err"), console.error(e);
      } finally {
        Pn = false;
      }
    }
  }
  function Bn(r) {
    clearTimeout(Ti), Ti = window.setTimeout(() => Vu(r), 280);
  }
  function Jt(r) {
    un = r, ln();
  }
  function ln() {
    if (!X) return;
    const r = X.regions();
    Au.textContent = JSON.stringify(r, null, 2);
    const e = un ?? "subject", t = ao(r, e);
    t ? Mu(Rn, t, X.pageSize(0), Iu, e) : (Rn.hidden = true, delete Rn.dataset.field);
  }
  async function Lu() {
    try {
      Ve("Initializing WASM\u2026"), await mo(), Ve("Loading usaf_memo quill\u2026");
      const r = await rf(), e = rt.fromTree(r);
      he = Fu(e);
      const t = new Wa();
      Ve("Opening live session (first compile may take a few seconds)\u2026"), X = await t.open(e, he), er(X, 0, kt), U.$body = lf(vu, he.main.bodyMarkdown, () => Bn("$body")), U.$body.el.addEventListener("focusin", () => Jt("$body")), U.subject = Oi(Ru, Lr(Ni(he.main, "subject")), () => Bn("subject")), Mi(Pu, U.subject), U.subject.view.dom.addEventListener("focusin", () => Jt("subject")), U.tag_line = Oi(Bu, Lr(Ni(he.main, "tag_line")), () => Bn("tag_line")), Mi(zu, U.tag_line), U.tag_line.view.dom.addEventListener("focusin", () => Jt("tag_line")), un = "subject", ln(), Du(kt, () => X, U, Jt), window.addEventListener("resize", () => {
        X && (er(X, 0, kt), ln());
      }), Ve("Ready \u2014 edit fields or click the preview to cross-navigate.", "ok"), window.__POC__.ready = true;
    } catch (r) {
      const e = r instanceof Error ? r.message : String(r);
      Ve(`Boot failed: ${e}`, "err"), window.__POC__.error = e, console.error(r);
    }
  }
  Lu();
})();
export {
  po as _,
  __tla
};
