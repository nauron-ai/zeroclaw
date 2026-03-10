#!/usr/bin/env node

import fs from "node:fs/promises";
import path from "node:path";

const siteDir = process.cwd();
const repoRoot = path.resolve(siteDir, "..");
const docsRoot = path.join(repoRoot, "docs");
const outRoot = path.join(siteDir, "public", "docs-content");
const generatedDir = path.join(siteDir, "src", "generated");
const manifestFile = path.join(generatedDir, "docs-manifest.json");

const ROOT_ASSET_PATTERN = /\.(png|jpe?g|gif|webp|svg|avif)$/i;
const MARKDOWN_PATTERN = /\.(md|mdx)$/i;
const ROOT_MARKDOWN_FILES = ["README.md", "CONTRIBUTING.md"];

function toPosix(filePath) {
  return filePath.replace(/\\/g, "/");
}

function normalizePath(filePath) {
  return toPosix(filePath).replace(/^\/+/, "").replace(/\/+/g, "/");
}

function stripMarkdownSyntax(text) {
  return text
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/<[^>]+>/g, "")
    .replace(/[*_~>#]+/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

function detectSection(relativePath) {
  const rel = normalizePath(relativePath);
  if (!rel.startsWith("docs/")) {
    return "root";
  }

  if (
    rel.startsWith("docs/project/") ||
    rel.startsWith("docs/project-triage-snapshot-") ||
    rel.startsWith("docs/docs-audit-")
  ) {
    return "archive";
  }

  const parts = rel.split("/");
  return parts[1] || "docs";
}

function detectJourney(relativePath) {
  const rel = normalizePath(relativePath).toLowerCase();

  if (!rel.startsWith("docs/")) {
    return "start";
  }

  if (
    rel.includes("/security/") ||
    rel.includes("security-roadmap") ||
    rel.includes("sandbox") ||
    rel.includes("advisory") ||
    rel.includes("vulnerability") ||
    rel.includes("agnostic-security") ||
    rel.includes("audit-logging")
  ) {
    return "secure";
  }

  if (
    rel.includes("/operations/") ||
    rel.includes("/sop/") ||
    rel.includes("runbook") ||
    rel.includes("release-process") ||
    rel.includes("ci-map") ||
    rel.includes("required-check")
  ) {
    return "operate";
  }

  if (
    rel.includes("/contributing/") ||
    rel.includes("pr-workflow") ||
    rel.includes("reviewer-playbook") ||
    rel.includes("doc-template")
  ) {
    return "contribute";
  }

  if (
    rel.includes("/datasheets/") ||
    rel.includes("/hardware/") ||
    rel.includes("arduino") ||
    rel.includes("esp32") ||
    rel.includes("nucleo") ||
    rel.includes("hardware-peripherals")
  ) {
    return "hardware";
  }

  if (rel.includes("troubleshooting")) {
    return "troubleshoot";
  }

  if (
    rel.includes("/reference/") ||
    rel.includes("commands-reference") ||
    rel.includes("config-reference") ||
    rel.includes("providers-reference") ||
    rel.includes("channels-reference") ||
    rel.includes("resource-limits") ||
    rel.includes("audit-event-schema")
  ) {
    return "reference";
  }

  if (
    rel.includes("langgraph") ||
    rel.includes("custom-providers") ||
    rel.includes("nextcloud-talk") ||
    rel.includes("mattermost") ||
    rel.includes("matrix-e2ee") ||
    rel.includes("zai-glm") ||
    rel.includes("qwen-provider") ||
    rel.includes("proxy-agent")
  ) {
    return "integrate";
  }

  if (
    rel.includes("getting-started") ||
    rel.includes("one-click-bootstrap") ||
    rel.includes("docker-setup") ||
    rel.includes("network-deployment") ||
    rel === "docs/readme.md"
  ) {
    return "start";
  }

  return "build";
}

function detectAudience(relativePath, journey) {
  const rel = normalizePath(relativePath).toLowerCase();

  if (journey === "start") return "newcomer";
  if (journey === "operate" || rel.includes("troubleshooting")) return "operator";
  if (journey === "secure") return "security";
  if (journey === "contribute") return "contributor";
  if (journey === "integrate") return "integrator";
  if (journey === "hardware") return "hardware";

  return "builder";
}

function detectKind(relativePath) {
  const rel = normalizePath(relativePath).toLowerCase();

  if (
    rel.includes("runbook") ||
    rel.includes("playbook") ||
    rel.includes("/sop/") ||
    rel.includes("operations/")
  ) {
    return "runbook";
  }

  if (
    rel.includes("policy") ||
    rel.includes("roadmap") ||
    rel.includes("release-process") ||
    rel.includes("required-check-mapping")
  ) {
    return "policy";
  }

  if (rel.includes("template") || rel.includes("checklist")) {
    return "template";
  }

  if (
    rel.includes("report") ||
    rel.includes("snapshot") ||
    rel.includes("inventory") ||
    rel.includes("docs-audit")
  ) {
    return "report";
  }

  if (
    rel.includes("/reference/") ||
    rel.includes("reference") ||
    rel.includes("schema") ||
    rel.includes("summary.md")
  ) {
    return "reference";
  }

  return "guide";
}

function isStartHere(relativePath, journey) {
  const rel = normalizePath(relativePath).toLowerCase();

  return (
    journey === "start" ||
    rel === "readme.md" ||
    rel === "docs/readme.md" ||
    rel === "docs/summary.md" ||
    rel === "docs/commands-reference.md" ||
    rel === "docs/config-reference.md" ||
    rel === "docs/network-deployment.md" ||
    rel === "docs/operations-runbook.md"
  );
}

function fallbackTitle(relativePath) {
  const filename = path.basename(relativePath).replace(/\.(md|mdx)$/i, "");

  if (filename.toLowerCase() === "readme") {
    const parent = path.basename(path.dirname(relativePath));
    if (parent && parent !== "." && parent !== "docs") {
      return `${parent} README`;
    }
  }

  return filename
    .replace(/[._-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

function extractTitle(markdown, relativePath) {
  const lines = markdown.split(/\r?\n/);

  for (const line of lines) {
    const heading = /^#{1,2}\s+(.+)$/.exec(line.trim());
    if (heading) {
      return stripMarkdownSyntax(heading[1].replace(/\s+#*$/, "")) || fallbackTitle(relativePath);
    }
  }

  const h1Tag = /<h1[^>]*>([\s\S]*?)<\/h1>/i.exec(markdown);
  if (h1Tag) {
    return stripMarkdownSyntax(h1Tag[1]) || fallbackTitle(relativePath);
  }

  return fallbackTitle(relativePath);
}

function extractSummary(markdown) {
  const lines = markdown.split(/\r?\n/);
  let inCode = false;

  for (const rawLine of lines) {
    const line = rawLine.trim();

    if (line.startsWith("```")) {
      inCode = !inCode;
      continue;
    }

    if (
      inCode ||
      !line ||
      line.startsWith("#") ||
      line.startsWith("|") ||
      line.startsWith("<") ||
      line.startsWith(">") ||
      line.startsWith("-") ||
      line.startsWith("*")
    ) {
      continue;
    }

    const cleaned = stripMarkdownSyntax(line);
    if (cleaned.length >= 24) {
      return cleaned.slice(0, 220);
    }
  }

  return "Project documentation.";
}

function estimateReadingMinutes(markdown) {
  const words = stripMarkdownSyntax(markdown).split(" ").filter(Boolean).length;
  return Math.max(1, Math.min(35, Math.ceil(words / 220) || 1));
}

function inferTags(relativePath, title, summary, journey, audience, kind, section) {
  const rel = normalizePath(relativePath).toLowerCase();
  const bag = `${rel} ${title} ${summary}`.toLowerCase();
  const tags = new Set([journey, audience, kind, section]);

  const rules = [
    { pattern: /(getting-started|one-click-bootstrap|onboard|setup|readme)/, tag: "onboarding" },
    { pattern: /(docker|network|gateway|deploy|daemon|mesh)/, tag: "deployment" },
    { pattern: /(security|sandbox|advisory|vulnerability|audit)/, tag: "security" },
    { pattern: /(commands|config|providers|channels|reference|schema)/, tag: "reference" },
    { pattern: /(operations|runbook|sop|release|ci|workflow|gate)/, tag: "operations" },
    { pattern: /(langgraph|matrix|mattermost|nextcloud|glm|custom-provider)/, tag: "integrations" },
    { pattern: /(hardware|arduino|esp32|nucleo|datasheet)/, tag: "hardware" },
    { pattern: /(contributing|reviewer|pull request|project|fork)/, tag: "contributing" },
    { pattern: /(troubleshoot|diagnos|doctor|debug)/, tag: "troubleshooting" },
  ];

  for (const rule of rules) {
    if (rule.pattern.test(bag)) {
      tags.add(rule.tag);
    }
  }

  return [...tags].map((tag) => tag.trim()).filter(Boolean).sort().slice(0, 8);
}

function toId(relativePath) {
  return normalizePath(relativePath)
    .toLowerCase()
    .replace(/[^a-z0-9/.-]/g, "-")
    .replace(/[/.]+/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-|-$/g, "");
}

async function ensureDir(dirPath) {
  await fs.mkdir(dirPath, { recursive: true });
}

async function walkFiles(rootDir) {
  const result = [];
  const stack = [rootDir];

  while (stack.length > 0) {
    const current = stack.pop();
    if (!current) continue;

    const entries = await fs.readdir(current, { withFileTypes: true });

    for (const entry of entries) {
      const next = path.join(current, entry.name);
      if (entry.isDirectory()) {
        stack.push(next);
      } else if (entry.isFile()) {
        result.push(next);
      }
    }
  }

  return result;
}

async function copyIntoPublic(filePath) {
  const rel = normalizePath(path.relative(repoRoot, filePath));
  const target = path.join(outRoot, rel);
  await ensureDir(path.dirname(target));
  await fs.copyFile(filePath, target);
}

async function main() {
  await ensureDir(generatedDir);
  await fs.rm(outRoot, { recursive: true, force: true });
  await ensureDir(outRoot);

  const docsAllFiles = await walkFiles(docsRoot);
  const docsMarkdownFiles = docsAllFiles.filter((filePath) => MARKDOWN_PATTERN.test(filePath));

  const rootMarkdownFiles = [];
  for (const relativePath of ROOT_MARKDOWN_FILES) {
    const absolutePath = path.join(repoRoot, relativePath);
    try {
      await fs.access(absolutePath);
      rootMarkdownFiles.push(absolutePath);
    } catch {
      // skip missing files
    }
  }

  const rootEntries = await fs.readdir(repoRoot, { withFileTypes: true });
  const rootAssetFiles = rootEntries
    .filter((entry) => entry.isFile() && ROOT_ASSET_PATTERN.test(entry.name))
    .map((entry) => path.join(repoRoot, entry.name));

  for (const filePath of [...docsAllFiles, ...rootMarkdownFiles, ...rootAssetFiles]) {
    await copyIntoPublic(filePath);
  }

  const manifestEntries = [];

  for (const filePath of [...rootMarkdownFiles, ...docsMarkdownFiles]) {
    const relativePath = normalizePath(path.relative(repoRoot, filePath));
    const content = await fs.readFile(filePath, "utf8");
    const journey = detectJourney(relativePath);
    const audience = detectAudience(relativePath, journey);
    const kind = detectKind(relativePath);
    const title = extractTitle(content, relativePath);
    const summary = extractSummary(content);
    const section = detectSection(relativePath);

    manifestEntries.push({
      id: toId(relativePath),
      path: relativePath,
      title,
      summary,
      section,
      language: "en",
      journey,
      audience,
      kind,
      tags: inferTags(relativePath, title, summary, journey, audience, kind, section),
      readingMinutes: estimateReadingMinutes(content),
      startHere: isStartHere(relativePath, journey),
      sourceUrl: `https://github.com/nauron-ai/labaclaw/blob/main/${relativePath}`,
    });
  }

  manifestEntries.sort((a, b) => a.path.localeCompare(b.path));
  await fs.writeFile(manifestFile, JSON.stringify(manifestEntries, null, 2) + "\n", "utf8");

  process.stdout.write(
    `[docs-manifest] generated ${manifestEntries.length} English markdown entries and copied docs assets\n`
  );
}

main().catch((error) => {
  process.stderr.write(`[docs-manifest] generation failed: ${String(error)}\n`);
  process.exit(1);
});
