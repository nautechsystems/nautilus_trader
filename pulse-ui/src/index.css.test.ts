import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const currentDir = dirname(fileURLToPath(import.meta.url));
const css = readFileSync(resolve(currentDir, "index.css"), "utf8");

function getRuleBody(selector: string) {
  const escapedSelector = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return css.match(new RegExp(`${escapedSelector}\\s*\\{([\\s\\S]*?)\\n\\}`))?.[1] ?? "";
}

describe("pulse shell stylesheet", () => {
  it("uses a flat body background instead of gradients", () => {
    const bodyRule = getRuleBody("body");

    expect(bodyRule).toContain("background: var(--bg-base);");
    expect(bodyRule).not.toMatch(/gradient\(/);
  });

  it("does not hard-lock viewport scrolling in the body rule", () => {
    const bodyRule = getRuleBody("body");

    expect(bodyRule).not.toContain("overflow: hidden;");
  });

  it("keeps the shell on the viewport while avoiding nested vertical scroll traps", () => {
    const appShellRule = getRuleBody(".app-shell");
    const contentRule = getRuleBody(".content");

    expect(appShellRule).toContain("min-height: 100vh;");
    expect(contentRule).toContain("flex: 1;");
    expect(contentRule).not.toContain("overflow: auto;");
  });

  it("uses a compact typography scale for the shell and jobs table", () => {
    const rootRule = getRuleBody(":root");
    const bodyRule = getRuleBody("body");

    expect(rootRule).toContain("--font-size-xs: 11px;");
    expect(rootRule).toContain("--font-size-sm: 12px;");
    expect(rootRule).toContain("--font-size-md: 13px;");
    expect(rootRule).toContain("--font-size-lg: 15px;");
    expect(rootRule).toContain("--font-size-xl: 24px;");
    expect(bodyRule).toContain("font-size: var(--font-size-md);");
    expect(css).toMatch(/\.topbar__title\s*\{[\s\S]*font-size: var\(--font-size-xl\);/);
    expect(css).toMatch(/\.group-row__label\s*\{[\s\S]*font-size: var\(--font-size-lg\);/);
    expect(css).toMatch(/\.jobs-table td\s*\{[\s\S]*font-size: var\(--font-size-sm\);/);
    expect(css).toMatch(/\.job-row__secondary\s*\{[\s\S]*font-size: var\(--font-size-xs\);/);
  });

  it("stacks the logs modal above the sticky top bar", () => {
    const topbarRule = getRuleBody(".topbar");
    const modalBackdropRule = getRuleBody(".modal-backdrop");

    expect(topbarRule).toContain("z-index: 40;");
    expect(modalBackdropRule).toMatch(/z-index:\s*[4-9]\d|z-index:\s*[1-9]\d{2,}/);
  });

  it("adds a narrow-screen media block that adapts shell spacing and dialog controls", () => {
    expect(css).toMatch(/@media \(max-width: 760px\)/);
    expect(css).toMatch(/@media \(max-width: 760px\)[\s\S]*\.topbar,\s*\.content\s*\{[\s\S]*padding-left:\s*16px;[\s\S]*padding-right:\s*16px;/);
    expect(css).toMatch(/@media \(max-width: 760px\)[\s\S]*\.modal__actions\s*\{[\s\S]*(width:\s*100%;|flex-wrap:\s*wrap;)/);
  });

  it("does not pin table headers at the viewport top once page-level scrolling is enabled", () => {
    const tableHeaderRule = getRuleBody(".jobs-table thead th");

    expect(tableHeaderRule).not.toContain("position: sticky;");
    expect(tableHeaderRule).not.toContain("top: 0;");
  });

  it("lets compact job-card secondary text wrap instead of clipping long error previews", () => {
    expect(css).toMatch(/\.job-card\s+\.job-row__secondary\s*\{[\s\S]*white-space:\s*normal;/);
    expect(css).toMatch(/\.job-card\s+\.job-row__secondary\s*\{[\s\S]*max-width:\s*none;/);
    expect(css).toMatch(/\.job-card\s+\.job-row__secondary\s*\{[\s\S]*overflow:\s*visible;/);
  });
});
