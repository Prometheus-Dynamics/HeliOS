import type {ReactNode} from 'react';
import Link from '@docusaurus/Link';
import Heading from '@theme/Heading';
import styles from './styles.module.css';

type FeatureItem = {
  title: string;
  description: string;
  href: string;
  tag: string;
};

const FeatureList: FeatureItem[] = [
  {
    title: 'Install and first boot',
    description: 'Flash an image, power on, reach the UI, and confirm storage + networking.',
    href: '/getting-started/install',
    tag: 'Guide',
  },
  {
    title: 'Streams: register, configure, record',
    description: 'Register a stream, set aliases, enable shadow recorder, and capture media.',
    href: '/os/devices/streams/overview',
    tag: 'OS',
  },
  {
    title: 'Pipelines: author, deploy, tune',
    description: 'Build graphs, validate, attach per-stream, and tune safely.',
    href: '/os/pipelines/overview',
    tag: 'OS',
  },
  {
    title: 'Systems: logs, console, IMU, processes',
    description: 'Diagnose device issues using live logs, process telemetry, and sensor status.',
    href: '/os/systems/overview',
    tag: 'OS',
  },
  {
    title: 'Media: library, replay, export',
    description: 'Review recordings and snapshots, replay streams, and manage stored assets.',
    href: '/os/media/overview',
    tag: 'OS',
  },
  {
    title: 'API integration',
    description: 'Use HTTP, WebSockets, NT4, and the SDK bindings for external integrations.',
    href: '/api/http',
    tag: 'API',
  },
];

const flowSteps = [
  {
    title: 'Capture',
    detail: 'Register streams and stabilize formats/codecs.',
  },
  {
    title: 'Process',
    detail: 'Attach pipelines, select outputs, and tune per stream.',
  },
  {
    title: 'Observe',
    detail: 'Use metrics, logs, and Systems diagnostics to stay healthy.',
  },
  {
    title: 'Export',
    detail: 'Record media or publish results via API, NT4, and peers.',
  },
];

export default function HomepageFeatures(): ReactNode {
  return (
    <div className={styles.wrapper}>
      <section className={styles.section}>
        <div className={styles.sectionHeader}>
          <Heading as="h2">Start with the core paths</Heading>
          <p>
            Short, operator-first docs for the workflows you actually use day-to-day.
          </p>
        </div>
        <div className={styles.featureGrid}>
          {FeatureList.map((feature) => (
            <Link key={feature.title} className={styles.featureCard} to={feature.href}>
              <div className={styles.cardTop}>
                <span className={styles.cardTag}>{feature.tag}</span>
                <Heading as="h3">{feature.title}</Heading>
              </div>
              <p>{feature.description}</p>
            </Link>
          ))}
        </div>
      </section>

      <section className={styles.sectionAlt}>
        <div className={styles.sectionHeader}>
          <Heading as="h2">From device to result</Heading>
          <p>A simple mental model for how the OS pieces fit together.</p>
        </div>
        <div className={styles.flowGrid}>
          {flowSteps.map((step, index) => (
            <div key={step.title} className={styles.flowCard}>
              <span className={styles.flowIndex}>0{index + 1}</span>
              <Heading as="h3">{step.title}</Heading>
              <p>{step.detail}</p>
            </div>
          ))}
        </div>
      </section>
    </div>
  );
}
