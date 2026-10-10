//! ITER-56C: DSP Window lowering - generated-source contract. The
//! constructors are infallible in buff-dsp (return Self, no Result), so
//! unlike the Tensor arms there is NO unwrap_or_default fallback; the
//! Buff Int arg casts to usize.

use buff_lang_codegen_rust::generate_rust;
use buff_lang_error::SourceId;
use buff_lang_lexer::tokenize;
use buff_lang_parser::parse;

fn gen(src: &str) -> String {
    let toks = tokenize(src, SourceId(0)).expect("lexer");
    let decls = parse(&toks, SourceId(0)).expect("parse");
    generate_rust(&decls).expect("codegen")
}

#[test]
fn window_constructors_lower() {
    let rust = gen(
        "func main():\n    let a = Window.hann(8)\n    let b = Window.hamming(8)\n    let c = Window.blackman(8)\n",
    );
    assert!(
        rust.contains("buff_dsp::Window::hann"),
        "hann ctor:\n{rust}"
    );
    assert!(
        rust.contains("buff_dsp::Window::hamming"),
        "hamming ctor:\n{rust}"
    );
    assert!(
        rust.contains("buff_dsp::Window::blackman"),
        "blackman ctor:\n{rust}"
    );
    assert!(
        rust.contains(" as usize"),
        "Buff Int arg must cast to the usize API:\n{rust}"
    );
    assert!(
        !rust.contains("unwrap_or_default"),
        "infallible constructors need no fallback:\n{rust}"
    );
}
