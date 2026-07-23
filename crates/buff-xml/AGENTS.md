# buff-xml

XML parsing for the Buff language. EXPERIMENTAL.

Pure-Rust MVP wrapping [`quick-xml`](https://docs.rs/quick-xml): streaming
XML parse into a simple DOM-like API (`XmlDocument` → `XmlElement` tree),
XPath-like queries, attribute/text access, and serialize-back. Shipped in
v1.18.0 (T50).

## STRUCTURE

```
src/
├── lib.rs        # XmlDocument / XmlElement types + parse/query/serialize.
└── error.rs      # XmlError enum (thiserror) + From for quick_xml::Error.
examples/
└── xml_parse_query.rs
tests/
└── core.rs
```

## PUBLIC API

```text
XmlDocument::from_str(xml) -> Result<XmlDocument, XmlError>
doc.root()            -> &XmlElement
doc.find(path)        -> Result<&XmlElement, XmlError>   // XPath-like
doc.to_string()       -> String

XmlElement:
  el.name()           -> &str
  el.attr(name)       -> Option<&str>
  el.text()           -> Option<&str>
  el.children()       -> &[XmlElement]
  el.to_string()      -> String
```

## WHERE TO LOOK

| Task | File |
|---|---|
| Change parse / DOM model / query / serialize | `src/lib.rs` |
| Change error variants / quick-xml mapping | `src/error.rs` |

## CONVENTIONS (this crate only)

- **No `unwrap`/`expect`/`panic!`/`todo!`/`unimplemented!`** in non-test
  code (project hard rule). `from_str` wraps its body in `catch_unwind`
  (FFI guide R6).
- **`XmlDocument` is `Send + Sync`** (owns its `Vec<XmlElement>` tree, no
  interior mutability). No public lifetime parameters / raw pointers
  (FFI guide R1/R4/R5).
- **Errors via `Result<T, XmlError>`**; `quick_xml::Error` mapped via `From`.
- **BTreeMap/BTreeSet only** where collections are used.

## INTEGRATION WITH BUFF LANGUAGE

`Xml` / `XmlDocument` / `XmlElement` are wired as prelude types in
`crates/buff-lang-types/src/prelude_types.rs` (T50 dedup of a
`PreludeInstanceFn::Find` collision with T43) and codegen-lowered in
`crates/buff-lang-codegen-rust/src/rust_codegen.rs`. Assoc/instance fns
resolve to `Type::Unknown` for MVP — full end-to-end `buff run` is
codegen-deferred (see `.sisyphus/decisions/api-compat-v20.md`).

## DEPS

All workspace-pinned: `quick-xml`. Dev: `insta`.

## REFERENCES

- Plan: `.sisyphus/plans/buff-v1x-frameworks.md` task T50.
- FFI guide: `crates/buff-lang-ffi-guide/GUIDE.md` (6 hard rules).
