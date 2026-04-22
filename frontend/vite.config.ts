import react from '@vitejs/plugin-react';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { defineConfig, loadEnv } from 'vite';
import { ViteImageOptimizer } from 'vite-plugin-image-optimizer';
import tsconfigPaths from 'vite-tsconfig-paths';
import tailwindcss from 'tailwindcss';
import autoprefixer from 'autoprefixer';
import { heroui } from '@heroui/theme';

const frontendRoot = fileURLToPath(new URL('.', import.meta.url));

// https://vitejs.dev/config/
export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd());
  const backendDebugUrl = env.VITE_DEBUG_BACKEND_URL || 'http://127.0.0.1:14500';
  console.log('backendDebugUrl', backendDebugUrl);
  return {
    root: frontendRoot,
    plugins: [
      react(),
      tsconfigPaths({
        root: frontendRoot,
      }),
      ViteImageOptimizer({}),
    ],
    css: {
      postcss: {
        plugins: [
          tailwindcss({
            content: [
              path.join(frontendRoot, 'index.html'),
              path.join(frontendRoot, 'src/layouts/**/*.{js,ts,jsx,tsx,mdx}'),
              path.join(frontendRoot, 'src/pages/**/*.{js,ts,jsx,tsx,mdx}'),
              path.join(frontendRoot, 'src/components/**/*.{js,ts,jsx,tsx,mdx}'),
              path.join(frontendRoot, '../node_modules/@heroui/theme/dist/**/*.{js,ts,jsx,tsx}'),
            ],
            safelist: [
              {
                pattern:
                  /bg-(primary|secondary|success|danger|warning|default)-(50|100|200|300|400|500|600|700|800|900)/,
              },
            ],
            theme: {
              extend: {
                fontFamily: {
                  mono: [
                    'ui-monospace',
                    'SFMono-Regular',
                    'SF Mono',
                    'Menlo',
                    'Consolas',
                    'Liberation Mono',
                    'JetBrains Mono',
                    'monospace',
                  ],
                },
              },
            },
            darkMode: 'class',
            plugins: [
              heroui({
                themes: {
                  light: {
                    colors: {
                      primary: {
                        DEFAULT: '#88C0D0',
                        foreground: '#fff',
                        50: '#F0F9FC',
                        100: '#D7F0F8',
                        200: '#AEE1F2',
                        300: '#88C0D0',
                        400: '#5E9FBF',
                        500: '#4C8DAE',
                        600: '#3A708C',
                        700: '#2A546A',
                        800: '#1A3748',
                        900: '#0B1B26',
                      },
                      secondary: {
                        DEFAULT: '#FF7FAC',
                        foreground: '#fff',
                        50: '#FFF0F5',
                        100: '#FFE4E9',
                        200: '#FFCDD9',
                        300: '#FF9EB5',
                        400: '#FF7FAC',
                        500: '#F33B7C',
                        600: '#C92462',
                        700: '#991B4B',
                        800: '#691233',
                        900: '#380A1B',
                      },
                      danger: {
                        DEFAULT: '#DB3694',
                        foreground: '#fff',
                        50: '#FEEAF6',
                        100: '#FDD7DD',
                        200: '#FBAFC4',
                        300: '#F485AE',
                        400: '#E965A3',
                        500: '#DB3694',
                        600: '#BC278B',
                        700: '#9D1B7F',
                        800: '#7F1170',
                        900: '#690A66',
                      },
                    },
                  },
                  dark: {
                    colors: {
                      primary: {
                        DEFAULT: '#88C0D0',
                        foreground: '#fff',
                        50: '#0B1B26',
                        100: '#1A3748',
                        200: '#2A546A',
                        300: '#3A708C',
                        400: '#4C8DAE',
                        500: '#5E9FBF',
                        600: '#88C0D0',
                        700: '#AEE1F2',
                        800: '#D7F0F8',
                        900: '#F0F9FC',
                      },
                      secondary: {
                        DEFAULT: '#f31260',
                        foreground: '#fff',
                        50: '#310413',
                        100: '#610726',
                        200: '#920b3a',
                        300: '#c20e4d',
                        400: '#f31260',
                        500: '#f54180',
                        600: '#f871a0',
                        700: '#faa0bf',
                        800: '#fdd0df',
                        900: '#fee7ef',
                      },
                      danger: {
                        DEFAULT: '#DB3694',
                        foreground: '#fff',
                        50: '#690A66',
                        100: '#7F1170',
                        200: '#9D1B7F',
                        300: '#BC278B',
                        400: '#DB3694',
                        500: '#E965A3',
                        600: '#F485AE',
                        700: '#FBAFC4',
                        800: '#FDD7DD',
                        900: '#FEEAF6',
                      },
                    },
                  },
                },
              }),
            ],
          } as Parameters<typeof tailwindcss>[0]),
          autoprefixer(),
        ],
      },
    },
    base: '/webui/',
    server: {
      host: '0.0.0.0',
      port: 1420,
      proxy: {
        '/api/ws/terminal': {
          target: backendDebugUrl,
          ws: true,
          changeOrigin: true,
        },
        '/api/Debug/ws': {
          target: backendDebugUrl,
          ws: true,
          changeOrigin: true,
        },
        '/api': backendDebugUrl,
        '/files': backendDebugUrl,
        '/plugin': backendDebugUrl,
        '/webui/fonts/CustomFont.woff': backendDebugUrl,
        '/webui/sw.js': backendDebugUrl,
      },
    },
    preview: {
      host: '0.0.0.0',
      port: 4173,
      strictPort: true,
    },
    build: {
      outDir: 'dist',
      emptyOutDir: true,
      assetsInlineLimit: 0,
      rollupOptions: {
        output: {
          manualChunks(id) {
            if (id.includes('node_modules')) {
              if (id.includes('react-dom')) {
                return 'react-dom';
              }
              if (id.includes('react-router-dom')) {
                return 'react-router-dom';
              }
              if (id.includes('react-hook-form')) {
                return 'react-hook-form';
              }
              if (id.includes('react-hot-toast')) {
                return 'react-hot-toast';
              }
              if (id.includes('qface')) {
                return 'qface';
              }
              if (id.includes('@uiw/react-codemirror') || id.includes('@codemirror/view') || id.includes('@codemirror/theme-one-dark')) {
                return 'codemirror-core';
              }
              if (id.includes('@codemirror/lang-')) {
                return 'codemirror-lang';
              }
            }
          },
        },
      },
    },
  };
});
