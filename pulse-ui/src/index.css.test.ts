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
});
