//! Terminal output: styles, progress and number formatting. Colours go through `anstream`, which
//! drops them when output is not a terminal or `NO_COLOR` is set.

use std::io::IsTerminal;
use std::time::Duration;

use anstream::println;
use anstyle::{AnsiColor, Style};
use indicatif::{ProgressBar, ProgressStyle};

use crate::preflight::{Finding, Verdict};

pub const BOLD: Style = Style::new().bold();
pub const DIM: Style = Style::new().dimmed();
pub const GOOD: Style = AnsiColor::Green.on_default();
pub const BEST: Style = AnsiColor::Green.on_default().bold();
pub const WARN: Style = AnsiColor::Yellow.on_default();
pub const BAD: Style = AnsiColor::Red.on_default().bold();
pub const ACCENT: Style = AnsiColor::Cyan.on_default().bold();

pub fn is_ci() -> bool {
    std::env::var("CI").is_ok_and(|v| !v.is_empty() && v != "false")
}

pub fn heading(text: &str) {
    println!();
    println!("{ACCENT}{text}{ACCENT:#}");
}

/// A `label: value` line for the machine summary.
pub fn field(label: &str, value: &str) {
    println!("  {DIM}{label:<16}{DIM:#} {value}");
}

pub fn finding(f: &Finding) {
    let (style, word) = match f.verdict {
        Verdict::Pass => (GOOD, "ok"),
        Verdict::Info => (DIM, "recorded"),
        Verdict::Warn => (WARN, "warning"),
        Verdict::Fail => (BAD, "FAIL"),
    };
    println!(
        "  {DIM}{:<16}{DIM:#} {:<24} {style}{word}{style:#}",
        f.name, f.value
    );
    if let Some(advice) = &f.advice {
        println!("  {:<16} {style}{advice}{style:#}", "");
    }
}

pub fn warning(text: &str) {
    println!("  {WARN}warning:{WARN:#} {text}");
    if std::env::var_os("GITHUB_ACTIONS").is_some() {
        println!("::warning::{text}");
    }
}

/// A spinner with a status message on a terminal, plain lines otherwise (e.g. in CI logs).
pub struct Progress {
    bar: Option<ProgressBar>,
    prefix: String,
}

impl Progress {
    pub fn new(prefix: &str) -> Self {
        let bar = (std::io::stderr().is_terminal() && !is_ci()).then(|| {
            let bar = ProgressBar::new_spinner().with_style(
                ProgressStyle::with_template("  {spinner} {prefix:.bold} {msg}")
                    .expect("valid template"),
            );
            bar.set_prefix(prefix.to_owned());
            bar.enable_steady_tick(Duration::from_millis(100));
            bar
        });
        Self {
            bar,
            prefix: prefix.to_owned(),
        }
    }

    pub fn set(&self, message: &str) {
        match &self.bar {
            Some(bar) => bar.set_message(message.to_owned()),
            None => println!("  {BOLD}{}{BOLD:#} {message}", self.prefix),
        }
    }

    /// Replaces the spinner with a final status line.
    pub fn finish(self, style: Style, status: &str) {
        if let Some(bar) = self.bar {
            bar.finish_and_clear();
        }
        println!("  {BOLD}{}{BOLD:#} {style}{status}{style:#}", self.prefix);
    }
}

/// `1234.5` as `1,235`.
pub fn thousands(value: f64) -> String {
    let digits = format!("{:.0}", value.max(0.0));
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

pub fn duration_ns(ns: f64) -> String {
    if ns >= 1e6 {
        format!("{:.2} ms", ns / 1e6)
    } else if ns >= 1e4 {
        format!("{:.1} µs", ns / 1e3)
    } else {
        format!("{} ns", thousands(ns))
    }
}
