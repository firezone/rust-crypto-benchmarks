"use strict";

const OPS = [
  {
    id: "chacha20poly1305_seal",
    title: "ChaCha20-Poly1305 seal",
    note: "Encrypting a transport data packet. 1420 bytes is the payload of a full packet on a 1500-byte link.",
  },
  {
    id: "chacha20poly1305_open",
    title: "ChaCha20-Poly1305 open",
    note: "Verifying and decrypting a transport data packet.",
  },
  {
    id: "xchacha20poly1305_seal",
    title: "XChaCha20-Poly1305 seal",
    note: "Encrypting the cookie in a cookie reply message, with MAC1 as associated data.",
  },
  {
    id: "x25519",
    title: "X25519",
    note: "Shared secret from a static secret key and a peer public key, as in the handshake.",
  },
  {
    id: "blake2s256",
    title: "BLAKE2s-256",
    note: "Unkeyed hash, the building block of the handshake's HASH, MAC and HKDF.",
  },
];

const LIBRARY_ORDER = ["ring", "RustCrypto", "libcrux", "graviola", "aws-lc-rs"];
const STORAGE_KEY = "rust-crypto-benchmarks.arch";

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
  if (ns >= 1e6) return `${fmt(ns / 1e6, 2)} ms`;
  if (ns >= 1e3) return `${fmt(ns / 1e3, ns >= 1e5 ? 0 : ns >= 1e4 ? 1 : 2)} µs`;
  return `${fmt(ns, ns >= 100 ? 0 : 1)} ns`;
}

function formatRate(run) {
  if (run.mib_per_s != null) {
    return `${fmt(run.mib_per_s, run.mib_per_s >= 100 ? 0 : 1)} MiB/s`;
  }
  const opsPerSec = 1e9 / run.median_ns;
  return opsPerSec >= 1e6 ? `${fmt(opsPerSec / 1e6, 2)} M op/s` : `${fmt(opsPerSec / 1e3, 1)} k op/s`;
}

const formatDate = (iso) => iso.replace("T", " ").replace(/:\d\dZ$/, " UTC");

// Libraries that split primitives across crates list only the crate for this operation.
const CRATE_HINTS = {
  chacha20poly1305_seal: /chacha/,
  chacha20poly1305_open: /chacha/,
  xchacha20poly1305_seal: /chacha/,
  x25519: /25519/,
  blake2s256: /blake2/,
};

function cratesFor(run, op) {
  const relevant = run.crates.filter((c) => CRATE_HINTS[op]?.test(c.name));
  return (relevant.length ? relevant : run.crates).map((c) => `${c.name} ${c.version}`).join(", ");
}

const rate = (row) => (row.mib_per_s != null ? row.mib_per_s : 1e9 / row.median_ns);

function libraryColors(runs) {
  const libs = [...new Set(runs.map((r) => r.library))].sort((a, b) => {
    const ia = LIBRARY_ORDER.indexOf(a);
    const ib = LIBRARY_ORDER.indexOf(b);
    return (ia < 0 ? 99 : ia) - (ib < 0 ? 99 : ib) || a.localeCompare(b);
  });
  // Colors follow the library, so they stay put when switching architecture.
  return new Map(libs.map((lib, i) => [lib, `var(--lib-${(i % 8) + 1})`]));
}

function unique(values) {
  return [...new Set(values.filter((v) => v != null && v !== ""))];
}

function renderMeta(runs, arch) {
  const meta = runs.map((r) => r.meta);
  const commits = unique(meta.map((m) => m.commit));
  const dates = unique(meta.map((m) => m.date)).sort();
  const fields = [
    ["Architecture", arch],
    ["CPU", unique(meta.map((m) => m.cpu)).join("; ")],
    ["CPU features", unique(meta.flatMap((m) => m.cpu_features || [])).join(" ")],
    ["Compiler", unique(meta.map((m) => m.rustc)).join("; ")],
    ["Date", dates.length ? formatDate(dates[dates.length - 1]) : "unknown"],
    [
      "Commit",
      commits.map((c, i) => [
        i ? ", " : "",
        /^[0-9a-f]{40}$/.test(c)
          ? el("a", { href: `https://github.com/firezone/rust-crypto-benchmarks/commit/${c}` }, c.slice(0, 10))
          : c,
      ]),
    ],
  ];
  const box = document.getElementById("meta");
  box.replaceChildren(...fields.map(([k, v]) => el("div", {}, el("dt", {}, k), el("dd", {}, v))));

  const notices = [];
  if (meta.some((m) => m.quick)) {
    notices.push("These results come from a quick run with few samples. Treat them as a smoke test, not a measurement.");
  }
  return notices.map((n) => el("p", { class: "notice", role: "note" }, n));
}

function renderCase(rows, colors) {
  rows.sort((a, b) => a.median_ns - b.median_ns);
  const best = Math.max(...rows.map(rate));

  const body = rows.map((row) => {
    const width = (rate(row) / best) * 100;
    const crates = cratesFor(row.run, row.op);
    const bar = () =>
      el(
        "div",
        { class: "bar-track", title: `${row.run.impl}: ${formatRate(row)} (${fmt(width, 0)}% of fastest)` },
        el("div", { class: "bar", style: `width:${width.toFixed(2)}%;background:${colors.get(row.run.library)}` }),
      );
    return el(
      "tr",
      {},
      el(
        "td",
        {},
        el("span", { class: "impl-name" }, el("span", { class: "swatch", style: `background:${colors.get(row.run.library)}`, "aria-hidden": "true" }), row.run.impl),
        el("span", { class: "crates" }, crates),
        el("div", { class: "bar-inline", "aria-hidden": "true" }, bar()),
      ),
      el("td", { class: "num" }, formatTime(row.median_ns)),
      el("td", { class: "num col-iqr" }, `${formatTime(row.q1_ns)} to ${formatTime(row.q3_ns)}`),
      el("td", { class: "num" }, formatRate(row)),
      el("td", { class: "bar-cell", "aria-hidden": "true" }, bar()),
    );
  });

  return el(
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
        el("th", { scope: "col", class: "num col-iqr" }, "IQR"),
        el("th", { scope: "col", class: "num" }, "Throughput"),
        el("th", { scope: "col", class: "bar-cell" }, el("span", { class: "visually-hidden" }, "Relative")),
      ),
    ),
    el("tbody", {}, body),
  );
}

function render(data, arch) {
  const runs = data.runs.filter((r) => r.meta.arch === arch);
  const colors = libraryColors(data.runs);
  const content = document.getElementById("content");
  const notices = renderMeta(runs, arch);

  const legend = el(
    "ul",
    { class: "legend", "aria-label": "Libraries" },
    [...colors].map(([lib, color]) => el("li", {}, el("span", { class: "swatch", style: `background:${color}` }), lib)),
  );

  const sections = [];
  const toc = [];
  for (const op of OPS) {
    const sizes = [...new Set(runs.flatMap((r) => r.results.filter((x) => x.op === op.id).map((x) => x.size)))].sort(
      (a, b) => a - b,
    );
    if (sizes.length === 0) continue;

    const supported = new Set();
    const cases = sizes.map((size) => {
      const rows = runs.flatMap((run) =>
        run.results.filter((x) => x.op === op.id && x.size === size).map((x) => ({ ...x, run })),
      );
      rows.forEach((r) => supported.add(r.run.impl));
      const heading = size > 0 ? `${size} bytes` : "One operation";
      return el("div", { class: "case" }, el("h3", {}, heading), renderCase(rows, colors));
    });

    const missing = runs.map((r) => r.impl).filter((i) => !supported.has(i));
    toc.push(el("a", { href: `#${op.id}` }, op.title));
    sections.push(
      el(
        "section",
        { class: "op", id: op.id, "aria-labelledby": `${op.id}-title` },
        el("h2", { id: `${op.id}-title` }, op.title),
        el("p", { class: "op-note" }, op.note),
        cases,
        missing.length ? el("p", { class: "missing" }, `Not provided by: ${missing.sort().join(", ")}.`) : null,
      ),
    );
  }

  document.getElementById("toc").replaceChildren(...toc);
  content.replaceChildren(...notices, legend, ...sections);
}

function setupArchPicker(data) {
  const arches = [...new Set(data.runs.map((r) => r.meta.arch))].sort();
  let stored = null;
  try {
    stored = localStorage.getItem(STORAGE_KEY);
  } catch {}
  let current = arches.includes(stored) ? stored : arches[0];

  const picker = document.querySelector(".arch-picker");
  const buttons = arches.map((arch) => {
    const button = el("button", { type: "button", "aria-pressed": String(arch === current) }, arch);
    button.addEventListener("click", () => {
      current = arch;
      buttons.forEach((b) => b.setAttribute("aria-pressed", String(b.textContent === arch)));
      try {
        localStorage.setItem(STORAGE_KEY, arch);
      } catch {}
      render(data, arch);
    });
    return button;
  });
  picker.replaceChildren(...buttons);
  document.getElementById("controls").hidden = arches.length < 2;
  render(data, current);
}

fetch("results.json", { cache: "no-cache" })
  .then((res) => {
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    return res.json();
  })
  .then((data) => {
    if (!data.runs || data.runs.length === 0) throw new Error("no runs in results.json");
    setupArchPicker(data);
  })
  .catch((err) => {
    document.getElementById("content").replaceChildren(
      el("p", { class: "status" }, `Could not load results: ${err.message}`),
    );
  });
