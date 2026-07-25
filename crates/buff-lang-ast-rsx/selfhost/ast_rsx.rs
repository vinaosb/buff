#[derive(Clone, PartialEq, Hash, Debug)]
pub struct Span {
    pub start: i64,
    pub end: i64,
    pub source_id: i64,
}
fn span_new(start: i64, end: i64, source_id: i64) -> Span {
    return Span {
        start: start,
        end: end,
        source_id: source_id,
    }
}
fn span_dummy() -> Span {
    return Span {
        start: 0,
        end: 0,
        source_id: 0,
    }
}
#[derive(Clone, PartialEq, Debug)]
pub struct RsxTemplateFile {
    pub script: Option<ScriptBlock>,
    pub root: Vector<RsxNode>,
    pub span: Span,
}
fn rsx_template_file_new(
    script: Option<ScriptBlock>,
    root: Vector<RsxNode>,
    span: Span,
) -> RsxTemplateFile {
    return RsxTemplateFile {
        script: script,
        root: root,
        span: span,
    }
}
#[derive(Clone, PartialEq, Debug)]
pub struct ScriptBlock {
    pub lang: String,
    pub props: Option<String>,
    pub source: String,
    pub span: Span,
}
fn script_block_new(lang: String, source: String, span: Span) -> ScriptBlock {
    return ScriptBlock {
        lang: lang,
        props: None,
        source: source,
        span: span,
    }
}
fn script_block_with_props(
    lang: String,
    props: String,
    source: String,
    span: Span,
) -> ScriptBlock {
    return ScriptBlock {
        lang: lang,
        props: Some(props),
        source: source,
        span: span,
    }
}
#[derive(Clone, PartialEq, Debug)]
pub enum RsxNode {
    Element(RsxElement),
    Fragment(RsxFragment),
    Text(RsxText),
    Interp(RsxInterp),
    If(RsxIf),
    Each(RsxEach),
    Slot(RsxSlot),
    Comment(RsxComment),
    Script(ScriptBlock),
    RawHtml(RsxRawHtml),
    Await(RsxAwait),
}
#[derive(Clone, PartialEq, Debug)]
pub struct RsxElement {
    pub tag: String,
    pub is_component: bool,
    pub attributes: Vector<RsxAttribute>,
    pub children: Vector<RsxNode>,
    pub self_closing: bool,
    pub span: Span,
}
fn rsx_element_new(
    tag: String,
    is_component: bool,
    attributes: Vector<RsxAttribute>,
    children: Vector<RsxNode>,
    self_closing: bool,
    span: Span,
) -> RsxElement {
    return RsxElement {
        tag: tag,
        is_component: is_component,
        attributes: attributes,
        children: children,
        self_closing: self_closing,
        span: span,
    }
}
#[derive(Clone, PartialEq, Debug)]
pub struct RsxFragment {
    pub children: Vector<RsxNode>,
    pub span: Span,
}
fn rsx_fragment_new(children: Vector<RsxNode>, span: Span) -> RsxFragment {
    return RsxFragment {
        children: children,
        span: span,
    }
}
#[derive(Clone, PartialEq, Hash, Debug)]
pub struct RsxText {
    pub text: String,
    pub span: Span,
}
fn rsx_text_new(text: String, span: Span) -> RsxText {
    return RsxText { text: text, span: span };
}
#[derive(Clone, PartialEq, Hash, Debug)]
pub struct RsxInterp {
    pub expr: String,
    pub span: Span,
}
fn rsx_interp_new(expr: String, span: Span) -> RsxInterp {
    return RsxInterp {
        expr: expr,
        span: span,
    }
}
#[derive(Clone, PartialEq, Debug)]
pub struct RsxAttribute {
    pub kind: RsxAttributeKind,
    pub span: Span,
}
fn rsx_attribute_new(kind: RsxAttributeKind, span: Span) -> RsxAttribute {
    return RsxAttribute {
        kind: kind,
        span: span,
    }
}
#[derive(Clone, PartialEq, Debug)]
pub enum RsxAttributeKind {
    Literal(String, String),
    Expression(String, String, Span),
    Event(String, Vector<String>, String, Span),
    NamedProp(String, String, Span),
    Boolean(String),
    Spread(String),
    Bind(String, String),
}
#[derive(Clone, PartialEq, Debug)]
pub struct RsxIf {
    pub branches: Vector<RsxIfBranch>,
    pub else_branch: Option<Vector<RsxNode>>,
    pub span: Span,
}
fn rsx_if_new(
    branches: Vector<RsxIfBranch>,
    else_branch: Option<Vector<RsxNode>>,
    span: Span,
) -> RsxIf {
    return RsxIf {
        branches: branches,
        else_branch: else_branch,
        span: span,
    }
}
#[derive(Clone, PartialEq, Debug)]
pub struct RsxIfBranch {
    pub cond: String,
    pub cond_span: Span,
    pub body: Vector<RsxNode>,
}
fn rsx_if_branch_new(
    cond: String,
    cond_span: Span,
    body: Vector<RsxNode>,
) -> RsxIfBranch {
    return RsxIfBranch {
        cond: cond,
        cond_span: cond_span,
        body: body,
    }
}
#[derive(Clone, PartialEq, Debug)]
pub struct RsxEach {
    pub iterable: String,
    pub iterable_span: Span,
    pub binding: String,
    pub index_binding: Option<String>,
    pub key: Option<String>,
    pub body: Vector<RsxNode>,
    pub else_branch: Option<Vector<RsxNode>>,
    pub span: Span,
}
fn rsx_each_new(
    iterable: String,
    iterable_span: Span,
    binding: String,
    index_binding: Option<String>,
    key: Option<String>,
    body: Vector<RsxNode>,
    else_branch: Option<Vector<RsxNode>>,
    span: Span,
) -> RsxEach {
    return RsxEach {
        iterable: iterable,
        iterable_span: iterable_span,
        binding: binding,
        index_binding: index_binding,
        key: key,
        body: body,
        else_branch: else_branch,
        span: span,
    }
}
#[derive(Clone, PartialEq, Debug)]
pub struct RsxSlot {
    pub name: Option<String>,
    pub span: Span,
}
fn rsx_slot_new(span: Span) -> RsxSlot {
    return RsxSlot { name: None, span: span };
}
fn rsx_slot_named(name: String, span: Span) -> RsxSlot {
    return RsxSlot {
        name: Some(name),
        span: span,
    }
}
#[derive(Clone, PartialEq, Hash, Debug)]
pub struct RsxComment {
    pub text: String,
    pub span: Span,
}
fn rsx_comment_new(text: String, span: Span) -> RsxComment {
    return RsxComment {
        text: text,
        span: span,
    }
}
#[derive(Clone, PartialEq, Hash, Debug)]
pub struct RsxRawHtml {
    pub expr: String,
    pub span: Span,
}
fn rsx_raw_html_new(expr: String, span: Span) -> RsxRawHtml {
    return RsxRawHtml {
        expr: expr,
        span: span,
    }
}
#[derive(Clone, PartialEq, Debug)]
pub struct RsxAwait {
    pub fut_expr: String,
    pub fut_span: Span,
    pub pending_body: Option<Vector<RsxNode>>,
    pub then_binding: String,
    pub then_body: Vector<RsxNode>,
    pub catch_binding: Option<String>,
    pub catch_body: Option<Vector<RsxNode>>,
    pub span: Span,
}
fn rsx_await_new(
    fut_expr: String,
    fut_span: Span,
    pending_body: Option<Vector<RsxNode>>,
    then_binding: String,
    then_body: Vector<RsxNode>,
    catch_binding: Option<String>,
    catch_body: Option<Vector<RsxNode>>,
    span: Span,
) -> RsxAwait {
    return RsxAwait {
        fut_expr: fut_expr,
        fut_span: fut_span,
        pending_body: pending_body,
        then_binding: then_binding,
        then_body: then_body,
        catch_binding: catch_binding,
        catch_body: catch_body,
        span: span,
    }
}
fn is_component_tag(tag: String) -> bool {
    if tag.starts_with("A".to_string()) {
        return true;
    }
    if tag.clone().starts_with("B".to_string()) {
        return true;
    }
    if tag.clone().starts_with("C".to_string()) {
        return true;
    }
    if tag.clone().starts_with("D".to_string()) {
        return true;
    }
    if tag.clone().starts_with("E".to_string()) {
        return true;
    }
    if tag.clone().starts_with("F".to_string()) {
        return true;
    }
    if tag.clone().starts_with("G".to_string()) {
        return true;
    }
    if tag.clone().starts_with("H".to_string()) {
        return true;
    }
    if tag.clone().starts_with("I".to_string()) {
        return true;
    }
    if tag.clone().starts_with("J".to_string()) {
        return true;
    }
    if tag.clone().starts_with("K".to_string()) {
        return true;
    }
    if tag.clone().starts_with("L".to_string()) {
        return true;
    }
    if tag.clone().starts_with("M".to_string()) {
        return true;
    }
    if tag.clone().starts_with("N".to_string()) {
        return true;
    }
    if tag.clone().starts_with("O".to_string()) {
        return true;
    }
    if tag.clone().starts_with("P".to_string()) {
        return true;
    }
    if tag.clone().starts_with("Q".to_string()) {
        return true;
    }
    if tag.clone().starts_with("R".to_string()) {
        return true;
    }
    if tag.clone().starts_with("S".to_string()) {
        return true;
    }
    if tag.clone().starts_with("T".to_string()) {
        return true;
    }
    if tag.clone().starts_with("U".to_string()) {
        return true;
    }
    if tag.clone().starts_with("V".to_string()) {
        return true;
    }
    if tag.clone().starts_with("W".to_string()) {
        return true;
    }
    if tag.clone().starts_with("X".to_string()) {
        return true;
    }
    if tag.clone().starts_with("Y".to_string()) {
        return true;
    }
    if tag.clone().starts_with("Z".to_string()) {
        return true;
    }
    return false;
}
fn main() {
    let sp = span_new(10, 20, 1);
    println!("{}", sp.start);
    println!("{}", sp.clone().end);
    println!("{}", sp.clone().source_id);
    let dummy = span_dummy();
    println!("{}", dummy.start);
    let txt = rsx_text_new("hello".to_string(), sp.clone());
    println!("{}", txt.text);
    println!("{}", txt.clone().span.start);
    let interp = rsx_interp_new("user.name".to_string(), sp.clone());
    println!("{}", interp.expr);
    let cmt = rsx_comment_new("visible only in source".to_string(), sp.clone());
    println!("{}", cmt.text);
    let raw = rsx_raw_html_new("trusted_html".to_string(), sp.clone());
    println!("{}", raw.expr);
    let slot_default = rsx_slot_new(sp.clone());
    println!("{}", slot_default.name.is_none);
    let slot_named = rsx_slot_named("header".to_string(), sp.clone());
    match slot_named.name {
        Some(n) => println!("{}", n),
        None => println!("{}", 0),
    };
    let elem = rsx_element_new(
        "Counter".to_string(),
        true,
        vec![],
        vec![],
        true,
        sp.clone(),
    );
    println!("{}", elem.tag);
    println!("{}", elem.clone().is_component);
    println!("{}", elem.clone().self_closing);
    println!("{}", elem.clone().attributes.len());
    println!("{}", elem.clone().children.len());
    let frag = rsx_fragment_new(vec![], sp.clone());
    println!("{}", frag.children.len());
    let script = script_block_new(
        "buff".to_string(),
        "print(\\\"hi\\\")".to_string(),
        sp.clone(),
    );
    println!("{}", script.lang);
    println!("{}", script.clone().source);
    println!("{}", script.clone().props.is_none);
    let script_p = script_block_with_props(
        "buff".to_string(),
        "Props".to_string(),
        "x = 1".to_string(),
        sp.clone(),
    );
    println!("{}", script_p.lang);
    match script_p.clone().props {
        Some(p) => println!("{}", p),
        None => println!("{}", 0),
    };
    let attr = rsx_attribute_new(
        RsxAttributeKind::Boolean("disabled".to_string()),
        sp.clone(),
    );
    println!("{}", attr.span.start);
    let branch = rsx_if_branch_new("count > 0".to_string(), sp.clone(), vec![]);
    println!("{}", branch.cond);
    let if_node = rsx_if_new(vec![branch.clone()], None, sp.clone());
    println!("{}", if_node.branches.len());
    println!("{}", if_node.clone().else_branch.is_none);
    let each_node = rsx_each_new(
        "items".to_string(),
        sp.clone(),
        "item".to_string(),
        None.clone(),
        None.clone(),
        vec![],
        None.clone(),
        sp.clone(),
    );
    println!("{}", each_node.iterable);
    println!("{}", each_node.clone().binding);
    println!("{}", each_node.clone().key.is_none);
    let await_node = rsx_await_new(
        "fetchUser(id)".to_string(),
        sp.clone(),
        None.clone(),
        "user".to_string(),
        vec![],
        None.clone(),
        None.clone(),
        sp.clone(),
    );
    println!("{}", await_node.fut_expr);
    println!("{}", await_node.clone().then_binding);
    println!("{}", await_node.clone().catch_binding.is_none);
    let tpl = rsx_template_file_new(None.clone(), vec![], sp.clone());
    println!("{}", tpl.script.is_none);
    println!("{}", tpl.clone().root.len());
    println!("{}", is_component_tag("Counter".to_string()));
    println!("{}", is_component_tag("Layout".to_string()));
    println!("{}", is_component_tag("div".to_string()));
    println!("{}", is_component_tag("h1".to_string()));
    println!("{}", is_component_tag("".to_string()));
    {
        if let Ok(__buff_contents) = std::fs::read_to_string(".env") {
            for __buff_line in __buff_contents.lines() {
                let __buff_line = __buff_line.trim();
                if __buff_line.is_empty() || __buff_line.starts_with('#') {
                    continue;
                }
                if let Some((__buff_key, __buff_val)) = __buff_line.split_once('=') {
                    let __buff_k = __buff_key.trim().to_string();
                    let __buff_v = __buff_val.trim().to_string();
                    if !__buff_k.is_empty() && std::env::var(&__buff_k).is_err() {
                        unsafe {
                            std::env::set_var(&__buff_k, &__buff_v);
                        }
                    }
                }
            }
        }
    }
}
impl Span {
    pub fn copy_start(&self, start: i64) -> Self {
        let mut c = self.clone();
        c.start = start;
        c
    }
    pub fn copy_end(&self, end: i64) -> Self {
        let mut c = self.clone();
        c.end = end;
        c
    }
    pub fn copy_source_id(&self, source_id: i64) -> Self {
        let mut c = self.clone();
        c.source_id = source_id;
        c
    }
}
impl RsxTemplateFile {
    pub fn copy_script(&self, script: Option<ScriptBlock>) -> Self {
        let mut c = self.clone();
        c.script = script;
        c
    }
    pub fn copy_root(&self, root: Vector<RsxNode>) -> Self {
        let mut c = self.clone();
        c.root = root;
        c
    }
    pub fn copy_span(&self, span: Span) -> Self {
        let mut c = self.clone();
        c.span = span;
        c
    }
}
impl ScriptBlock {
    pub fn copy_lang(&self, lang: String) -> Self {
        let mut c = self.clone();
        c.lang = lang;
        c
    }
    pub fn copy_props(&self, props: Option<String>) -> Self {
        let mut c = self.clone();
        c.props = props;
        c
    }
    pub fn copy_source(&self, source: String) -> Self {
        let mut c = self.clone();
        c.source = source;
        c
    }
    pub fn copy_span(&self, span: Span) -> Self {
        let mut c = self.clone();
        c.span = span;
        c
    }
}
impl RsxElement {
    pub fn copy_tag(&self, tag: String) -> Self {
        let mut c = self.clone();
        c.tag = tag;
        c
    }
    pub fn copy_is_component(&self, is_component: bool) -> Self {
        let mut c = self.clone();
        c.is_component = is_component;
        c
    }
    pub fn copy_attributes(&self, attributes: Vector<RsxAttribute>) -> Self {
        let mut c = self.clone();
        c.attributes = attributes;
        c
    }
    pub fn copy_children(&self, children: Vector<RsxNode>) -> Self {
        let mut c = self.clone();
        c.children = children;
        c
    }
    pub fn copy_self_closing(&self, self_closing: bool) -> Self {
        let mut c = self.clone();
        c.self_closing = self_closing;
        c
    }
    pub fn copy_span(&self, span: Span) -> Self {
        let mut c = self.clone();
        c.span = span;
        c
    }
}
impl RsxFragment {
    pub fn copy_children(&self, children: Vector<RsxNode>) -> Self {
        let mut c = self.clone();
        c.children = children;
        c
    }
    pub fn copy_span(&self, span: Span) -> Self {
        let mut c = self.clone();
        c.span = span;
        c
    }
}
impl RsxText {
    pub fn copy_text(&self, text: String) -> Self {
        let mut c = self.clone();
        c.text = text;
        c
    }
    pub fn copy_span(&self, span: Span) -> Self {
        let mut c = self.clone();
        c.span = span;
        c
    }
}
impl RsxInterp {
    pub fn copy_expr(&self, expr: String) -> Self {
        let mut c = self.clone();
        c.expr = expr;
        c
    }
    pub fn copy_span(&self, span: Span) -> Self {
        let mut c = self.clone();
        c.span = span;
        c
    }
}
impl RsxAttribute {
    pub fn copy_kind(&self, kind: RsxAttributeKind) -> Self {
        let mut c = self.clone();
        c.kind = kind;
        c
    }
    pub fn copy_span(&self, span: Span) -> Self {
        let mut c = self.clone();
        c.span = span;
        c
    }
}
impl RsxIf {
    pub fn copy_branches(&self, branches: Vector<RsxIfBranch>) -> Self {
        let mut c = self.clone();
        c.branches = branches;
        c
    }
    pub fn copy_else_branch(&self, else_branch: Option<Vector<RsxNode>>) -> Self {
        let mut c = self.clone();
        c.else_branch = else_branch;
        c
    }
    pub fn copy_span(&self, span: Span) -> Self {
        let mut c = self.clone();
        c.span = span;
        c
    }
}
impl RsxIfBranch {
    pub fn copy_cond(&self, cond: String) -> Self {
        let mut c = self.clone();
        c.cond = cond;
        c
    }
    pub fn copy_cond_span(&self, cond_span: Span) -> Self {
        let mut c = self.clone();
        c.cond_span = cond_span;
        c
    }
    pub fn copy_body(&self, body: Vector<RsxNode>) -> Self {
        let mut c = self.clone();
        c.body = body;
        c
    }
}
impl RsxEach {
    pub fn copy_iterable(&self, iterable: String) -> Self {
        let mut c = self.clone();
        c.iterable = iterable;
        c
    }
    pub fn copy_iterable_span(&self, iterable_span: Span) -> Self {
        let mut c = self.clone();
        c.iterable_span = iterable_span;
        c
    }
    pub fn copy_binding(&self, binding: String) -> Self {
        let mut c = self.clone();
        c.binding = binding;
        c
    }
    pub fn copy_index_binding(&self, index_binding: Option<String>) -> Self {
        let mut c = self.clone();
        c.index_binding = index_binding;
        c
    }
    pub fn copy_key(&self, key: Option<String>) -> Self {
        let mut c = self.clone();
        c.key = key;
        c
    }
    pub fn copy_body(&self, body: Vector<RsxNode>) -> Self {
        let mut c = self.clone();
        c.body = body;
        c
    }
    pub fn copy_else_branch(&self, else_branch: Option<Vector<RsxNode>>) -> Self {
        let mut c = self.clone();
        c.else_branch = else_branch;
        c
    }
    pub fn copy_span(&self, span: Span) -> Self {
        let mut c = self.clone();
        c.span = span;
        c
    }
}
impl RsxSlot {
    pub fn copy_name(&self, name: Option<String>) -> Self {
        let mut c = self.clone();
        c.name = name;
        c
    }
    pub fn copy_span(&self, span: Span) -> Self {
        let mut c = self.clone();
        c.span = span;
        c
    }
}
impl RsxComment {
    pub fn copy_text(&self, text: String) -> Self {
        let mut c = self.clone();
        c.text = text;
        c
    }
    pub fn copy_span(&self, span: Span) -> Self {
        let mut c = self.clone();
        c.span = span;
        c
    }
}
impl RsxRawHtml {
    pub fn copy_expr(&self, expr: String) -> Self {
        let mut c = self.clone();
        c.expr = expr;
        c
    }
    pub fn copy_span(&self, span: Span) -> Self {
        let mut c = self.clone();
        c.span = span;
        c
    }
}
impl RsxAwait {
    pub fn copy_fut_expr(&self, fut_expr: String) -> Self {
        let mut c = self.clone();
        c.fut_expr = fut_expr;
        c
    }
    pub fn copy_fut_span(&self, fut_span: Span) -> Self {
        let mut c = self.clone();
        c.fut_span = fut_span;
        c
    }
    pub fn copy_pending_body(&self, pending_body: Option<Vector<RsxNode>>) -> Self {
        let mut c = self.clone();
        c.pending_body = pending_body;
        c
    }
    pub fn copy_then_binding(&self, then_binding: String) -> Self {
        let mut c = self.clone();
        c.then_binding = then_binding;
        c
    }
    pub fn copy_then_body(&self, then_body: Vector<RsxNode>) -> Self {
        let mut c = self.clone();
        c.then_body = then_body;
        c
    }
    pub fn copy_catch_binding(&self, catch_binding: Option<String>) -> Self {
        let mut c = self.clone();
        c.catch_binding = catch_binding;
        c
    }
    pub fn copy_catch_body(&self, catch_body: Option<Vector<RsxNode>>) -> Self {
        let mut c = self.clone();
        c.catch_body = catch_body;
        c
    }
    pub fn copy_span(&self, span: Span) -> Self {
        let mut c = self.clone();
        c.span = span;
        c
    }
}
