// Iconos monocromos (heredan el color del texto).
const base = {
  width: 18,
  height: 18,
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.8,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
};

export const MicIcon = () => (
  <svg {...base} aria-hidden>
    <rect x="9" y="3" width="6" height="11" rx="3" />
    <path d="M5 11a7 7 0 0 0 14 0M12 18v3" />
  </svg>
);

export const ScreenIcon = () => (
  <svg {...base} aria-hidden>
    <rect x="3" y="4" width="18" height="12" rx="2" />
    <path d="M8 20h8M12 16v4" />
  </svg>
);

export const ClipIcon = () => (
  <svg {...base} aria-hidden>
    <path d="M20.5 11.5 12 20a5 5 0 0 1-7-7l8.5-8.5a3.3 3.3 0 0 1 4.7 4.7L9.7 17.7a1.7 1.7 0 0 1-2.4-2.4L15 7.6" />
  </svg>
);

export const SendIcon = () => (
  <svg {...base} aria-hidden>
    <path d="M12 19V5M5 12l7-7 7 7" />
  </svg>
);

export const StopIcon = () => (
  <svg {...base} aria-hidden>
    <rect x="7" y="7" width="10" height="10" rx="1.5" fill="currentColor" />
  </svg>
);
