import { classifyLogLine, countLogLines, filterLogLines, parseLogLines } from "./logs";

describe("pulse log helpers", () => {
  it("classifies log lines into operator-facing severity buckets", () => {
    expect(classifyLogLine("INFO connected")).toBe("INFO");
    expect(classifyLogLine("WARNING retrying")).toBe("WARNING");
    expect(classifyLogLine("ERROR something bad")).toBe("ERROR");
    expect(classifyLogLine("plain output without level")).toBe("OTHER");
  });

  it("parses and filters log lines deterministically", () => {
    const lines = parseLogLines("INFO healthy\nERROR failure\nWARNING retrying\n\nTRACEBACK boom");

    expect(lines.map((line) => line.severity)).toEqual(["INFO", "ERROR", "WARNING", "ERROR"]);
    expect(filterLogLines(lines, "ERROR").map((line) => line.text)).toEqual(["ERROR failure", "TRACEBACK boom"]);
    expect(countLogLines(lines)).toEqual({
      ERROR: 2,
      WARNING: 1,
      INFO: 1,
      OTHER: 0,
    });
  });
});
