//! The `run` command: build, verify and benchmark every implementation, then save the results.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anstream::println;

use crate::machine::{self, Machine};
use crate::placement;
use crate::preflight::{self, Verdict};
use crate::rounds::{self, Budget};
use crate::schema::{self, Meta, Op, Placement, PlacementMethod, Preflight, Report, RunFile};
use crate::ui::{self, BAD, BEST, BOLD, DIM, GOOD, Progress, WARN};

#[derive(clap::Args, Debug, Default)]
pub struct RunArgs {
    /// Only run these implementations (directory names under impls/)
    pub impls: Vec<String>,

    /// Take fewer, shorter samples: a smoke test, not a measurement worth submitting
    #[arg(long)]
    pub quick: bool,

    /// Benchmark even if a preflight check fails (the run is flagged in the results)
    #[arg(long)]
    pub force: bool,

    /// Label for this machine in the results [default: derived from the CPU model and OS]
    #[arg(long, value_name = "LABEL")]
    pub machine: Option<String>,

    /// Measure in this many interleaved rounds [default: 7, or 3 with --quick]
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..=100))]
    pub rounds: Option<u32>,

    /// Pin the benchmarks to this logical CPU [default: the fastest one] (Linux and Windows)
    #[arg(long, value_name = "N")]
    pub cpu: Option<u32>,

    /// Directory to save the results file in
    #[arg(long, value_name = "DIR", default_value = "results")]
    pub out_dir: PathBuf,
}

/// Implementations are built here, apart from the runner's own `target/debug`.
const TARGET_DIR: &str = "target/impls";

struct Impl {
    name: String,
    dir: PathBuf,
}

pub fn run(args: RunArgs) -> ExitCode {
    let ci = ui::is_ci();
    let machine = Machine::detect();
    let label = args
        .machine
        .clone()
        .unwrap_or_else(|| machine.default_label());
    if !schema::is_label(&label) {
        println!(
            "{BAD}error:{BAD:#} machine label {label:?} must be lowercase letters and digits separated by `-`, `.` or `_`"
        );
        return ExitCode::FAILURE;
    }

    ui::heading("Machine");
    ui::field("label", &label);
    ui::field("cpu", &machine.cpu);
    ui::field(
        "cores",
        &machine
            .cpu_count
            .map_or("unknown".to_owned(), |n| n.to_string()),
    );
    ui::field("system", &format!("{} {}", machine.os, machine.arch));
    ui::field("compiler", &machine.rustc);
    let placement = match placement::choose(args.cpu) {
        Ok(placement) => placement,
        Err(e) => {
            println!("{BAD}error:{BAD:#} --cpu: {e}");
            return ExitCode::FAILURE;
        }
    };
    ui::field("placement", &placement::describe(placement.as_ref()));
    if args.cpu.is_some()
        && placement
            .as_ref()
            .is_none_or(|p| p.method != PlacementMethod::Affinity)
    {
        ui::warning("--cpu is ignored: this system cannot pin processes to a CPU");
    }
    let rounds = args.rounds.unwrap_or(if args.quick {
        rounds::DEFAULT_QUICK_ROUNDS
    } else {
        rounds::DEFAULT_ROUNDS
    });
    if args.quick {
        ui::warning(
            "--quick: few, short samples, fine for checking the setup but not for submitting",
        );
    }

    // Power first, so a throttled machine is refused before minutes of compiling.
    ui::heading("Preflight");
    let mut forced = Vec::new();
    let (power, findings) = preflight::power();
    for finding in &findings {
        ui::finding(finding);
    }
    if findings.is_empty() {
        println!("  {DIM}no power information available on this system{DIM:#}");
    }
    let power_failures = failures(&findings);
    if !power_failures.is_empty() {
        if !(args.force || ci) {
            return refuse();
        }
        forced.extend(power_failures);
    }

    let impls = match discover(&args.impls) {
        Ok(impls) => impls,
        Err(e) => {
            println!("{BAD}error:{BAD:#} {e}");
            return ExitCode::FAILURE;
        }
    };

    ui::heading("Building");
    let built = impls
        .into_iter()
        .filter(|i| build(i, machine.arch))
        .collect::<Vec<_>>();
    if built.is_empty() {
        println!("{BAD}error:{BAD:#} nothing could be built");
        return ExitCode::FAILURE;
    }

    // After building, so the compiler's own load is not mistaken for other applications.
    ui::heading("Checking that the machine is idle");
    println!("  {DIM}sampling CPU load...{DIM:#}");
    let (cpu_busy_pct, finding) = preflight::cpu_busy();
    ui::finding(&finding);
    if finding.verdict == Verdict::Fail {
        // CI runners cannot be asked to close anything, so there it is only recorded.
        if !(args.force || ci) {
            return refuse();
        }
        forced.extend(failures(&[finding]));
    }

    ui::heading(&format!(
        "Benchmarking (ChaCha20-Poly1305, 1280-byte messages, {rounds} interleaved rounds)"
    ));
    let reports_dir = Path::new(TARGET_DIR).join("reports");
    if let Err(e) = std::fs::create_dir_all(&reports_dir) {
        println!("{BAD}error:{BAD:#} {}: {e}", reports_dir.display());
        return ExitCode::FAILURE;
    }
    let budget = Budget::per_round(rounds, args.quick);
    let mut active = built.iter().collect::<Vec<_>>();
    let mut per_impl = (0..built.len()).map(|_| Vec::new()).collect::<Vec<_>>();
    let mut failed = Vec::new();
    let mut cycles_note = None;
    for round in 0..rounds {
        let progress = Progress::quiet(&format!("round {}/{rounds}", round + 1));
        let mut dropped = Vec::new();
        for imp in rounds::order(&active, round) {
            let index = built
                .iter()
                .position(|b| b.name == imp.name)
                .expect("built");
            match bench(
                imp,
                &reports_dir,
                &budget,
                placement.as_ref(),
                &progress,
                &mut cycles_note,
            ) {
                Ok(report) => per_impl[index].push(report),
                Err(problems) => {
                    println!("  {BAD}{} FAILED{BAD:#}", imp.name);
                    for line in problems {
                        println!("      {BAD}{line}{BAD:#}");
                    }
                    failed.push(imp.name.clone());
                    per_impl[index].clear();
                    dropped.push(imp.name.clone());
                }
            }
        }
        active.retain(|imp| !dropped.contains(&imp.name));
        progress.finish(GOOD, "done");
    }
    let reports = per_impl
        .into_iter()
        .filter(|r| !r.is_empty())
        .map(rounds::aggregate)
        .collect::<Vec<_>>();
    if reports.is_empty() {
        println!("{BAD}error:{BAD:#} no implementation produced results");
        return ExitCode::FAILURE;
    }

    print_table(&reports, cycles_note.as_deref());

    let run = RunFile {
        schema: schema::SCHEMA_VERSION,
        machine: label,
        meta: Meta {
            date: machine::now_rfc3339(),
            arch: machine.arch.to_owned(),
            os: machine.os.to_owned(),
            cpu: machine.cpu,
            cpu_count: machine.cpu_count,
            cpu_features: machine.cpu_features,
            rustc: machine.rustc,
            commit: std::env::var("GITHUB_SHA").ok().filter(|s| !s.is_empty()),
            quick: args.quick,
            preflight: Some(Preflight {
                cpu_busy_pct: cpu_busy_pct.map(|p| (p * 10.0).round() / 10.0),
                power,
                forced,
            }),
            placement,
            rounds: Some(rounds),
        },
        implementations: reports,
    };
    let path = match save(&run, &args.out_dir) {
        Ok(path) => path,
        Err(e) => {
            println!("{BAD}error:{BAD:#} failed to save the results: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!();
    println!("Saved to {BOLD}{}{BOLD:#}", path.display());

    if !ci {
        println!(
            "\nTo share these results, commit the file and open a pull request against\nfirezone/rust-crypto-benchmarks:\n\n    git checkout -b add-results\n    git add {}\n    git commit -m \"chore: add benchmark results\"\n    git push <your fork> add-results",
            path.display()
        );
        if args.quick {
            ui::warning("this was a --quick run: run again without --quick before submitting");
        }
    }

    if failed.is_empty() {
        ExitCode::SUCCESS
    } else {
        println!(
            "\n{BAD}FAILED:{BAD:#} {} did not pass the test vectors or crashed",
            failed.join(", ")
        );
        ExitCode::FAILURE
    }
}

fn failures(findings: &[preflight::Finding]) -> Vec<String> {
    findings
        .iter()
        .filter(|f| f.verdict == Verdict::Fail)
        .map(|f| format!("{}: {}", f.name, f.value))
        .collect()
}

fn refuse() -> ExitCode {
    println!(
        "\n{BAD}Not benchmarking:{BAD:#} fix the problem above and run again, or pass --force to\nbenchmark anyway (the run is then flagged in the results)."
    );
    ExitCode::FAILURE
}

pub fn list() -> ExitCode {
    match discover(&[]) {
        Ok(impls) => {
            let names = impls.into_iter().map(|i| i.name).collect::<Vec<_>>();
            println!(
                "{}",
                serde_json::to_string(&names).expect("strings serialize")
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn discover(only: &[String]) -> Result<Vec<Impl>, String> {
    let mut impls = std::fs::read_dir("impls")
        .map_err(|e| format!("impls/: {e}"))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("Cargo.toml").is_file())
        .filter_map(|dir| {
            let name = dir.file_name()?.to_str()?.to_owned();
            Some(Impl { name, dir })
        })
        .collect::<Vec<_>>();
    impls.sort_by(|a, b| a.name.cmp(&b.name));

    let only = only
        .iter()
        .map(|o| o.trim_end_matches('/').trim_start_matches("impls/"))
        .collect::<Vec<_>>();
    if let Some(unknown) = only.iter().find(|o| !impls.iter().any(|i| i.name == **o)) {
        return Err(format!("no implementation named {unknown} under impls/"));
    }
    if !only.is_empty() {
        impls.retain(|i| only.contains(&i.name.as_str()));
    }
    Ok(impls)
}

/// The `arches = [...]` list under `[package.metadata.bench]`, if any.
fn supported_arches(manifest: &str) -> Option<Vec<String>> {
    let line = manifest
        .lines()
        .find(|l| l.trim_start().starts_with("arches"))?;
    let list = line.split_once('[')?.1.split_once(']')?.0;
    Some(
        list.split(',')
            .map(|a| a.trim().trim_matches('"').to_owned())
            .filter(|a| !a.is_empty())
            .collect(),
    )
}

fn build(imp: &Impl, arch: &str) -> bool {
    let manifest = imp.dir.join("Cargo.toml");
    let arches = std::fs::read_to_string(&manifest)
        .ok()
        .and_then(|m| supported_arches(&m));
    if arches.is_some_and(|arches| !arches.iter().any(|a| a == arch)) {
        Progress::new(&imp.name).finish(DIM, &format!("skipped: not supported on {arch}"));
        return false;
    }

    let progress = Progress::new(&imp.name);
    progress.set("building");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .args(["build", "--release", "--locked", "--manifest-path"])
        .arg(&manifest)
        .env("CARGO_TARGET_DIR", TARGET_DIR)
        .stdin(Stdio::null())
        .output();
    match output {
        Ok(out) if out.status.success() => {
            progress.finish(GOOD, "built");
            true
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let lines = stderr.lines().collect::<Vec<_>>();
            let reason = build_failure_reason(&lines);
            progress.finish(WARN, &format!("skipped: failed to build ({reason})"));
            for line in &lines[lines.len().saturating_sub(12)..] {
                println!("      {DIM}{line}{DIM:#}");
            }
            ui::warning(&format!(
                "{} does not build on this machine and is left out of the results",
                imp.name
            ));
            false
        }
        Err(e) => {
            progress.finish(WARN, &format!("skipped: could not run cargo: {e}"));
            false
        }
    }
}

/// A one-line summary of a failed build, e.g. a missing tool like CMake, NASM or a C compiler.
fn build_failure_reason(stderr: &[&str]) -> String {
    let lower = stderr.join("\n").to_lowercase();
    for (needle, reason) in [
        ("nasm", "NASM not found"),
        ("cmake", "CMake missing or failing"),
        ("link.exe", "MSVC build tools not found"),
        ("linker `cc` not found", "C toolchain not found"),
        ("failed to find tool", "C compiler not found"),
        ("is `cc` not installed", "C compiler not found"),
    ] {
        if lower.contains(needle) {
            return reason.to_owned();
        }
    }
    stderr
        .iter()
        .find(|l| l.trim_start().starts_with("error"))
        .map_or("see the output below", |l| l.trim())
        .chars()
        .take(100)
        .collect()
}

/// Runs one implementation for one round. `cycles_note` receives what the implementation says
/// about its cycle counter.
fn bench(
    imp: &Impl,
    reports_dir: &Path,
    budget: &Budget,
    placement: Option<&Placement>,
    progress: &Progress,
    cycles_note: &mut Option<String>,
) -> Result<Report, Vec<String>> {
    progress.set(&format!("{}: verifying test vectors", imp.name));
    let out = reports_dir.join(format!("{}.json", imp.name));
    let binary = Path::new(TARGET_DIR).join("release").join(format!(
        "{}{}",
        imp.name.replace('.', "_"),
        std::env::consts::EXE_SUFFIX
    ));

    let mut command = Command::new(&binary);
    command.arg("--out").arg(&out).args(budget.args());
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    placement::before_spawn(&mut command, placement);
    let mut child = command
        .spawn()
        .map_err(|e| vec![format!("failed to start {}: {e}", binary.display())])?;
    if let Err(e) = placement::after_spawn(&child, placement) {
        let _ = child.kill();
        return Err(vec![format!("failed to pin to the chosen CPU: {e}")]);
    }

    let mut problems = Vec::new();
    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Some(op) = line.strip_prefix("bench ") {
                progress.set(&format!("{}: benchmarking {op}", imp.name));
            } else if line == "verify: ok" {
                progress.set(&format!("{}: test vectors ok", imp.name));
            } else if let Some(note) = line.strip_prefix("cycles: ") {
                *cycles_note = Some(note.to_owned());
            } else {
                problems.push(line);
            }
        }
    }
    let status = child.wait().map(|s| s.success()).unwrap_or(false);

    status
        .then(|| std::fs::read_to_string(&out).ok())
        .flatten()
        .and_then(|text| serde_json::from_str::<Report>(&text).ok())
        .ok_or(problems)
}

fn print_table(reports: &[Report], cycles_note: Option<&str>) {
    fn find(r: &Report, op: Op) -> Option<&schema::Measurement> {
        r.results.iter().find(|m| m.op == op)
    }
    let mut rows = reports.iter().collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        let key = |r: &Report| find(r, Op::Seal).map_or(f64::INFINITY, |m| m.median_ns);
        key(a).total_cmp(&key(b))
    });
    let best = |op: Op| {
        reports
            .iter()
            .filter_map(|r| find(r, op))
            .map(|m| m.median_ns)
            .fold(f64::INFINITY, f64::min)
    };
    let width = reports
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(0)
        .max(14);
    let cycles = reports
        .iter()
        .any(|r| r.results.iter().any(|m| m.cycles_per_byte.is_some()));
    let text_width = if cycles { 35 } else { 24 };
    let cell_width = text_width + 8;

    ui::heading("Results (median of the round medians per 1280-byte message, fastest first)");
    println!(
        "  {BOLD}{:<width$}  {:>cell_width$}  {:>cell_width$}{BOLD:#}",
        "implementation", "seal", "open"
    );
    for r in rows {
        let cell = |op: Op| match find(r, op) {
            Some(m) => {
                let mut text = format!(
                    "{:>9} {:>6.1} Gbit/s",
                    ui::duration_ns(m.median_ns),
                    m.mib_per_s * 1024.0 * 1024.0 * 8.0 / 1e9
                );
                if cycles {
                    text.push_str(&match m.cycles_per_byte {
                        Some(c) => format!(" {c:>6.2} c/B"),
                        None => format!("{:>11}", "-"),
                    });
                }
                let style = if m.median_ns <= best(op) {
                    BEST
                } else {
                    anstyle::Style::new()
                };
                let spread = m.spread_pct.map_or(String::new(), |s| format!("±{s:.1}%"));
                format!("{style}{text:>text_width$}{style:#} {DIM}{spread:>7}{DIM:#}")
            }
            None => format!("{:>cell_width$}", "-"),
        };
        println!(
            "  {:<width$}  {}  {}",
            r.name,
            cell(Op::Seal),
            cell(Op::Open)
        );
    }
    println!("  {DIM}± is half the range of the per-round medians, relative to the median{DIM:#}");
    match cycles_note {
        Some(note) if cycles => {
            println!("  {DIM}c/B is CPU cycles per byte, counted with {note}{DIM:#}")
        }
        Some(note) => println!("  {DIM}cycles per byte {note}{DIM:#}"),
        None => {}
    }
}

fn save(run: &RunFile, dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    // 2026-10-07T12:03:01Z -> 2026-10-07T120301Z, so names sort chronologically.
    let stamp = run.meta.date.replace(':', "");
    let mut path = dir.join(format!("{stamp}-{}.json", run.machine));
    for n in 2.. {
        if !path.exists() {
            break;
        }
        path = dir.join(format!("{stamp}-{}-{n}.json", run.machine));
    }
    let mut json = serde_json::to_string_pretty(run).map_err(std::io::Error::other)?;
    json.push('\n');
    std::fs::write(&path, json)?;
    Ok(path)
}
