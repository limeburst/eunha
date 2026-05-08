/** @type {import('tailwindcss').Config} */
export default {
  content: ['./index.html', './src/**/*.{ts,tsx}'],
  theme: {
    extend: {
      colors: {
        bg:       'var(--color-bg)',
        surface:  'var(--color-surface)',
        elevated: 'var(--color-elevated)',
        border:   'var(--color-border)',
        text:     'var(--color-text)',
        muted:    'var(--color-muted)',
        accent:   'var(--color-accent)',
        danger:   'var(--color-danger)',
        success:  'var(--color-success)',
      },
      fontFamily: {
        brand: ['Lora', 'Georgia', 'serif'],
        sans:  ['DM Sans', 'sans-serif'],
      },
    },
  },
  plugins: [],
}
