/**
 * Vitest stand-in for `next/font/google`: font loading happens at Next
 * build time (self-hosted via next/font internals) and is not runnable in
 * the node test environment. The mock keeps the layout's CSS-variable
 * contract so rendered markup still carries the font classes.
 */
type FontOptions = { variable?: string };

function createFont(className: string, variable?: string) {
  return () => ({
    className,
    style: variable ? { [variable]: `var(--mock-${variable.slice(2)})` } : {},
  });
}

export const Sora = createFont('mock-font-sora', '--font-sora');
export const Open_Sans = createFont('mock-font-open-sans', '--font-open-sans');
