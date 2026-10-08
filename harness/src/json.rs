use std::fmt::Write;

use crate::measure::Config;
use crate::{Manifest, Measurement, SIZE};

pub fn report(
    library: &str,
    manifest: &Manifest,
    config: &Config,
    cycle_counter: Option<&str>,
    measurements: &[Measurement],
) -> String {
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
    let results = measurements
        .iter()
        .map(|m| {
            let st = &m.stats;
            let cycles = st
                .cycles_per_byte(SIZE)
                .map_or(String::new(), |c| format!(", \"cycles_per_byte\": {c:.3}"));
            format!(
                "    {{ \"op\": {}, \"size\": {SIZE}, \"median_ns\": {:.3}, \"q1_ns\": {:.3}, \"q3_ns\": {:.3}, \"min_ns\": {:.3}, \"mib_per_s\": {:.2}, \"samples\": {}, \"iters_per_sample\": {}{cycles} }}",
                string(m.op),
                st.median_ns,
                st.q1_ns,
                st.q3_ns,
                st.min_ns,
                st.mib_per_s(SIZE),
                st.samples,
                st.iters_per_sample
            )
        })
        .collect::<Vec<_>>();

    let mut s = String::from("{\n");
    let _ = writeln!(s, "  \"impl\": {},", string(manifest.name()));
    let _ = writeln!(s, "  \"library\": {},", string(library));
    let _ = writeln!(s, "  \"crates\": [{}],", crates.join(", "));
    if let Some(counter) = cycle_counter {
        let _ = writeln!(s, "  \"cycle_counter\": {},", string(counter));
    }
    let _ = writeln!(
        s,
        "  \"config\": {{ \"samples\": {}, \"sample_time_ms\": {}, \"warmup_ms\": {} }},",
        config.samples,
        config.sample_time.as_millis(),
        config.warmup.as_millis()
    );
    let _ = writeln!(s, "  \"results\": [\n{}\n  ]", results.join(",\n"));
    s.push_str("}\n");
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
