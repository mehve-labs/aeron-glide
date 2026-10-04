#!/usr/bin/env python3
"""Generate media driver context bindings from Aeron's aeronmd.h.

Writes src/driver_gen.h (C++ wrappers) and src/driver_gen.rs (cxx bridge,
`MediaDriverBuilder` setters, `MediaDriver` getters). The output is checked in;
rerun this script after an Aeron upgrade:

    python3 scripts/gen_driver_context.py [path/to/aeronmd.h]

Without an argument the header is taken from the most recent cargo build.
"""

import glob
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Setters hand-written in src/lib.rs (their getters are still generated).
EXCLUDE = {"threading_mode"}

# C parameter type -> (Rust parameter type, cxx bridge type, C++ parameter type, expression passed to C).
SCALARS = {
    "uint64_t": ("u64", "u64", "uint64_t", "{v}"),
    "int64_t": ("i64", "i64", "int64_t", "{v}"),
    "size_t": ("usize", "usize", "size_t", "{v}"),
    "bool": ("bool", "bool", "bool", "{v}"),
    "int32_t": ("i32", "i32", "int32_t", "{v}"),
    "uint32_t": ("u32", "u32", "uint32_t", "{v}"),
    "uint8_t": ("u8", "u8", "uint8_t", "{v}"),
    # Some C setters keep the pointer rather than copying it, so the wrapper owns every string.
    "const char *": ("&str", "&str", "rust::Str", "driver.keep({v})"),
    "aeron_thread_naming_t": ("ThreadNaming", "i32", "int32_t", "static_cast<aeron_thread_naming_t>({v})"),
    "aeron_inferable_boolean_t": ("InferableBoolean", "i32", "int32_t", "static_cast<aeron_inferable_boolean_t>({v})"),
}

# C return type -> (Rust return type, cxx bridge type, C++ return type, C++ expression, Rust conversion).
GETTERS = {
    "uint64_t": ("u64", "u64", "uint64_t", "{e}", "{e}"),
    "int64_t": ("i64", "i64", "int64_t", "{e}", "{e}"),
    "size_t": ("usize", "usize", "size_t", "{e}", "{e}"),
    "bool": ("bool", "bool", "bool", "{e}", "{e}"),
    "int32_t": ("i32", "i32", "int32_t", "{e}", "{e}"),
    "int": ("i32", "i32", "int32_t", "static_cast<int32_t>({e})", "{e}"),
    "uint32_t": ("u32", "u32", "uint32_t", "{e}", "{e}"),
    "uint8_t": ("u8", "u8", "uint8_t", "{e}", "{e}"),
    "const char *": ("String", "String", "rust::String", "lossyOrEmpty({e})", "{e}"),
    "aeron_thread_naming_t": ("ThreadNaming", "i32", "int32_t", "static_cast<int32_t>({e})", "ThreadNaming::from_c({e})"),
    "aeron_inferable_boolean_t": ("InferableBoolean", "i32", "int32_t", "static_cast<int32_t>({e})", "InferableBoolean::from_c({e})"),
    "aeron_threading_mode_t": ("ThreadingMode", "i32", "int32_t", "static_cast<int32_t>({e})", "ThreadingMode::from_c({e})"),
}

# Getters whose C return type differs from their setter's are returned as the setter's type.
CAST_TO_SETTER = {"uint64_t", "int64_t", "size_t", "bool", "int32_t", "int", "uint32_t", "uint8_t"}

DECL = re.compile(
    r"(?P<ret>[A-Za-z_][\w ]*?\s*\*?)\s*\baeron_driver_context_(?P<kind>set|get)_(?P<name>\w+)\s*\(\s*aeron_driver_context_t\s*\*\s*\w+\s*(?:,\s*(?P<args>[^)]*?))?\)\s*;",
    re.S,
)


# Getters named after their setter where Aeron's C names differ.
GETTER_RENAMES = {
    "resolver_bootstrap_resolution_interval_ns": "resolver_bootstrap_neighbor_resolution_interval_ns",
}
# Getters exposed as one Rust getter (matching the setter's Option<i64>).
COMBINED_GETTERS = {"receiver_group_tag_is_present", "receiver_group_tag_value"}

# Time settings whose default is Aeron's null value (u64::MAX here), with its meaning.
NULL_VALUE_DEFAULT = {
    "untethered_linger_timeout_ns": "`u64::MAX` (Aeron's null value, the default) uses the untethered window limit timeout.",
}


def env_ranges(context_c):
    """Ranges Aeron applies when it parses a setting from its environment
    variable (aeron_driver_context_init), keyed by variable name. Its setters
    don't check them. Only numeric bounds: others are internal macros."""
    pattern = re.compile(
        r"aeron_config_parse_\w+\(\s*(\w+)_ENV_VAR,\s*getenv\(\w+\),\s*(?:\([\w ]+\)\s*)?_context->\w+,"
        r"\s*([^,]+?),\s*([^)]+?)\);",
        re.S,
    )
    numeric = re.compile(r"^-?[\d\s*]+$|^INT(32|64)_(MAX|MIN)$")
    ranges = {}
    for env, low, high in pattern.findall(context_c):
        low, high = " ".join(low.split()), " ".join(high.split())
        ranges[env] = (low if numeric.match(low) else None, high if numeric.match(high) else None)
    return ranges


def find_header():
    if len(sys.argv) > 1:
        return Path(sys.argv[1])
    candidates = glob.glob(str(ROOT / "target/*/build/aeron-glide-*/out/aeron-*/aeron-driver/src/main/c/aeronmd.h"))
    if not candidates:
        sys.exit("aeronmd.h not found: run `cargo build` first or pass its path")
    return Path(max(candidates, key=os.path.getmtime))


def norm(t):
    t = re.sub(r"\s+", " ", t.strip())
    return t.replace(" *", " *").replace("char*", "char *")


def split_params(args):
    params = []
    for a in args.split(","):
        a = norm(a)
        m = re.match(r"(.*?)\s*\b(\w+)$", a)
        params.append((norm(m.group(1)), m.group(2)))
    return params


def doc_for(segment):
    """The last /** */ comment and ENV_VAR define in the text before a setter."""
    docs = re.findall(r"/\*\*(.*?)\*/", segment, re.S)
    env = re.findall(r'#define\s+\w+_ENV_VAR\s+"(\w+)"', segment)
    text = []
    if docs:
        for line in docs[-1].splitlines():
            line = re.sub(r"^\s*\*\s?", "", line).rstrip()
            if line and not line.startswith("@"):
                text.append(line)
    return text, (env[-1] if env else None)


def parse(header):
    source = header.read_text()
    setters, getters, skipped = [], {}, []
    last = 0
    for m in DECL.finditer(source):
        name, kind = m.group("name"), m.group("kind")
        if kind == "get":
            if m.group("args") is None:
                getters[name] = norm(m.group("ret"))
            else:
                skipped.append((name, [t for t, _ in split_params(m.group("args"))]))
            continue
        segment = source[last:m.start()]
        last = m.end()
        params = split_params(m.group("args") or "")
        doc, env = doc_for(segment)
        setters.append((name, params, doc, env))
    return setters, getters, skipped


def rust_doc(lines, indent="    "):
    return "".join(f"{indent}/// {l}\n" if l else f"{indent}///\n" for l in lines)


def generate(header):
    setters, getters, skipped = parse(header)
    h, bridge, builder, driver, not_generated = [], [], [], [], []
    setter_types = {n: p[0][0] for n, p, _, _ in setters if len(p) == 1}
    setter_envs = {n: e for n, _, _, e in setters}
    ranges = env_ranges((header.parent / "aeron_driver_context.c").read_text())
    for name, params, doc, env in setters:
        if name in EXCLUDE:
            continue
        types = [t for t, _ in params]
        refs = [f"`aeron_driver_context_set_{name}`"] + ([f"environment variable `{env}`"] if env else [])
        lines = (doc or [f"Sets `{name}`."]) + ["", "C: " + ", ".join(refs) + "."]
        cfn = f"driver_set_{name}"
        after = ""
        if name.endswith("_idle_strategy_init_args"):
            base = name[: -len("_init_args")]
            lines[-2:-2] = ["", f"Aeron loads the idle strategy when `{base}` is set, so `{base}` is reloaded here with these arguments; the order of the two settings does not matter."]
            after = (
                f"    std::string strategy = driver.effectiveStrategy(\"{base}\", \"{setter_envs[base]}\",\n"
                f"        aeron_driver_context_get_{base}(driver.context()));\n"
                f"    if (!strategy.empty() && aeron_driver_context_set_{base}(driver.context(), driver.keep(strategy)) < 0) {{\n"
                f'        throwDriverError("Failed to reload {base}");\n'
                f"    }}\n"
            )
        if name.endswith("_idle_strategy") and types == ["const char *"]:
            rparams, bparams, cparams = "strategy: DriverIdleStrategy", "value: &str", "rust::Str value"
            cargs, rcall = SCALARS["const char *"][3].format(v="value"), "strategy.as_str()"
            after = f"    driver.chooseStrategy(\"{name}\", std::string(value));\n"
        elif types == ["uint16_t", "uint16_t"]:
            rparams, bparams, cparams = "low: u16, high: u16", "low: u16, high: u16", "uint16_t low, uint16_t high"
            cargs, rcall = "low, high", "low, high"
        elif types == ["bool", "int64_t"]:
            rparams, bparams, cparams = "tag: Option<i64>", "is_present: bool, value: i64", "bool is_present, int64_t value"
            cargs, rcall = "is_present, value", "tag.is_some(), tag.unwrap_or(0)"
        elif len(types) == 1 and types[0] in SCALARS:
            rt, bt, ct, expr = SCALARS[types[0]]
            rparams, bparams, cparams = f"value: {rt}", f"value: {bt}", f"{ct} value"
            cargs = expr.format(v="value")
            rcall = "value as i32" if bt == "i32" and rt != "i32" else "value"
            if types[0] == "uint64_t" and name.endswith(("_ns", "_ms")):
                # The driver adds times to its clock in signed 64-bit arithmetic:
                # cap them like the client's timeouts so huge ones don't overflow.
                cap = "crate::MAX_TIMEOUT_NS" if name.endswith("_ns") else "(crate::MAX_TIMEOUT_NS / 1_000_000)"
                if name in NULL_VALUE_DEFAULT:
                    # u64::MAX is Aeron's null value here (-1 as int64_t): keep it.
                    rcall = f"if value == u64::MAX {{ value }} else {{ value.min({cap} as u64) }}"
                    lines[-2:-2] = ["", NULL_VALUE_DEFAULT[name] + " Other values above about 73 years are capped: the driver adds this to its clock."]
                else:
                    rcall = f"value.min({cap} as u64)"
                    lines[-2:-2] = ["", "Values above about 73 years are capped: the driver adds this to its clock."]
        else:
            not_generated.append(f"aeron_driver_context_set_{name}({', '.join(types)})")
            continue
        check = ""
        if env in ranges and len(types) == 1 and types[0] in SCALARS and types[0] not in ("bool", "const char *"):
            low, high = ranges[env]
            unsigned = types[0].startswith("uint") or types[0] == "size_t"
            conditions = []
            if low is not None and not (unsigned and low.lstrip().startswith(("0", "-"))):
                conditions.append(f"value < static_cast<{types[0]}>({low})")
            if high is not None and not (high == "INT64_MAX" and types[0] == "int64_t") and not (
                high == "INT32_MAX" and types[0] == "int32_t"
            ):
                conditions.append(f"static_cast<uint64_t>(value) > static_cast<uint64_t>({high})"
                                  if unsigned else f"value > {high}")
            if conditions:
                check = (
                    f"    if ({' || '.join(conditions)}) {{\n"
                    f'        throw aeron::util::IllegalArgumentException(\n'
                    f'            "{name} must be in [{low or "-"}, {high or "-"}] (as for {env}), got " + std::to_string(value), SOURCEINFO, EINVAL);\n'
                    f"    }}\n"
                )
                lines[-2:-2] = ["", f"Must be within the range Aeron accepts from `{env}`: from {low or 'any'} to {high or 'any'}; checked by `start`."]
        h.append(
            f"inline void {cfn}(MediaDriverWrapper &driver, {cparams}) {{\n"
            f"    driver.ensureNotStarted();\n{check}"
            f"    if (aeron_driver_context_set_{name}(driver.context(), {cargs}) < 0) {{\n"
            f'        throwDriverError("Failed to set {name}");\n'
            f"    }}\n{after}}}\n"
        )
        bridge.append(f"        fn {cfn}(driver: Pin<&mut MediaDriverWrapper>, {bparams}) -> Result<()>;\n")
        builder.append(
            rust_doc(lines)
            + f"    pub fn {name}(self, {rparams}) -> Self {{\n"
            f"        self.apply(|w| ffi::{cfn}(w, {rcall}))\n    }}\n"
        )
    for name, types in skipped:
        if types == ["uint16_t *", "uint16_t *"]:
            cfn = f"driver_get_{name}"
            h.append(
                f"inline rust::Vec<uint16_t> {cfn}(const MediaDriverWrapper &driver) {{\n"
                f"    uint16_t low = 0, high = 0;\n"
                f"    aeron_driver_context_get_{name}(driver.context(), &low, &high);\n"
                f"    rust::Vec<uint16_t> range;\n    range.push_back(low);\n    range.push_back(high);\n"
                f"    return range;\n}}\n"
            )
            bridge.append(f"        fn {cfn}(driver: &MediaDriverWrapper) -> Vec<u16>;\n")
            driver.append(
                f"    /// The driver's `{name}` setting as `(low, high)` (`aeron_driver_context_get_{name}`).\n"
                f"    pub fn {name}(&self) -> (u16, u16) {{\n"
                f"        let range = ffi::{cfn}(&self.inner);\n        (range[0], range[1])\n    }}\n"
            )
        else:
            not_generated.append(f"aeron_driver_context_get_{name}({', '.join(types)})")
    for name, ret in sorted(getters.items()):
        if ret not in GETTERS:
            not_generated.append(f"aeron_driver_context_get_{name} -> {ret}")
            continue
        rt, bt, ct, cexpr, rconv = GETTERS[ret]
        wanted = setter_types.get(name)
        if wanted and wanted != ret and ret in CAST_TO_SETTER and wanted in CAST_TO_SETTER:
            rt, bt, ct, _, rconv = GETTERS[wanted]
            cexpr = "{e} != 0" if wanted == "bool" else f"static_cast<{ct}>({{e}})"
        cfn = f"driver_get_{name}"
        rust_name = GETTER_RENAMES.get(name, name)
        h.append(
            f"inline {ct} {cfn}(const MediaDriverWrapper &driver) {{\n"
            f"    return {cexpr.format(e=f'aeron_driver_context_get_{name}(driver.context())')};\n}}\n"
        )
        bridge.append(f"        fn {cfn}(driver: &MediaDriverWrapper) -> {bt};\n")
        if name in COMBINED_GETTERS:
            continue  # bridged, but exposed through the combined getter below
        note = (
            "    ///\n    /// Aeron's recorded name: a strategy chosen through its environment\n"
            "    /// variable still reads as the default here.\n"
            if name.endswith("_idle_strategy") else ""
        )
        driver.append(
            f"    /// The driver's `{rust_name}` setting (`aeron_driver_context_get_{name}`).\n" + note +
            f"    pub fn {rust_name}(&self) -> {rt} {{\n"
            f"        {rconv.format(e=f'ffi::{cfn}(&self.inner)')}\n    }}\n"
        )
    driver.append(
        "    /// The driver's `receiver_group_tag` setting, `None` if unset\n"
        "    /// (`aeron_driver_context_get_receiver_group_tag_is_present` / `_value`).\n"
        "    pub fn receiver_group_tag(&self) -> Option<i64> {\n"
        "        ffi::driver_get_receiver_group_tag_is_present(&self.inner)\n"
        "            .then(|| ffi::driver_get_receiver_group_tag_value(&self.inner))\n    }\n"
    )
    return h, bridge, builder, driver, not_generated


HEADER_PROLOGUE = """// @generated by scripts/gen_driver_context.py from {src}. Do not edit.
#pragma once
#include "driver_shim.h"

extern "C" {{
#include <aeronmd.h>
}}

namespace aeron_rs {{

inline rust::String lossyOrEmpty(const char *value) {{
    return rust::String::lossy(value == nullptr ? "" : value);
}}

"""

RUST_PROLOGUE = '''// @generated by scripts/gen_driver_context.py from {src}. Do not edit.
//! Media driver settings (`aeron_driver_context_set_*` / `get_*`).
//!
//! Not generated (function pointers or driver-internal structs):
{not_generated}
#![allow(clippy::too_many_arguments)]

use crate::{{DriverIdleStrategy, MediaDriver, MediaDriverBuilder, ThreadingMode}};

#[cxx::bridge(namespace = "aeron_rs")]
pub(crate) mod ffi {{
    unsafe extern "C++" {{
        include!("driver_gen.h");

        type MediaDriverWrapper = crate::driver::ffi::MediaDriverWrapper;

{bridge}    }}
}}

/// How the media driver names its threads (`aeron_thread_naming_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ThreadNaming {{
    /// `AERON_THREAD_NAMING_CLASSIC`.
    Classic = 0,
    /// `AERON_THREAD_NAMING_NEW`.
    New = 1,
}}

impl ThreadNaming {{
    fn from_c(value: i32) -> Self {{
        if value == 1 {{ Self::New }} else {{ Self::Classic }}
    }}
}}

/// A boolean setting that can be forced or inferred (`aeron_inferable_boolean_t`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InferableBoolean {{
    /// `AERON_FORCE_FALSE`.
    ForceFalse = 0,
    /// `AERON_FORCE_TRUE`.
    ForceTrue = 1,
    /// `AERON_INFER`.
    Infer = 2,
}}

impl InferableBoolean {{
    fn from_c(value: i32) -> Self {{
        match value {{
            0 => Self::ForceFalse,
            1 => Self::ForceTrue,
            _ => Self::Infer,
        }}
    }}
}}

impl MediaDriverBuilder {{
{builder}}}

impl MediaDriver {{
{driver}}}
'''


def main():
    header = find_header()
    src = "aeron-driver/src/main/c/aeronmd.h (" + header.parts[header.parts.index("out") + 1] + ")" if "out" in header.parts else header.name
    h, bridge, builder, driver, not_generated = generate(header)
    (ROOT / "src/driver_gen.h").write_text(
        HEADER_PROLOGUE.format(src=src) + "\n".join(h) + "\n} // namespace aeron_rs\n"
    )
    ng = "\n".join(f"//! - `{n}`" for n in not_generated)
    (ROOT / "src/driver_gen.rs").write_text(
        RUST_PROLOGUE.format(src=src, not_generated=ng, bridge="".join(bridge), builder="\n".join(builder), driver="\n".join(driver))
    )
    subprocess.run(["rustfmt", "--edition", "2024", str(ROOT / "src/driver_gen.rs")], check=True)
    print(f"{len(builder)} setters, {len(driver)} getters, {len(not_generated)} not generated", file=sys.stderr)


if __name__ == "__main__":
    main()
