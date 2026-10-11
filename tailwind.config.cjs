module.exports = {
  content: ['./src/**/*.{js,jsx}'],
  darkMode: 'class',
  theme: {
    extend: {
      colors: {
        primary: {
          50: '#eef9ff',
          100: '#dcf2ff',
          200: '#b2e6ff',
          300: '#6ed4ff',
          400: '#33c5ff',
          500: '#0aa3eb',
          600: '#0082ca',
          700: '#0068a3',
          800: '#005887',
          900: '#06496f',
          950: '#042f4a',
        },
        gray: {
          850: '#1f2937',
          900: '#111827',
          950: '#0b0f19',
        },
      },
      fontFamily: {
        sans: ['Inter', 'sans-serif'],
      },
    },
  },
};
