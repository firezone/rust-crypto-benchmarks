"use strict";

const OPS = [
  { id: "seal", title: "Seal", note: "Encrypting a transport data packet in place." },
  { id: "open", title: "Open", note: "Verifying and decrypting a transport data packet in place." },
];
const LIBRARY_ORDER = ["ring", "RustCrypto", "libcrux", "graviola", "aws-lc-rs"];
const STORAGE_KEY = "rust-crypto-benchmarks.machine";

const el = (tag, attrs = {}, ...children) => {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (key === "class") node.className = value;
    else if (key === "style") node.style.cssText = value;
    else node.setAttribute(key, value);
  }
  for (const child of children.flat()) {
    if (child == null) continue;
    node.append(child instanceof Node ? child : document.createTextNode(String(child)));
  }
  return node;
};

const fmt = (value, digits) =>
  value.toLocaleString("en-US", { minimumFractionDigits: digits, maximumFractionDigits: digits });

function formatTime(ns) {
  if (ns >= 1e4) return `${fmt(ns / 1e3, 1)} µs`;
  return `${fmt(ns, 0)} ns`;
}

const formatRate = (mib) => `${fmt((mib * 1024 * 1024 * 8) / 1e9, 1)} Gbit/s`;
const formatDate = (iso) => iso.replace("T", " ").replace(/:\d\dZ$/, " UTC");

// Runs on the same hardware belong together, whatever label they were submitted under.
const machineKey = (run) => [run.meta.cpu, run.meta.arch, run.meta.os].join("|");
const machineSlug = (key) =>
  key
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");

function groupMachines(runs) {
  const groups = new Map();
  for (const run of runs) {
    const key = machineKey(run);
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(run);
  }
  for (const list of groups.values()) list.sort((a, b) => b.meta.date.localeCompare(a.meta.date));
  return groups;
}

function libraryColors(runs) {
  const libs = [...new Set(runs.flatMap((r) => r.implementations.map((i) => i.library)))].sort((a, b) => {
    const ia = LIBRARY_ORDER.indexOf(a);
    const ib = LIBRARY_ORDER.indexOf(b);
    return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib) || a.localeCompare(b);
  });
  // Colors follow the library, so they stay put when switching machines.
  return new Map(libs.map((lib, i) => [lib, `var(--lib-${(i % 8) + 1})`]));
}

function describePlacement(p) {
  if (!p) return "not recorded";
  if (p.method === "qos") return "QoS user-interactive (prefers performance cores)";
  const parts = [`pinned to CPU ${p.cpu}`];
  if (p.cpu_max_mhz) parts.push(`max ${fmt(p.cpu_max_mhz, 0)} MHz`);
  if (p.efficiency_class != null) parts.push(`efficiency class ${p.efficiency_class}`);
  return parts.join(", ");
}

function renderMeta(run) {
  const m = run.meta;
  const p = m.preflight;
  const power = p
    ? [
        p.power.source && (p.power.source === "ac" ? "AC" : "battery"),
        p.power.low_power_mode != null && `low power mode ${p.power.low_power_mode ? "on" : "off"}`,
        p.power.profile && `profile ${p.power.profile}`,
        p.power.governor && `governor ${p.power.governor}`,
      ].filter(Boolean)
    : [];
  const fields = [
    ["Machine", run.machine],
    ["CPU", m.cpu_count ? `${m.cpu} (${m.cpu_count} threads)` : m.cpu],
    ["System", `${m.os} ${m.arch}`],
    ["CPU features", (m.cpu_features || []).join(" ") || "none detected"],
    ["Compiler", m.rustc],
    ["C compiler", m.c_compiler || "not recorded"],
    ["Date", formatDate(m.date)],
    [
      "Commit",
      m.commit
        ? [
            el("a", { href: `https://github.com/firezone/rust-crypto-benchmarks/commit/${m.commit}` }, m.commit.slice(0, 10)),
            m.dirty ? " (dirty working tree)" : null,
          ]
        : "not recorded",
    ],
    ["CPU busy before run", p && p.cpu_busy_pct != null ? `${fmt(p.cpu_busy_pct, 1)}%` : "not recorded"],
    ["Power", power.length ? power.join(", ") : "not recorded"],
    ["Placement", describePlacement(m.placement)],
    ["Rounds", m.rounds ? `${m.rounds}, interleaved` : "1"],
  ];
  document
    .getElementById("meta")
    .replaceChildren(...fields.map(([k, v]) => el("div", {}, el("dt", {}, k), el("dd", {}, v))));

  const notices = [];
  if (p && p.forced.length) {
    notices.push(`This run went ahead despite failed preflight checks (${p.forced.join("; ")}). Treat it with care.`);
  }
  if (m.quick) {
    notices.push("This is a quick run with few samples: a smoke test, not a measurement.");
  }
  return notices.map((n) => el("p", { class: "notice", role: "note" }, n));
}

function renderOp(run, op, colors) {
  const rows = run.implementations
    .map((imp) => ({ imp, m: imp.results.find((r) => r.op === op.id) }))
    .filter((r) => r.m)
    .sort((a, b) => a.m.median_ns - b.m.median_ns);
  if (rows.length === 0) return null;
  const best = Math.max(...rows.map((r) => r.m.mib_per_s));

  const body = rows.map(({ imp, m }) => {
    const width = (m.mib_per_s / best) * 100;
    const color = colors.get(imp.library);
    const bar = () =>
      el(
        "div",
        { class: "bar-track", title: `${imp.impl}: ${formatRate(m.mib_per_s)} (${fmt(width, 0)}% of fastest)` },
        el("div", { class: "bar", style: `width:${width.toFixed(2)}%;background:${color}` }),
      );
    return el(
      "tr",
      {},
      el(
        "td",
        {},
        el("span", { class: "impl-name" }, el("span", { class: "swatch", style: `background:${color}`, "aria-hidden": "true" }), imp.impl),
        el("span", { class: "crates" }, imp.crates.map((c) => `${c.name} ${c.version}`).join(", ")),
        el("div", { class: "bar-inline", "aria-hidden": "true" }, bar()),
      ),
      el(
        "td",
        { class: "num" },
        formatTime(m.median_ns),
        m.spread_pct != null ? el("span", { class: "spread-inline" }, `±${fmt(m.spread_pct, 1)}%`) : null,
      ),
      el(
        "td",
        { class: "num col-spread", title: m.round_medians_ns ? `Round medians: ${m.round_medians_ns.map(formatTime).join(", ")}` : "" },
        m.spread_pct != null ? `±${fmt(m.spread_pct, 1)}%` : "n/a",
      ),
      el("td", { class: "num" }, formatRate(m.mib_per_s)),
      el("td", { class: "bar-cell", "aria-hidden": "true" }, bar()),
    );
  });

  return el(
    "section",
    { class: "case", "aria-labelledby": `${op.id}-title` },
    el("h2", { id: `${op.id}-title` }, op.title),
    el("p", { class: "op-note" }, op.note),
    el(
      "table",
      {},
      el(
        "thead",
        {},
        el(
          "tr",
          {},
          el("th", { scope: "col" }, "Implementation"),
          el("th", { scope: "col", class: "num" }, "Median"),
          el("th", { scope: "col", class: "num col-spread", title: "Half the range of the per-round medians, relative to the median" }, "Spread"),
          el("th", { scope: "col", class: "num" }, "Throughput"),
          el("th", { scope: "col", class: "bar-cell" }, el("span", { class: "visually-hidden" }, "Relative")),
        ),
      ),
      el("tbody", {}, body),
    ),
  );
}

function render(run, colors) {
  const notices = renderMeta(run);
  const libs = new Set(run.implementations.map((i) => i.library));
  const legend = el(
    "ul",
    { class: "legend", "aria-label": "Libraries" },
    [...colors].filter(([lib]) => libs.has(lib)).map(([lib, color]) => el("li", {}, el("span", { class: "swatch", style: `background:${color}` }), lib)),
  );
  document.getElementById("content").replaceChildren(...notices, legend, ...OPS.map((op) => renderOp(run, op, colors)).filter(Boolean));
}

function setup(data) {
  const colors = libraryColors(data.runs);
  const groups = groupMachines(data.runs);
  const machineSelect = document.getElementById("machine");
  const runSelect = document.getElementById("run");

  // Group the machine list by architecture, newest machines first within each.
  const byArch = new Map();
  for (const [key, runs] of groups) {
    const arch = runs[0].meta.arch;
    if (!byArch.has(arch)) byArch.set(arch, []);
    byArch.get(arch).push([key, runs]);
  }
  const options = [...byArch.keys()].sort().map((arch) =>
    el(
      "optgroup",
      { label: arch },
      byArch
        .get(arch)
        .sort((a, b) => a[1][0].machine.localeCompare(b[1][0].machine))
        .map(([key, runs]) => el("option", { value: key }, `${runs[0].machine} (${runs[0].meta.cpu})`)),
    ),
  );
  machineSelect.replaceChildren(...options);

  const bySlug = new Map([...groups.keys()].map((key) => [machineSlug(key), key]));
  const fromHash = () => bySlug.get(decodeURIComponent(location.hash.slice(1)));

  let stored = null;
  try {
    stored = localStorage.getItem(STORAGE_KEY);
  } catch {}
  machineSelect.value = fromHash() ?? (groups.has(stored) ? stored : [...groups.keys()][0]);

  const showMachine = () => {
    const runs = groups.get(machineSelect.value);
    runSelect.replaceChildren(
      ...runs.map((run, i) => el("option", { value: String(i) }, `${formatDate(run.meta.date)}${i === 0 ? " (latest)" : ""}`)),
    );
    runSelect.disabled = runs.length < 2;
    render(runs[0], colors);
    history.replaceState(null, "", `#${machineSlug(machineSelect.value)}`);
    try {
      localStorage.setItem(STORAGE_KEY, machineSelect.value);
    } catch {}
  };
  machineSelect.addEventListener("change", showMachine);
  window.addEventListener("hashchange", () => {
    const key = fromHash();
    if (key && key !== machineSelect.value) {
      machineSelect.value = key;
      showMachine();
    }
  });
  runSelect.addEventListener("change", () => render(groups.get(machineSelect.value)[Number(runSelect.value)], colors));

  document.getElementById("controls").hidden = false;
  showMachine();
}

fetch("results.json", { cache: "no-cache" })
  .then((res) => {
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return res.json();
  })
  .then((data) => {
    if (!data.runs || data.runs.length === 0) throw new Error("no runs in results.json");
    setup(data);
  })
  .catch((err) => {
    document.getElementById("content").replaceChildren(el("p", { class: "status" }, `Could not load results: ${err.message}`));
  });
