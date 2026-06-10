import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { defineConfig } from '@rspress/core';
import { pluginLlms } from '@rspress/plugin-llms';
import pluginFileTree from 'rspress-plugin-file-tree';

const __dirname = path.dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  root: 'content',
  lang: 'zh',
  title: 'BYK',
  description: '轻量级、可扩展的 CLI 命令行工具集',
  globalStyles: path.join(__dirname, 'styles', 'custom.css'),
  plugins: [pluginLlms(), pluginFileTree()],
  markdown: {
    link: {
      checkDeadLinks: {
        excludes: ['/llms-full.txt', '/basics/'],
      },
    },
  },
  route: {
    cleanUrls: true,
  },
  themeConfig: {
    llmsUI: true,
    editLink: {
      docRepoBaseUrl: 'https://github.com/fcbyk/byk/tree/main/docs/content',
    },
    socialLinks: [
      {
        icon: 'github',
        mode: 'link',
        content: 'https://github.com/fcbyk/byk',
      },
    ],
  },
});
