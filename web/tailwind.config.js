/** @type {import('tailwindcss').Config} */
export default {
  content: [
    "./index.html",
    "./src/**/*.{js,ts,jsx,tsx}",
  ],
  theme: {
    extend: {
      colors: {
        miao: {
          bg: '#121212', // Main background (Deep Black)
          panel: '#1E1E1E', // Panel/Card background (Dark Gray)
          border: '#333333', // Borders
          text: '#E0E0E0', // Primary Text
          muted: '#888888', // Secondary Text
          green: {
            DEFAULT: '#00AB44', // Netdata Green
            hover: '#00C950',
            dim: 'rgba(0, 171, 68, 0.1)',
          },
          red: '#FF4444',
        }
      }
    },
  },
  plugins: [],
}