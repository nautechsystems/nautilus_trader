import type { ReactNode } from "react";

type Tone = "success" | "danger" | "warning" | "info" | "muted";

interface StatusPillProps {
  label: string;
  tone?: Tone;
  icon?: ReactNode;
}

export function StatusPill({ label, tone = "muted", icon }: StatusPillProps) {
  return (
    <span className={`status-pill status-pill--${tone}`}>
      {icon}
      <span>{label}</span>
    </span>
  );
}
