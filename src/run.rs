//! The `run` command: build, verify and benchmark every implementation, then save the results.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use anstream::println;

use crate::machine::{self, Machine};
use crate::preflight::{self, Verdict};
use crate::schema::{self, Meta, Op, Preflight, Report, RunFile};
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
        if !args.force {
            return refuse();
        }
        forced.extend(failures(&[finding]));
    }

    ui::heading("Benchmarking (ChaCha20-Poly1305, 1280-byte messages)");
    let reports_dir = Path::new(TARGET_DIR).join("reports");
    if let Err(e) = std::fs::create_dir_all(&reports_dir) {
        println!("{BAD}error:{BAD:#} {}: {e}", reports_dir.display());
        return ExitCode::FAILURE;
    }
    let mut reports = Vec::new();
    let mut failed = Vec::new();
    for imp in &built {
        match bench(imp, &reports_dir, args.quick) {
            Ok(report) => reports.push(report),
            Err(()) => failed.push(imp.name.clone()),
        }
    }
    if reports.is_empty() {
        println!("{BAD}error:{BAD:#} no implementation produced results");
        return ExitCode::FAILURE;
    }

    print_table(&reports);

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
            progress.finish(WARN, "skipped: failed to build");
            let stderr = String::from_utf8_lossy(&out.stderr);
            let lines = stderr.lines().collect::<Vec<_>>();
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

fn bench(imp: &Impl, reports_dir: &Path, quick: bool) -> Result<Report, ()> {
    let progress = Progress::new(&imp.name);
    progress.set("verifying test vectors");
    let out = reports_dir.join(format!("{}.json", imp.name));
    let binary = Path::new(TARGET_DIR).join("release").join(format!(
        "{}{}",
        imp.name.replace('.', "_"),
        std::env::consts::EXE_SUFFIX
    ));

    let mut command = Command::new(&binary);
    command.arg("--out").arg(&out);
    if quick {
        command.arg("--quick");
    }
    let mut child = match command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            progress.finish(BAD, &format!("failed to start {}: {e}", binary.display()));
            return Err(());
        }
    };

    let mut problems = Vec::new();
    if let Some(stderr) = child.stderr.take() {
        for line in BufReader::new(stderr).lines().map_while(Result::ok) {
            if let Some(op) = line.strip_prefix("bench ") {
                progress.set(&format!("benchmarking {op}"));
            } else if line == "verify: ok" {
                progress.set("test vectors ok");
            } else {
                problems.push(line);
            }
        }
    }
    let status = child.wait().map(|s| s.success()).unwrap_or(false);

    let report = status
        .then(|| std::fs::read_to_string(&out).ok())
        .flatten()
        .and_then(|text| serde_json::from_str::<Report>(&text).ok());
    match report {
        Some(report) => {
            progress.finish(GOOD, "done");
            Ok(report)
        }
        None => {
            progress.finish(BAD, "FAILED");
            for line in problems {
                println!("      {BAD}{line}{BAD:#}");
            }
            Err(())
        }
    }
}

fn print_table(reports: &[Report]) {
    let median = |r: &Report, op: Op| {
        r.results
            .iter()
            .find(|m| m.op == op)
            .map(|m| (m.median_ns, m.mib_per_s))
    };
    let mut rows = reports.iter().collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        let key = |r: &Report| median(r, Op::Seal).map_or(f64::INFINITY, |m| m.0);
        key(a).total_cmp(&key(b))
    });
    let best = |op: Op| {
        reports
            .iter()
            .filter_map(|r| median(r, op))
            .map(|m| m.0)
            .fold(f64::INFINITY, f64::min)
    };
    let width = reports
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(0)
        .max(14);

    ui::heading("Results (median per 1280-byte message, fastest first)");
    println!(
        "  {BOLD}{:<width$}  {:>24}  {:>24}{BOLD:#}",
        "implementation", "seal", "open"
    );
    for r in rows {
        let cell = |op: Op| match median(r, op) {
            Some((ns, mib)) => {
                let text = format!("{:>9} {:>8} MiB/s", ui::duration_ns(ns), ui::thousands(mib));
                let style = if ns <= best(op) {
                    BEST
                } else {
                    anstyle::Style::new()
                };
                format!("{style}{text:>24}{style:#}")
            }
            None => format!("{:>24}", "-"),
        };
        println!(
            "  {:<width$}  {}  {}",
            r.name,
            cell(Op::Seal),
            cell(Op::Open)
        );
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
