import type {SidebarsConfig} from '@docusaurus/plugin-content-docs';

// This runs in Node.js - Don't use client-side code here (browser APIs, JSX...)

/**
 * Creating a sidebar enables you to:
 - create an ordered group of docs
 - render a sidebar for each doc of that group
 - provide next/previous navigation

 The sidebars can be generated from the filesystem, or explicitly defined here.

 Create as many sidebars as you want.
 */
const sidebars: SidebarsConfig = {
  docsSidebar: [
    'overview',
    'architecture',
    {
      type: 'category',
      label: 'Devices',
      items: [
        {
          type: 'category',
          label: 'HVS - Raze',
          items: [
            'devices/hvs-raze/overview',
            'devices/hvs-raze/specs',
            'devices/hvs-raze/power-ports',
            'devices/hvs-raze/buttons-leds',
            'devices/hvs-raze/install',
            'devices/hvs-raze/cameras',
          ],
        },
      ],
    },
    {
      type: 'category',
      label: 'Getting Started',
      items: [
        'getting-started/install',
        'getting-started/quickstart',
        'getting-started/update',
      ],
    },
    {
      type: 'category',
      label: 'Guides',
      items: [
        'guides/overview',
        {
          type: 'category',
          label: 'Setup + Bring-Up',
          items: [
            'guides/hardware-setup',
            'guides/flash-and-recover',
            'guides/power-on-robot',
            'guides/get-online',
            'guides/network-reset',
            'guides/first-login-and-sanity-check',
          ],
        },
        {
          type: 'category',
          label: 'Streams',
          items: [
            'guides/camera-setup',
            'guides/create-your-first-stream',
            'guides/ov9782-best-practices',
            'guides/usb-cameras',
            'guides/recording-and-capture-last',
            'guides/stream-performance',
          ],
        },
        {
          type: 'category',
          label: 'Pipelines + ArUco',
          items: [
            'guides/aruco-quickstart',
            'guides/pipeline-attach-and-output-select',
            'guides/tune-for-detection',
            'guides/pipeline-outputs-and-debugging',
            'guides/pipeline-validation-fixes',
            'guides/pipeline-import-export',
          ],
        },
        {
          type: 'category',
          label: 'Calibration + Rig',
          items: [
            'guides/calibration-start-to-finish',
            'guides/pose-and-rig-layout',
          ],
        },
        {
          type: 'category',
          label: 'Operations',
          items: [
            'guides/update-os-safely',
            'guides/firmware-bootloader-update',
            'guides/diagnostics-bundles-for-support',
            'guides/restarts-what-breaks',
            'guides/troubleshooting',
          ],
        },
        {
          type: 'category',
          label: 'UI',
          items: [
            'guides/floating-stream-viewer',
            'guides/alerts-and-error-history',
          ],
        },
        {
          type: 'category',
          label: 'Integrations',
          items: [
            'guides/networktables-nt4',
            'guides/peers-discovery-and-registration',
            'guides/helios-interdevice-protocol',
          ],
        },
      ],
    },
    {
      type: 'category',
      label: 'OS',
      items: [
        'os/dashboard',
        {
          type: 'category',
          label: 'Pipelines',
          items: [
            'os/pipelines/overview',
            'os/pipelines/workspace',
            'os/pipelines/node-registry',
            'os/pipelines/inputs-constants',
            'os/pipelines/validation',
            'os/pipelines/import-export',
            'os/pipelines/deploy',
            'os/pipelines/templates',
            'os/pipelines/tuning',
            'os/pipelines/metrics',
            'os/pipelines/pipeline-ui-elements',
            'os/pipelines/sdk',
          ],
        },
        {
          type: 'category',
          label: 'Devices',
          items: [
            'os/devices/overview',
            {
              type: 'category',
              label: 'Streams',
              items: [
                'os/devices/streams/overview',
                'os/devices/streams/inventory',
                'os/devices/streams/register',
                'os/devices/streams/page',
                'os/devices/streams/stream-tab',
                'os/devices/streams/recording',
                'os/devices/streams/controls-tab',
                'os/devices/streams/pipelines-tab',
                'os/devices/streams/pose-tab',
                'os/devices/streams/calibration-tab',
                'os/devices/streams/media-tab',
              ],
            },
            {
              type: 'category',
              label: 'Peripherals',
              items: [
                'os/devices/peripherals/overview',
                'os/devices/peripherals/coral',
                'os/devices/peripherals/fan',
                'os/devices/peripherals/lighting',
                'os/devices/peripherals/imu',
                'os/devices/peripherals/power',
              ],
            },
          ],
        },
        {
          type: 'category',
          label: 'Peers',
          items: [
            'os/peers/overview',
            'os/peers/discovery',
            'os/peers/registration',
            'os/peers/monitoring',
          ],
        },
        {
          type: 'category',
          label: 'Media',
          items: [
            'os/media/overview',
            'os/media/library',
            'os/media/upload',
            'os/media/asset-detail',
            'os/media/replay',
          ],
        },
        {
          type: 'category',
          label: 'Localization',
          items: [
            'os/localization/overview',
            'os/localization/sources',
            'os/localization/visualization',
            'os/localization/camera-pose',
            'os/localization/field-maps',
          ],
        },
        {
          type: 'category',
          label: 'Systems',
          items: [
            'os/systems/overview',
            'os/systems/logs',
            'os/systems/i2c',
            'os/systems/imu',
            'os/systems/console',
            'os/systems/processes',
          ],
        },
        'os/docs',
        {
          type: 'category',
          label: 'Settings',
          items: [
            'os/settings/overview',
            'os/settings/networking',
            'os/settings/rig-layout',
            'os/settings/diagnostics',
            'os/settings/updater',
            'os/settings/plugins',
            'os/settings/usb-power',
            'os/settings/firmware',
            'os/settings/api-endpoint',
            'os/settings/restart',
          ],
        },
      ],
    },
    // Pipelines authoring lives under OS > Pipelines.
    // Keep /pipelines/overview as a legacy route for older links, but do not surface it in the sidebar.
    {
      type: 'category',
      label: 'API',
      items: [
        'api/http',
        'api/websockets',
        'api/networktables',
        'api/sdk',
      ],
    },
    {
      type: 'category',
      label: 'Reference',
      items: [
        'reference/engineering-review',
        'reference/hardware',
        'reference/api',
        'reference/sdk',
      ],
    },
  ],
};

export default sidebars;
