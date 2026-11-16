/**
 * The Copybook mark: a vermilion seal holding a copied page. The back sheet is an outline,
 * the front sheet carries ruled lines, so it reads as "copy" at 16 px and as a copybook
 * page at larger sizes. The same geometry is exported as the app and tray icon.
 */
export function Seal({ size = 20, muted = false }: { size?: number; muted?: boolean }) {
  return (
    <svg
      className="seal"
      width={size}
      height={size}
      viewBox="0 0 64 64"
      aria-hidden="true"
      style={muted ? { filter: 'grayscale(1)', opacity: 0.6 } : undefined}
    >
      <rect x="4" y="4" width="56" height="56" rx="13" fill="var(--seal, #b94631)" />
      <rect x="15" y="13" width="24" height="31" rx="3.5" fill="none" stroke="#fff" strokeWidth="3.5" />
      <rect x="23.5" y="20.5" width="27" height="32" rx="4.5" fill="var(--seal, #b94631)" />
      <rect x="25" y="22" width="24" height="29" rx="3.5" fill="#fff" />
      <path d="M30 31h14M30 37h14M30 43h8" stroke="var(--seal, #b94631)" strokeWidth="2.75" strokeLinecap="round" />
    </svg>
  );
}
