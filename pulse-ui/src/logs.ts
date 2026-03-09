export type LogSeverity = "ERROR" | "WARNING" | "INFO" | "OTHER";
export type LogFilter = "ALL" | "ERROR" | "WARNING" | "INFO";

export interface LogLine {
  id: string;
  text: string;
  severity: LogSeverity;
}

const ERROR_PATTERN = /\b(ERROR|CRITICAL|EXCEPTION|TRACEBACK|FAILED TO START|FAILED WITH RESULT)\b/i;
const WARNING_PATTERN = /\b(WARN|WARNING)\b/i;
const INFO_PATTERN = /\b(INFO)\b/i;

export function classifyLogLine(line: string): LogSeverity {
  if (ERROR_PATTERN.test(line)) {
    return "ERROR";
  }
  if (WARNING_PATTERN.test(line)) {
    return "WARNING";
  }
  if (INFO_PATTERN.test(line)) {
    return "INFO";
  }
  return "OTHER";
}

export function parseLogLines(logs: string): LogLine[] {
  return logs
    .split(/\r?\n/)
    .map((line) => line.trimEnd())
    .filter((line) => line.length > 0)
    .map((line, index) => ({
      id: `log-line-${index}`,
      text: line,
      severity: classifyLogLine(line),
    }));
}

export function filterLogLines(lines: LogLine[], filter: LogFilter): LogLine[] {
  if (filter === "ALL") {
    return lines;
  }
  return lines.filter((line) => line.severity === filter);
}

export function countLogLines(lines: LogLine[]): Record<LogSeverity, number> {
  return lines.reduce<Record<LogSeverity, number>>(
    (counts, line) => {
      counts[line.severity] += 1;
      return counts;
    },
    {
      ERROR: 0,
      WARNING: 0,
      INFO: 0,
      OTHER: 0,
    },
  );
}
