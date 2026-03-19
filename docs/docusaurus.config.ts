import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

// This runs in Node.js - Don't use client-side code here (browser APIs, JSX...)

const defaultRepository = 'Prometheus-Dynamics/HeliOS';
const [defaultOrganizationName, defaultProjectName] = (
  process.env.GITHUB_REPOSITORY || defaultRepository
).split('/');

const organizationName =
  process.env.DOCUSAURUS_ORGANIZATION_NAME || defaultOrganizationName;
const projectName = process.env.DOCUSAURUS_PROJECT_NAME || defaultProjectName;
const siteUrl =
  process.env.DOCUSAURUS_URL || `https://${organizationName}.github.io`;
const siteBaseUrl =
  process.env.DOCUSAURUS_BASE_URL || `/${projectName}/`;
const editUrl =
  process.env.DOCUSAURUS_EDIT_URL ||
  `https://github.com/${organizationName}/${projectName}/tree/main/docs`;

const config: Config = {
  title: 'Helios Docs',
  tagline: 'Instrument-grade pipelines and telemetry, documented.',
  favicon: 'img/favicon.png',

  // Future flags, see https://docusaurus.io/docs/api/docusaurus-config#future
  future: {
    v4: true, // Improve compatibility with the upcoming Docusaurus v4
  },

  // Set the production url of your site here
  url: siteUrl,
  // Set the /<baseUrl>/ pathname under which your site is served
  // For GitHub Pages project deployments, this is usually '/<projectName>/'.
  baseUrl: siteBaseUrl,
  trailingSlash: true,

  // GitHub pages deployment config.
  organizationName,
  projectName,

  onBrokenLinks: 'throw',

  // Even if you don't use internationalization, you can use this field to set
  // useful metadata like html lang. For example, if your site is Chinese, you
  // may want to replace "en" with "zh-Hans".
  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          routeBasePath: '/',
          editUrl,
        },
        // Release notes live as pages under src/pages/release-notes/.
        // Keep the blog disabled to avoid "blog-like" formatting for releases.
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themes: [
    [
      require.resolve('@easyops-cn/docusaurus-search-local'),
      {
        hashed: true,
        language: ['en'],
        // Our docs live at routeBasePath: '/' so indexing "pages" is the most reliable way
        // to cover the whole site (docs + release notes) without relying on docsRouteBasePath.
        indexPages: true,
        indexDocs: false,
        indexBlog: false,
      },
    ],
  ],

  themeConfig: {
    // Replace with your project's social card
    image: 'img/docusaurus-social-card.jpg',
    colorMode: {
      defaultMode: 'dark',
      respectPrefersColorScheme: false,
      disableSwitch: true,
    },
    navbar: {
      title: 'Helios Docs',
      logo: {
        alt: 'Helios',
        src: 'img/logo.svg',
      },
      items: [
        {
          type: 'docSidebar',
          sidebarId: 'docsSidebar',
          position: 'left',
          label: 'Guides',
        },
        {to: '/release-notes', label: 'Release Notes', position: 'left'},
        {type: 'search', position: 'right'},
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Docs',
          items: [
            {label: 'Devices', to: '/devices/hvs-raze/overview'},
            {label: 'Pipelines', to: '/os/pipelines/overview'},
          ],
        },
      ],
      copyright: `© ${new Date().getFullYear()} Helios. Built with Docusaurus.`,
    },
    prism: {
      theme: prismThemes.vsDark,
      darkTheme: prismThemes.vsDark,
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
