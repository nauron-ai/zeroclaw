import { useEffect, useMemo, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import manifestRaw from "./generated/docs-manifest.json";

type Journey =
  | "start"
  | "build"
  | "integrate"
  | "operate"
  | "secure"
  | "contribute"
  | "reference"
  | "hardware"
  | "troubleshoot";

type Audience =
  | "newcomer"
  | "builder"
  | "operator"
  | "security"
  | "contributor"
  | "integrator"
  | "hardware";

type DocKind = "guide" | "reference" | "runbook" | "policy" | "template" | "report";

type ManifestDoc = {
  id: string;
  path: string;
  title: string;
  summary: string;
  section: string;
  language: string;
  journey: Journey;
  audience: Audience;
  kind: DocKind;
  tags: string[];
  readingMinutes: number;
  startHere: boolean;
  sourceUrl: string;
};

const manifest = (manifestRaw as ManifestDoc[])
  .filter((doc) => doc.language === "en")
  .sort((a, b) => a.path.localeCompare(b.path));

const featuredPaths = [
  "README.md",
  "docs/README.md",
  "docs/SUMMARY.md",
  "docs/commands-reference.md",
  "docs/config-reference.md",
  "docs/operations-runbook.md",
  "docs/network-deployment.md",
];

function normalizePath(value: string) {
  const output: string[] = [];

  for (const part of value.split("/")) {
    if (!part || part === ".") continue;
    if (part === "..") {
      output.pop();
      continue;
    }
    output.push(part);
  }

  return output.join("/");
}

function dirname(relativePath: string) {
  const normalized = normalizePath(relativePath);
  const index = normalized.lastIndexOf("/");
  if (index === -1) {
    return "";
  }
  return normalized.slice(0, index);
}

function resolveRelativePath(currentPath: string, href: string) {
  if (!href || href.startsWith("#") || /^[a-z]+:/i.test(href)) {
    return href;
  }

  const [target, query = ""] = href.split("?");
  const base = target.startsWith("/")
    ? target.slice(1)
    : normalizePath([dirname(currentPath), target].filter(Boolean).join("/"));

  return query ? `${base}?${query}` : base;
}

function buildDocHref(relativePath: string) {
  return `#doc=${encodeURIComponent(relativePath)}`;
}

function buildContentUrl(relativePath: string) {
  return `${import.meta.env.BASE_URL}docs-content/${relativePath}`;
}

function parseHashDoc() {
  const rawHash = window.location.hash.replace(/^#/, "");
  if (!rawHash.startsWith("doc=")) {
    return "";
  }
  return decodeURIComponent(rawHash.slice(4));
}

function sectionLabel(section: string) {
  if (section === "all") return "All docs";
  if (section === "root") return "Root";
  return section.replace(/-/g, " ");
}

function formatLabel(value: string) {
  return value
    .replace(/[-_]/g, " ")
    .replace(/\b\w/g, (match) => match.toUpperCase());
}

export default function App() {
  const defaultDoc = manifest.find((doc) => doc.path === "README.md")?.path ?? manifest[0]?.path ?? "";
  const [selectedPath, setSelectedPath] = useState(() => {
    const fromHash = parseHashDoc();
    return manifest.some((doc) => doc.path === fromHash) ? fromHash : defaultDoc;
  });
  const [sectionFilter, setSectionFilter] = useState("all");
  const [query, setQuery] = useState("");
  const [content, setContent] = useState("");
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");

  const sections = useMemo(() => {
    const values = new Set<string>(["all"]);
    for (const doc of manifest) {
      values.add(doc.section);
    }
    return [...values];
  }, []);

  const filteredDocs = useMemo(() => {
    const q = query.trim().toLowerCase();

    return manifest.filter((doc) => {
      if (sectionFilter !== "all" && doc.section !== sectionFilter) {
        return false;
      }

      if (!q) {
        return true;
      }

      const haystack = `${doc.title} ${doc.summary} ${doc.path} ${doc.tags.join(" ")}`.toLowerCase();
      return haystack.includes(q);
    });
  }, [query, sectionFilter]);

  const selectedDoc = useMemo(
    () => manifest.find((doc) => doc.path === selectedPath) ?? manifest[0],
    [selectedPath]
  );

  const featuredDocs = useMemo(
    () =>
      featuredPaths
        .map((path) => manifest.find((doc) => doc.path === path))
        .filter((doc): doc is ManifestDoc => Boolean(doc)),
    []
  );

  const startHereDocs = useMemo(() => manifest.filter((doc) => doc.startHere).slice(0, 6), []);

  useEffect(() => {
    const onHashChange = () => {
      const next = parseHashDoc();
      if (manifest.some((doc) => doc.path === next)) {
        setSelectedPath(next);
      }
    };

    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, []);

  useEffect(() => {
    if (!selectedDoc) {
      return;
    }

    const nextHash = buildDocHref(selectedDoc.path);
    if (window.location.hash !== nextHash) {
      window.history.replaceState(null, "", `${window.location.pathname}${window.location.search}${nextHash}`);
    }

    const controller = new AbortController();
    setLoading(true);
    setError("");

    fetch(buildContentUrl(selectedDoc.path), { signal: controller.signal })
      .then(async (response) => {
        if (!response.ok) {
          throw new Error(`Unable to load ${selectedDoc.path}`);
        }
        return response.text();
      })
      .then((text) => {
        setContent(text);
        setLoading(false);
      })
      .catch((fetchError: Error) => {
        if (controller.signal.aborted) {
          return;
        }
        setError(fetchError.message);
        setLoading(false);
      });

    return () => controller.abort();
  }, [selectedDoc]);

  if (!selectedDoc) {
    return null;
  }

  return (
    <div className="lc-app">
      <header className="topbar">
        <div className="topbar-inner">
          <a className="brand" href="#doc=README.md">
            LabaClaw Docs
          </a>
          <nav className="top-nav" aria-label="Primary">
            <a href={buildDocHref("README.md")}>Overview</a>
            <a href={buildDocHref("docs/README.md")}>Docs Hub</a>
            <a href={buildDocHref("docs/SUMMARY.md")}>Summary</a>
            <a href="https://github.com/nauron-ai/labaclaw" target="_blank" rel="noreferrer">
              GitHub
            </a>
          </nav>
        </div>
      </header>

      <main className="page">
        <section className="hero">
          <div className="hero-copy">
            <p className="eyebrow">Mesh-first distributed runtime</p>
            <h1>LabaClaw puts the docs surface on the target operating model now.</h1>
            <p className="lead">
              English-only documentation, LabaClaw-first naming, and a simpler operator path for mesh,
              high-performance, and distributed deployment work.
            </p>
            <div className="hero-actions">
              <a className="button button-primary" href={buildDocHref("docs/README.md")}>
                Open docs hub
              </a>
              <a className="button button-secondary" href={buildDocHref("docs/SUMMARY.md")}>
                Browse summary
              </a>
            </div>
            <ul className="hero-points">
              <li>English-only maintained docs</li>
              <li>LabaClaw operator surface by default</li>
              <li>Public fork with explicit upstream sync policy</li>
            </ul>
          </div>

          <aside className="fork-card">
            <p className="fork-label">Fork provenance and sync policy</p>
            <p>
              LabaClaw is a public fork of ZeroClaw. The docs already use the target LabaClaw surface:
              <code> labaclaw</code>, <code>~/.labaclaw</code>, <code>LABACLAW_*</code>,
              <code> /etc/labaclaw</code>, and <code>labaclaw.service</code>.
            </p>
            <p>
              The runtime still carries some legacy <code>zeroclaw</code> identifiers and will be aligned in
              a follow-up code migration track.
            </p>
            <p>
              Upstream content may land from ZeroClaw either 1:1 or as a qualitative adaptation when the
              LabaClaw mesh-first direction requires it.
            </p>
          </aside>
        </section>

        <section className="stats">
          <article className="stat-card">
            <span className="stat-value">{manifest.length}</span>
            <span className="stat-label">English docs indexed</span>
          </article>
          <article className="stat-card">
            <span className="stat-value">{sections.length - 1}</span>
            <span className="stat-label">Sections</span>
          </article>
          <article className="stat-card">
            <span className="stat-value">{startHereDocs.length}</span>
            <span className="stat-label">Start-here routes</span>
          </article>
        </section>

        <section className="routes">
          <div className="section-heading">
            <h2>Recommended routes</h2>
            <p>Start from the docs hub, or jump directly to operator-critical references.</p>
          </div>
          <div className="route-grid">
            {featuredDocs.map((doc) => (
              <button
                key={doc.id}
                className="route-card"
                type="button"
                onClick={() => setSelectedPath(doc.path)}
              >
                <span className="route-kicker">{sectionLabel(doc.section)}</span>
                <strong>{doc.title}</strong>
                <p>{doc.summary}</p>
              </button>
            ))}
          </div>
        </section>

        <section className="workspace">
          <aside className="sidebar">
            <div className="sidebar-panel">
              <label className="searchbox">
                <span>Find a doc</span>
                <input
                  type="search"
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  placeholder="Search by title, summary, tag, or path"
                />
              </label>
              <div className="filter-group" aria-label="Section filters">
                {sections.map((section) => (
                  <button
                    key={section}
                    type="button"
                    className={sectionFilter === section ? "filter-chip active" : "filter-chip"}
                    onClick={() => setSectionFilter(section)}
                  >
                    {sectionLabel(section)}
                  </button>
                ))}
              </div>
            </div>

            <div className="doc-list" role="list">
              {filteredDocs.map((doc) => (
                <button
                  key={doc.id}
                  type="button"
                  className={doc.path === selectedDoc.path ? "doc-card active" : "doc-card"}
                  onClick={() => setSelectedPath(doc.path)}
                >
                  <span className="doc-card-path">{doc.path}</span>
                  <strong>{doc.title}</strong>
                  <p>{doc.summary}</p>
                  <div className="doc-card-meta">
                    <span>{formatLabel(doc.section)}</span>
                    <span>{formatLabel(doc.kind)}</span>
                    <span>{doc.readingMinutes} min</span>
                  </div>
                </button>
              ))}
            </div>
          </aside>

          <section className="reader">
            <div className="reader-header">
              <div>
                <p className="reader-kicker">{selectedDoc.path}</p>
                <h2>{selectedDoc.title}</h2>
                <p className="reader-summary">{selectedDoc.summary}</p>
              </div>
              <div className="reader-meta">
                <span>{formatLabel(selectedDoc.section)}</span>
                <span>{formatLabel(selectedDoc.journey)}</span>
                <span>{selectedDoc.readingMinutes} min read</span>
                <a href={selectedDoc.sourceUrl} target="_blank" rel="noreferrer">
                  View source
                </a>
              </div>
            </div>

            {loading ? <p className="reader-status">Loading document…</p> : null}
            {error ? <p className="reader-status reader-error">{error}</p> : null}

            {!loading && !error ? (
              <article className="markdown-body">
                <ReactMarkdown
                  remarkPlugins={[remarkGfm]}
                  components={{
                    a: ({ href = "", children, ...props }) => {
                      const resolved = resolveRelativePath(selectedDoc.path, href);

                      if (!resolved || href.startsWith("#")) {
                        return (
                          <a href={href} {...props}>
                            {children}
                          </a>
                        );
                      }

                      if (/\.mdx?$/i.test(resolved)) {
                        return (
                          <a
                            href={buildDocHref(resolved)}
                            onClick={(event) => {
                              event.preventDefault();
                              setSelectedPath(resolved);
                            }}
                            {...props}
                          >
                            {children}
                          </a>
                        );
                      }

                      if (/^[a-z]+:/i.test(resolved)) {
                        return (
                          <a href={resolved} target="_blank" rel="noreferrer" {...props}>
                            {children}
                          </a>
                        );
                      }

                      return (
                        <a href={buildContentUrl(resolved)} target="_blank" rel="noreferrer" {...props}>
                          {children}
                        </a>
                      );
                    },
                    img: ({ src = "", alt = "", ...props }) => {
                      const resolved = resolveRelativePath(selectedDoc.path, src);
                      const finalSrc = resolved && !/^[a-z]+:/i.test(resolved) ? buildContentUrl(resolved) : src;
                      return <img src={finalSrc} alt={alt} {...props} />;
                    },
                  }}
                >
                  {content}
                </ReactMarkdown>
              </article>
            ) : null}
          </section>
        </section>
      </main>
    </div>
  );
}
