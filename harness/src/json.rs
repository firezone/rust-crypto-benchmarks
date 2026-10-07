use std::fmt::Write;

use crate::measure::Config;
use crate::meta::Meta;
use crate::{Manifest, Measurement};

pub const SCHEMA_VERSION: u32 = 1;

pub fn report(
    library: &str,
    manifest: &Manifest,
    meta: &Meta,
    config: &Config,
    measurements: &[Measurement],
) -> String {
    let mut s = String::new();
    s.push_str("{\n");
    let _ = writeln!(s, "  \"schema\": {SCHEMA_VERSION},");
    let _ = writeln!(s, "  \"impl\": {},", string(manifest.name()));
    let _ = writeln!(s, "  \"library\": {},", string(library));

    let crates = direct_dependencies(manifest)
        .into_iter()
        .map(|(name, version)| {
            format!(
                "{{ \"name\": {}, \"version\": {} }}",
                string(&name),
                string(&version)
            )
        })
        .collect::<Vec<_>>();
    let _ = writeln!(s, "  \"crates\": [{}],", crates.join(", "));

    let features = meta
        .cpu_features
        .iter()
        .map(|f| string(f))
        .collect::<Vec<_>>();
    s.push_str("  \"meta\": {\n");
    let _ = writeln!(s, "    \"arch\": {},", string(meta.arch));
    let _ = writeln!(s, "    \"os\": {},", string(meta.os));
    let _ = writeln!(s, "    \"cpu\": {},", string(&meta.cpu));
    let _ = writeln!(s, "    \"cpu_features\": [{}],", features.join(", "));
    let _ = writeln!(s, "    \"rustc\": {},", string(meta.rustc));
    let _ = writeln!(s, "    \"date\": {},", string(&meta.date));
    let _ = writeln!(s, "    \"commit\": {},", string(&meta.commit));
    let _ = writeln!(s, "    \"quick\": {},", meta.quick);
    let _ = writeln!(
        s,
        "    \"samples\": {}, \"sample_time_ms\": {}, \"warmup_ms\": {}",
        config.samples,
        config.sample_time.as_millis(),
        config.warmup.as_millis()
    );
    s.push_str("  },\n");

    s.push_str("  \"results\": [\n");
    let rows = measurements
        .iter()
        .map(|m| {
            let st = &m.stats;
            let throughput = st
                .mib_per_s(m.size)
                .map_or("null".to_owned(), |t| format!("{t:.2}"));
            format!(
                "    {{ \"op\": {}, \"size\": {}, \"median_ns\": {:.3}, \"q1_ns\": {:.3}, \"q3_ns\": {:.3}, \"min_ns\": {:.3}, \"mib_per_s\": {throughput}, \"samples\": {}, \"iters_per_sample\": {} }}",
                string(m.op),
                m.size,
                st.median_ns,
                st.q1_ns,
                st.q3_ns,
                st.min_ns,
                st.samples,
                st.iters_per_sample
            )
        })
        .collect::<Vec<_>>();
    s.push_str(&rows.join(",\n"));
    s.push_str("\n  ]\n}\n");
    s
}

fn string(v: &str) -> String {
    let mut out = String::with_capacity(v.len() + 2);
    out.push('"');
    for c in v.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The registry dependencies in `[dependencies]`, with the versions `Cargo.lock` resolved them to.
fn direct_dependencies(manifest: &Manifest) -> Vec<(String, String)> {
    let mut in_deps = false;
    let mut names = Vec::new();
    for line in manifest.cargo_toml.lines().map(str::trim) {
        if line.starts_with('[') {
            in_deps = line == "[dependencies]";
            continue;
        }
        if !in_deps || line.is_empty() || line.starts_with('#') || line.contains("path =") {
            continue;
        }
        if let Some((name, _)) = line.split_once('=') {
            names.push(name.trim().trim_matches('"').to_owned());
        }
    }

    let mut locked = Vec::new();
    let mut name = None;
    for line in manifest.cargo_lock.lines().map(str::trim) {
        if let Some(v) = value(line, "name") {
            name = Some(v);
        } else if let (Some(n), Some(v)) = (name.take(), value(line, "version")) {
            locked.push((n, v));
        }
    }

    names
        .into_iter()
        .map(|n| {
            let versions = locked
                .iter()
                .filter(|(l, _)| *l == n)
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>();
            let version = if versions.is_empty() {
                "unknown".to_owned()
            } else {
                versions.join(" + ")
            };
            (n, version)
        })
        .collect()
}

fn value(line: &str, key: &str) -> Option<String> {
    let (k, v) = line.split_once('=')?;
    (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
}
