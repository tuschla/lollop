//! The shells' `CoreBridge` (android/core-bridge) from the bridge's op table
//! (`shared::ffi::ops`): one interface method per op, and the implementation
//! over the two entry points both platforms provide (`call` for a byte
//! buffer, `callLines` for lines), each taking the op's code.

use std::fmt::Write;

use shared::ffi::ops::{Op, Param};

use crate::Output;

const PATH: &str = "android/core-bridge/src/commonMain/kotlin/app/milestone/core/CoreBridge.kt";

pub(crate) fn outputs() -> Vec<Output> {
    vec![Output {
        path: PATH,
        contents: core_bridge(),
    }]
}

/// The Kotlin method name: `parseFit` for `ParseFit`.
fn method(op: Op) -> String {
    let name = op.name();
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_ascii_lowercase().to_string() + chars.as_str()
    })
}

/// The Kotlin parameter list, and the entry point with its argument.
fn signature(op: Op) -> (String, String) {
    let code = op.code();
    match op.param() {
        Param::None => (String::new(), format!("call({code}, NO_INPUT)")),
        Param::Bytes(name) => (
            format!("{name}: ByteArray"),
            format!("call({code}, {name})"),
        ),
        Param::Lines(name) => (
            format!("{name}: Array<String>"),
            format!("callLines({code}, {name})"),
        ),
    }
}

fn kdoc(out: &mut String, indent: &str, doc: &str) {
    let _ = writeln!(out, "{indent}/**");
    for line in doc.lines() {
        let line = line.strip_prefix(' ').unwrap_or(line);
        if line.is_empty() {
            let _ = writeln!(out, "{indent} *");
        } else {
            let _ = writeln!(out, "{indent} * {line}");
        }
    }
    let _ = writeln!(out, "{indent} */");
}

fn core_bridge() -> String {
    let mut out = String::new();
    out.push_str(crate::HEADER);
    out.push_str("\n\npackage app.milestone.core\n\n");
    kdoc(
        &mut out,
        "",
        "The Rust core's operations (`shared::ffi::ops::Op`), one method each.\n\
         Every answer is the core's bytes: the op's payload, or the\n\
         `{\"error\":{\"kind\":…,\"context\":…,\"message\":…}}` envelope of its\n\
         failure; null only when the platform couldn't allocate it. The core\n\
         runs every op that reads it on its own thread, so any thread may call.",
    );
    out.push_str("interface CoreBridge {\n");
    for (i, &op) in Op::ALL.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        kdoc(&mut out, "    ", op.doc());
        let (params, _) = signature(op);
        let _ = writeln!(out, "    fun {}({params}): ByteArray?", method(op));
    }
    out.push_str("}\n\n");
    kdoc(
        &mut out,
        "",
        "[CoreBridge] over the core's two entry points, each taking an op's code:\n\
         Android's JNI (`JniCore`) and iOS's C ABI (`NativeCore`).",
    );
    out.push_str("abstract class CoreCalls : CoreBridge {\n");
    out.push_str("    /** Op [op]'s answer to [input]. */\n");
    out.push_str("    protected abstract fun call(op: Int, input: ByteArray): ByteArray?\n\n");
    out.push_str("    /** Op [op]'s answer to [lines]. */\n");
    out.push_str(
        "    protected abstract fun callLines(op: Int, lines: Array<String>): ByteArray?\n\n",
    );
    for &op in Op::ALL {
        let (params, body) = signature(op);
        let _ = writeln!(
            out,
            "    final override fun {}({params}): ByteArray? = {body}",
            method(op)
        );
    }
    out.push_str("\n    private companion object {\n");
    out.push_str("        val NO_INPUT = ByteArray(0)\n");
    out.push_str("    }\n}\n");
    out
}
